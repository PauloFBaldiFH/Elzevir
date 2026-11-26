// SPDX-License-Identifier: GPL-3.0-or-later
//! `patrons.*`: patrons and patron categories (docs/specs/circulacao/leitores.md).

use elzevir_app::commands::{WriteOutput, WriteRefusal};
use elzevir_app::patrons::{
    CategoryChange, PatronCommand, PatronField, PatronFields, PatronOutput, PatronRefusal,
    PatronUpdate, ReadingHistory, checked_category, checked_patron,
};
use elzevir_domain::patron::{Enrolment, PatronCategoryId, PatronId};
use jiff::civil::Date;
use rusqlite::{Connection, OptionalExtension, params};

use crate::{CommandFailed, StoreError, WriteTx};

type Applied = Result<WriteOutput, CommandFailed<WriteRefusal>>;

pub(crate) const fn refused(refusal: PatronRefusal) -> CommandFailed<WriteRefusal> {
    CommandFailed::Rejected(WriteRefusal::Patrons(refusal))
}

/// Applies one patron command inside the writer's transaction.
pub(crate) fn apply(tx: &WriteTx<'_>, command: &PatronCommand) -> Applied {
    let output = match command {
        PatronCommand::ChangeCategory(change) => change_category(tx.connection(), change)?,
        PatronCommand::Add(fields) => add(tx.connection(), fields)?,
        PatronCommand::Update(update) => update_patron(tx.connection(), update)?,
        PatronCommand::View(patron)
        | PatronCommand::RevealCpf(patron)
        | PatronCommand::Export(patron) => {
            if !crate::catalog::exists(
                tx.connection(),
                "SELECT 1 FROM patrons WHERE id = ?1 AND anonymized_at IS NULL",
                patron.0,
            )? {
                return Err(refused(PatronRefusal::UnknownPatron));
            }
            if matches!(command, PatronCommand::Export(_)) {
                PatronOutput::Exported(*patron)
            } else {
                PatronOutput::Viewed(*patron)
            }
        }
        PatronCommand::SetPassword {
            patron,
            password_hash,
        } => {
            let changed = tx
                .connection()
                .execute(
                    "UPDATE patrons SET password_hash = ?1 WHERE id = ?2",
                    params![password_hash, patron.0],
                )
                .map_err(StoreError::from)?;
            if changed == 0 {
                return Err(refused(PatronRefusal::UnknownPatron));
            }
            PatronOutput::PasswordSet(*patron)
        }
        PatronCommand::Anonymize(patron) => {
            crate::patron_privacy::anonymize(tx.connection(), *patron)?
        }
        PatronCommand::SetReadingHistory {
            patron,
            reading_history,
        } => {
            // The version moves, so a staff edit begun before does not undo the choice.
            let changed = tx
                .connection()
                .execute(
                    "UPDATE patrons SET reading_history = ?1, row_version = row_version + 1
                     WHERE id = ?2",
                    params![
                        i64::from(*reading_history == ReadingHistory::Kept),
                        patron.0
                    ],
                )
                .map_err(StoreError::from)?;
            if changed == 0 {
                return Err(refused(PatronRefusal::UnknownPatron));
            }
            PatronOutput::ReadingHistorySet(*patron)
        }
        PatronCommand::MigrateKoha(batch) => crate::koha_migration::apply(tx.connection(), batch)?,
        PatronCommand::MigrateKohaCirculation(batch) => {
            crate::koha_circulation::apply(tx.connection(), batch)?
        }
        PatronCommand::MigrateKohaAccounts(batch) => {
            crate::koha_accounts::apply(tx.connection(), batch)?
        }
        PatronCommand::MigrateKohaHistory(batch) => {
            crate::koha_history::apply(tx.connection(), batch)?
        }
    };
    Ok(WriteOutput::Patrons(output))
}

fn change_category(
    connection: &Connection,
    change: &CategoryChange,
) -> Result<PatronOutput, CommandFailed<WriteRefusal>> {
    let taken = |name: &str, id: i64| -> Result<bool, StoreError> {
        Ok(connection
            .query_row(
                "SELECT 1 FROM patron_categories WHERE name = ?1 AND id <> ?2",
                params![name, id],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    };
    match checked_category(change.clone()).map_err(refused)? {
        CategoryChange::Create { name, enrolment } => {
            if taken(&name, 0)? {
                return Err(refused(PatronRefusal::CategoryNameTaken));
            }
            let (months, until) = enrolment_columns(enrolment);
            connection
                .execute(
                    "INSERT INTO patron_categories (name, enrolment_months, enrolment_until)
                     VALUES (?1, ?2, ?3)",
                    params![name, months, until],
                )
                .map_err(StoreError::from)?;
            Ok(PatronOutput::CategoryChanged(Some(PatronCategoryId(
                connection.last_insert_rowid(),
            ))))
        }
        CategoryChange::Update {
            category,
            name,
            enrolment,
        } => {
            if taken(&name, category.0)? {
                return Err(refused(PatronRefusal::CategoryNameTaken));
            }
            let (months, until) = enrolment_columns(enrolment);
            let changed = connection
                .execute(
                    "UPDATE patron_categories
                     SET name = ?2, enrolment_months = ?3, enrolment_until = ?4,
                         row_version = row_version + 1
                     WHERE id = ?1",
                    params![category.0, name, months, until],
                )
                .map_err(StoreError::from)?;
            if changed == 0 {
                return Err(refused(PatronRefusal::UnknownCategory));
            }
            Ok(PatronOutput::CategoryChanged(Some(category)))
        }
        CategoryChange::Delete(category) => {
            let users: i64 = connection
                .query_row(
                    "SELECT (SELECT count(*) FROM patrons WHERE category_id = ?1)
                          + (SELECT count(*) FROM circulation_rules WHERE category_id = ?1)
                          + (SELECT count(*) FROM overdue_notice_rules WHERE category_id = ?1)
                          + (SELECT count(*) FROM hold_notice_preferences WHERE category_id = ?1)",
                    [category.0],
                    |row| row.get(0),
                )
                .map_err(StoreError::from)?;
            if users > 0 {
                return Err(refused(PatronRefusal::CategoryInUse));
            }
            let deleted = connection
                .execute("DELETE FROM patron_categories WHERE id = ?1", [category.0])
                .map_err(StoreError::from)?;
            if deleted == 0 {
                return Err(refused(PatronRefusal::UnknownCategory));
            }
            Ok(PatronOutput::CategoryChanged(None))
        }
    }
}

pub(crate) fn enrolment_columns(enrolment: Enrolment) -> (Option<u16>, Option<String>) {
    match enrolment {
        Enrolment::Months(months) => (Some(months), None),
        Enrolment::Until(day) => (None, Some(day.to_string())),
    }
}

/// The category's period, read from its columns.
fn enrolment_of(
    connection: &Connection,
    category: PatronCategoryId,
) -> Result<Option<Enrolment>, StoreError> {
    let row: Option<(Option<u16>, Option<String>)> = connection
        .query_row(
            "SELECT enrolment_months, enrolment_until FROM patron_categories WHERE id = ?1",
            [category.0],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    Ok(match row {
        Some((Some(months), _)) => Some(Enrolment::Months(months)),
        Some((None, Some(day))) => Some(Enrolment::Until(day.parse().map_err(|_| {
            StoreError::Sqlite(format!("patron category {} has date `{day}`", category.0))
        })?)),
        _ => None,
    })
}

/// The checked fields with the expiry filled in, after every reference and unique value
/// is confirmed; `patron` is the one being edited (0 for a new one).
fn ready(
    connection: &Connection,
    fields: &PatronFields,
    patron: i64,
) -> Result<(PatronFields, Date), CommandFailed<WriteRefusal>> {
    let fields = checked_patron(fields.clone()).map_err(refused)?;
    let found = |sql: &str, value: &dyn rusqlite::ToSql| -> Result<bool, StoreError> {
        Ok(connection
            .query_row(sql, params![value, patron], |_| Ok(()))
            .optional()?
            .is_some())
    };
    if !crate::catalog::exists(
        connection,
        "SELECT 1 FROM branches WHERE id = ?1",
        fields.branch.0,
    )? {
        return Err(refused(PatronRefusal::UnknownBranch));
    }
    let Some(enrolment) = enrolment_of(connection, fields.category)? else {
        return Err(refused(PatronRefusal::UnknownCategory));
    };
    let expires = match fields.expires {
        Some(day) => day,
        None => enrolment
            .expiry(fields.enrolled)
            .ok_or(refused(PatronRefusal::BadField(PatronField::Expires)))?,
    };
    if found(
        "SELECT 1 FROM patrons WHERE cardnumber = ?1 AND id <> ?2",
        &fields.cardnumber,
    )? {
        return Err(refused(PatronRefusal::CardnumberTaken));
    }
    if let Some(cpf) = &fields.cpf
        && found(
            "SELECT 1 FROM patrons WHERE cpf = ?1 AND id <> ?2",
            &cpf.digits(),
        )?
    {
        return Err(refused(PatronRefusal::CpfTaken));
    }
    Ok((fields, expires))
}

/// The columns of `patrons` after `id`, in the order [`values`] gives them.
const COLUMNS: &str = "cardnumber, surname, first_name, cpf, date_of_birth, email, phone, \
                       address, branch_id, category_id, enrolled, expires, staff_note, \
                       reading_history";

fn values(fields: &PatronFields, expires: Date) -> [Box<dyn rusqlite::ToSql + '_>; 14] {
    [
        Box::new(&fields.cardnumber),
        Box::new(&fields.surname),
        Box::new(&fields.first_name),
        Box::new(fields.cpf.as_ref().map(|cpf| cpf.digits().to_owned())),
        Box::new(fields.date_of_birth.map(|day| day.to_string())),
        Box::new(&fields.email),
        Box::new(&fields.phone),
        Box::new(&fields.address),
        Box::new(fields.branch.0),
        Box::new(fields.category.0),
        Box::new(fields.enrolled.to_string()),
        Box::new(expires.to_string()),
        Box::new(&fields.staff_note),
        Box::new(match fields.reading_history {
            ReadingHistory::NotKept => 0,
            ReadingHistory::Kept => 1,
        }),
    ]
}

/// Adds a patron whose fields pass every check.
pub(crate) fn add(
    connection: &Connection,
    fields: &PatronFields,
) -> Result<PatronOutput, CommandFailed<WriteRefusal>> {
    let (fields, expires) = ready(connection, fields, 0)?;
    let values = values(&fields, expires);
    connection
        .execute(
            &format!(
                "INSERT INTO patrons ({COLUMNS})
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)"
            ),
            rusqlite::params_from_iter(values.iter()),
        )
        .map_err(StoreError::from)?;
    Ok(PatronOutput::Added(PatronId(
        connection.last_insert_rowid(),
    )))
}

fn update_patron(
    connection: &Connection,
    update: &PatronUpdate,
) -> Result<PatronOutput, CommandFailed<WriteRefusal>> {
    let version: Option<i64> = connection
        .query_row(
            "SELECT row_version FROM patrons WHERE id = ?1",
            [update.patron.0],
            |row| row.get(0),
        )
        .optional()
        .map_err(StoreError::from)?;
    match version {
        None => return Err(refused(PatronRefusal::UnknownPatron)),
        Some(version) if version != update.row_version => {
            return Err(refused(PatronRefusal::Stale));
        }
        Some(_) => {}
    }
    let (fields, expires) = ready(connection, &update.fields, update.patron.0)?;
    let mut values: Vec<Box<dyn rusqlite::ToSql + '_>> =
        values(&fields, expires).into_iter().collect();
    values.push(Box::new(update.patron.0));
    let assignments = COLUMNS
        .split(", ")
        .zip(1..)
        .map(|(column, n)| format!("{column} = ?{n}"))
        .collect::<Vec<_>>()
        .join(", ");
    // `assignments` is built from the constant column list, never from input.
    connection
        .execute(
            &format!(
                "UPDATE patrons SET {assignments}, row_version = row_version + 1
                 WHERE id = ?15"
            ),
            rusqlite::params_from_iter(values.iter()),
        )
        .map_err(StoreError::from)?;
    Ok(PatronOutput::Updated(update.patron))
}
