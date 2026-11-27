// SPDX-License-Identifier: GPL-3.0-or-later
//! What the Elzevir holds under Koha's numbers, read from the edge tables for
//! `elzevir verify-koha` (docs/specs/migracao/verificacao-do-koha.md, item 2).

use elzevir_app::koha_verify::{
    Footprint, FoundHold, FoundItem, FoundLoan, FoundPatron, HoldState,
};
use rusqlite::{Connection, params};

use crate::StoreError;
use crate::koha_migration::Keyed;
use crate::places::Coded;

impl crate::ElzevirStore {
    /// The branches, patron categories, patrons, records and Items the Elzevir holds
    /// under Koha's numbers.
    ///
    /// # Errors
    /// The database could not be read.
    pub fn koha_footprint(&self) -> Result<Footprint, StoreError> {
        self.readers().read(|connection| {
            Ok(Footprint {
                branches: branches(connection)?,
                categories: categories(connection)?,
                patrons: patrons(connection)?,
                records: records(connection)?,
                items: items(connection)?,
                loans: loans(connection)?,
                holds: holds(connection)?,
                accounts: accounts(connection)?,
                returned: returned(connection)?,
                item_types: item_types(connection)?,
                rules: rules(connection)?,
            })
        })
    }
}

fn branches(
    connection: &Connection,
) -> Result<std::collections::BTreeMap<String, String>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT k.code, b.name FROM koha_codes k JOIN branches b ON b.id = k.local_id
         WHERE k.kind = ?1",
    )?;
    let rows = statement
        .query_map(params![Coded::Branch as i64], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

fn categories(connection: &Connection) -> Result<std::collections::BTreeSet<String>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT k.koha_key FROM koha_keys k JOIN patron_categories c ON c.id = k.local_id
         WHERE k.kind = ?1",
    )?;
    let rows = statement
        .query_map(params![Keyed::Category as i64], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

fn patrons(
    connection: &Connection,
) -> Result<std::collections::BTreeMap<String, FoundPatron>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT k.koha_key, p.cardnumber,
                (SELECT code FROM koha_codes WHERE kind = ?2 AND local_id = p.branch_id),
                (SELECT koha_key FROM koha_keys WHERE kind = ?3 AND local_id = p.category_id
                 ORDER BY koha_key LIMIT 1),
                p.anonymized_at IS NOT NULL
         FROM koha_keys k JOIN patrons p ON p.id = k.local_id WHERE k.kind = ?1",
    )?;
    let rows = statement
        .query_map(
            params![
                Keyed::Patron as i64,
                Coded::Branch as i64,
                Keyed::Category as i64
            ],
            |row| {
                Ok((
                    row.get(0)?,
                    FoundPatron {
                        cardnumber: row.get(1)?,
                        branch: row.get(2)?,
                        category: row.get(3)?,
                        anonymized: row.get(4)?,
                    },
                ))
            },
        )?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

fn records(
    connection: &Connection,
) -> Result<std::collections::BTreeMap<String, usize>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT external_id, count(DISTINCT instance_id) FROM import_identities
         WHERE entity = 'instance' AND system = 'koha.biblionumber' GROUP BY external_id",
    )?;
    let rows = statement
        .query_map([], |row| {
            let count: i64 = row.get(1)?;
            Ok((row.get(0)?, usize::try_from(count).unwrap_or(usize::MAX)))
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

fn items(
    connection: &Connection,
) -> Result<std::collections::BTreeMap<String, Vec<FoundItem>>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT DISTINCT o.external_id,
                (SELECT external_id FROM import_identities
                 WHERE instance_id = i.instance_id AND entity = 'instance'
                   AND system = 'koha.biblionumber' ORDER BY external_id LIMIT 1),
                i.barcode,
                (SELECT code FROM koha_codes WHERE kind = ?1 AND local_id = i.home_branch),
                (SELECT code FROM koha_codes WHERE kind = ?1 AND local_id = i.holding_branch),
                (SELECT code FROM koha_codes WHERE kind = ?2 AND local_id = i.policy_id),
                i.id
         FROM item_origins o JOIN items i ON i.id = o.item_id
         WHERE o.system = 'koha.itemnumber'
         ORDER BY 1",
    )?;
    let mut found: std::collections::BTreeMap<String, Vec<FoundItem>> =
        std::collections::BTreeMap::new();
    let mut rows = statement.query(params![Coded::Branch as i64, Coded::Policy as i64])?;
    while let Some(row) = rows.next()? {
        found.entry(row.get(0)?).or_default().push(FoundItem {
            biblionumber: row.get(1)?,
            barcode: row.get(2)?,
            home: row.get(3)?,
            holding: row.get(4)?,
            item_type: row.get(5)?,
        });
    }
    Ok(found)
}

fn loans(
    connection: &Connection,
) -> Result<std::collections::BTreeMap<String, Vec<FoundLoan>>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT DISTINCT o.external_id,
                (SELECT koha_key FROM koha_keys WHERE kind = ?1 AND local_id = l.patron_id),
                l.due, l.id
         FROM loans l JOIN item_origins o ON o.item_id = l.item_id
         WHERE o.system = 'koha.itemnumber'
         ORDER BY 1",
    )?;
    let mut found: std::collections::BTreeMap<String, Vec<FoundLoan>> =
        std::collections::BTreeMap::new();
    let mut rows = statement.query(params![Keyed::Patron as i64])?;
    while let Some(row) = rows.next()? {
        found.entry(row.get(0)?).or_default().push(FoundLoan {
            borrowernumber: row.get(1)?,
            due: row.get(2)?,
        });
    }
    Ok(found)
}

fn holds(
    connection: &Connection,
) -> Result<std::collections::BTreeMap<String, Vec<FoundHold>>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT (SELECT external_id FROM import_identities
                 WHERE instance_id = h.instance_id AND entity = 'instance'
                   AND system = 'koha.biblionumber' ORDER BY external_id LIMIT 1) AS record,
                (SELECT koha_key FROM koha_keys WHERE kind = ?1 AND local_id = h.patron_id),
                h.found, h.priority
         FROM holds h ORDER BY record, h.priority, h.id",
    )?;
    let mut found: std::collections::BTreeMap<String, Vec<FoundHold>> =
        std::collections::BTreeMap::new();
    let mut rows = statement.query(params![Keyed::Patron as i64])?;
    while let Some(row) = rows.next()? {
        // A hold of a record that did not come from Koha has nothing to compare with.
        let Some(record) = row.get::<_, Option<String>>(0)? else {
            continue;
        };
        let state = match row.get::<_, Option<String>>(2)?.as_deref() {
            Some("W") => HoldState::Waiting,
            Some(_) => HoldState::InTransit,
            None => HoldState::Queued,
        };
        found.entry(record).or_default().push(FoundHold {
            borrowernumber: row.get(1)?,
            state,
            priority: row.get(3)?,
        });
    }
    Ok(found)
}

fn accounts(
    connection: &Connection,
) -> Result<std::collections::BTreeMap<String, (i64, usize)>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT k.koha_key, sum(a.outstanding), count(*)
         FROM account_lines a JOIN koha_keys k ON k.kind = ?1 AND k.local_id = a.patron_id
         GROUP BY k.koha_key",
    )?;
    let rows = statement
        .query_map(params![Keyed::Patron as i64], |row| {
            let count: i64 = row.get(2)?;
            Ok((
                row.get(0)?,
                (row.get(1)?, usize::try_from(count).unwrap_or(usize::MAX)),
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

fn returned(
    connection: &Connection,
) -> Result<std::collections::BTreeMap<String, bool>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT k.koha_key, r.patron_id IS NOT NULL
         FROM koha_keys k JOIN returned_loans r ON r.id = k.local_id WHERE k.kind = ?1",
    )?;
    let rows = statement
        .query_map(params![Keyed::ReturnedLoan as i64], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

fn item_types(connection: &Connection) -> Result<std::collections::BTreeSet<String>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT k.code FROM koha_codes k JOIN circulation_policies p ON p.id = k.local_id
         WHERE k.kind = ?1",
    )?;
    let rows = statement
        .query_map(params![Coded::Policy as i64], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

fn rules(
    connection: &Connection,
) -> Result<std::collections::BTreeMap<elzevir_app::koha_verify::RuleKey, String>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT CASE WHEN r.branch_id IS NULL THEN NULL ELSE coalesce(
                    (SELECT code FROM koha_codes WHERE kind = ?1 AND local_id = r.branch_id),
                    '#' || r.branch_id) END,
                CASE WHEN r.category_id IS NULL THEN NULL ELSE coalesce(
                    (SELECT koha_key FROM koha_keys WHERE kind = ?2 AND local_id = r.category_id
                     ORDER BY koha_key LIMIT 1),
                    '#' || r.category_id) END,
                CASE WHEN r.policy_id IS NULL THEN NULL ELSE coalesce(
                    (SELECT code FROM koha_codes WHERE kind = ?3 AND local_id = r.policy_id),
                    '#' || r.policy_id) END,
                r.kind, r.value
         FROM circulation_rules r",
    )?;
    let rows = statement
        .query_map(
            params![
                Coded::Branch as i64,
                Keyed::Category as i64,
                Coded::Policy as i64
            ],
            |row| {
                Ok((
                    (row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?),
                    row.get(4)?,
                ))
            },
        )?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}
