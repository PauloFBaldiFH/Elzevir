// SPDX-License-Identifier: GPL-3.0-or-later
//! What the Elzevir holds under Koha's numbers, read from the edge tables for
//! `elzevir verify-koha` (docs/specs/migracao/verificacao-do-koha.md, item 2).

use elzevir_app::koha_verify::{Footprint, FoundItem, FoundPatron};
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
