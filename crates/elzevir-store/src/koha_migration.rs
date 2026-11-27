// SPDX-License-Identifier: GPL-3.0-or-later
//! `patrons.migrate_koha`: one write of the migration of patrons from Koha
//! (docs/specs/migracao/leitores-do-koha.md). Koha's codes go only to the edge tables
//! `koha_codes` (branches) and `koha_keys` (categories, patrons).

use elzevir_app::commands::WriteRefusal;
use elzevir_app::koha_migration::{KohaBatch, KohaBranch, KohaCategory, KohaMigrated, KohaPatron};
use elzevir_app::patrons::{CategoryChange, PatronOutput, checked_category};
use elzevir_domain::catalog::BranchId;
use elzevir_domain::patron::PatronCategoryId;
use rusqlite::{Connection, OptionalExtension, params};

use crate::places::{Coded, by_code, set_code};
use crate::{CommandFailed, StoreError};

/// What `koha_keys` names. The numbers are stored.
#[derive(Clone, Copy)]
pub(crate) enum Keyed {
    Category = 1,
    Patron = 2,
    /// A returned loan, by `issue_id` (circulacao-do-koha.md, item 16).
    ReturnedLoan = 3,
}

pub(crate) fn key(
    connection: &Connection,
    keyed: Keyed,
    key: &str,
) -> Result<Option<i64>, StoreError> {
    Ok(connection
        .query_row(
            "SELECT local_id FROM koha_keys WHERE kind = ?1 AND koha_key = ?2",
            params![keyed as i64, key],
            |row| row.get(0),
        )
        .optional()?)
}

pub(crate) fn set_key(
    connection: &Connection,
    keyed: Keyed,
    key: &str,
    id: i64,
) -> Result<(), StoreError> {
    connection.execute(
        "INSERT OR REPLACE INTO koha_keys (kind, koha_key, local_id) VALUES (?1, ?2, ?3)",
        params![keyed as i64, key, id],
    )?;
    Ok(())
}

/// Applies one write: branches, categories, then patrons and their restrictions.
pub(crate) fn apply(
    connection: &Connection,
    batch: &KohaBatch,
) -> Result<PatronOutput, CommandFailed<WriteRefusal>> {
    let mut done = KohaMigrated::default();
    for branch in &batch.branches {
        save_branch(connection, branch, &mut done)?;
    }
    for category in &batch.categories {
        save_category(connection, category, &mut done)?;
    }
    for patron in &batch.patrons {
        add_patron(connection, patron, &mut done)?;
    }
    Ok(PatronOutput::KohaMigrated(done))
}

/// Item 5: the branch with the code takes Koha's data; a new code is a new branch.
fn save_branch(
    connection: &Connection,
    branch: &KohaBranch,
    done: &mut KohaMigrated,
) -> Result<(), StoreError> {
    let values = params![
        branch.name,
        branch.address,
        branch.phone,
        branch.email,
        branch.url
    ];
    if let Some(id) = by_code(connection, Coded::Branch, &branch.code)? {
        connection.execute(
            "UPDATE branches SET name = ?1, address = ?2, phone = ?3, email = ?4, url = ?5,
                 row_version = row_version + 1
             WHERE id = ?6",
            params![
                branch.name,
                branch.address,
                branch.phone,
                branch.email,
                branch.url,
                id
            ],
        )?;
        done.branches_updated += 1;
    } else {
        connection.execute(
            "INSERT INTO branches (name, address, phone, email, url) VALUES (?1, ?2, ?3, ?4, ?5)",
            values,
        )?;
        set_code(
            connection,
            Coded::Branch,
            connection.last_insert_rowid(),
            &branch.code,
        )?;
        done.branches_created += 1;
    }
    Ok(())
}

/// Item 7: a category migrated before stays; one with the name is used; else a new one.
fn save_category(
    connection: &Connection,
    category: &KohaCategory,
    done: &mut KohaMigrated,
) -> Result<(), CommandFailed<WriteRefusal>> {
    if key(connection, Keyed::Category, &category.code)?.is_some() {
        return Ok(());
    }
    let change = checked_category(CategoryChange::Create {
        name: category.name.clone(),
        enrolment: category.enrolment,
    })
    .map_err(|refusal| CommandFailed::Rejected(WriteRefusal::Patrons(refusal)))?;
    let CategoryChange::Create { name, enrolment } = change else {
        return Err(StoreError::Sqlite("a category check changed its kind".to_owned()).into());
    };
    let named: Option<i64> = connection
        .query_row(
            "SELECT id FROM patron_categories WHERE name = ?1",
            [&name],
            |row| row.get(0),
        )
        .optional()
        .map_err(StoreError::from)?;
    let id = if let Some(id) = named {
        done.categories_linked += 1;
        id
    } else {
        let (months, until) = crate::patrons::enrolment_columns(enrolment);
        connection
            .execute(
                "INSERT INTO patron_categories (name, enrolment_months, enrolment_until)
                 VALUES (?1, ?2, ?3)",
                params![name, months, until],
            )
            .map_err(StoreError::from)?;
        done.categories_created += 1;
        connection.last_insert_rowid()
    };
    set_key(connection, Keyed::Category, &category.code, id)?;
    Ok(())
}

/// Items 11 to 15: the patron, unless a former run added it or the card is taken.
fn add_patron(
    connection: &Connection,
    patron: &KohaPatron,
    done: &mut KohaMigrated,
) -> Result<(), CommandFailed<WriteRefusal>> {
    let number = patron.borrowernumber.to_string();
    if let Some(id) = key(connection, Keyed::Patron, &number)? {
        done.already_migrated += 1;
        // A run before the passwords came (item 12) left them out; a set one stays.
        done.passwords += set_password(connection, id, patron, "password_hash IS NULL")?;
        return Ok(());
    }
    let taken = connection
        .query_row(
            "SELECT 1 FROM patrons WHERE cardnumber = ?1",
            [&patron.cardnumber],
            |_| Ok(()),
        )
        .optional()
        .map_err(StoreError::from)?
        .is_some();
    if taken {
        done.cardnumber_taken.push(patron.borrowernumber);
        return Ok(());
    }
    let missing = |what: &str| StoreError::Sqlite(format!("Koha {what} not in this migration"));
    let branch = by_code(connection, Coded::Branch, &patron.branch)?.ok_or(missing("branch"))?;
    let category =
        key(connection, Keyed::Category, &patron.category)?.ok_or(missing("category"))?;
    let fields = patron.fields(BranchId(branch), PatronCategoryId(category));
    let PatronOutput::Added(id) = crate::patrons::add(connection, &fields)? else {
        return Err(StoreError::Sqlite("adding a patron answered otherwise".to_owned()).into());
    };
    set_key(connection, Keyed::Patron, &number, id.0)?;
    done.passwords += set_password(connection, id.0, patron, "1")?;
    for restriction in &patron.restrictions {
        connection
            .execute(
                "INSERT INTO patron_restrictions (patron_id, kind, expires, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    id.0,
                    restriction.kind.as_str(),
                    restriction.expires.map(|day| day.to_string()),
                    restriction.created_at
                ],
            )
            .map_err(StoreError::from)?;
        done.restrictions += 1;
    }
    done.patrons_added += 1;
    Ok(())
}

/// Gives the patron Koha's hash when `condition`, a constant of this file, holds; 1 when
/// it did.
fn set_password(
    connection: &Connection,
    id: i64,
    patron: &KohaPatron,
    condition: &'static str,
) -> Result<usize, StoreError> {
    let Some(hash) = &patron.password_hash else {
        return Ok(0);
    };
    // `condition` is one of two constants above, never input.
    Ok(connection.execute(
        &format!("UPDATE patrons SET password_hash = ?1 WHERE id = ?2 AND {condition}"),
        params![hash, id],
    )?)
}

impl crate::ElzevirStore {
    /// Whether the catalog has an Instance or the store holds a patron migrated from Koha,
    /// which makes the folder unfit for `switch-from-koha` (troca-do-koha.md, item 3).
    ///
    /// # Errors
    /// The database could not be read.
    pub fn holds_catalog_or_koha_patrons(&self) -> Result<bool, StoreError> {
        self.readers()
            .read(|connection| {
                connection.query_row(
                    "SELECT EXISTS (SELECT 1 FROM instances)
                         OR EXISTS (SELECT 1 FROM koha_keys WHERE kind = ?1)",
                    params![Keyed::Patron as i64],
                    |row| row.get(0),
                )
            })
            .map_err(StoreError::from)
    }
}
