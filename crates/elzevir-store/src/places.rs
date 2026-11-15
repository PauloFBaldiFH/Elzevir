// SPDX-License-Identifier: GPL-3.0-or-later
//! The edge table of Koha codes (docs/specs/catalogo/bibframe-completo.md, items 23a
//! and 23b): the only place where a branch, a shelving place or a collection has a code.
//! Only the MARC import and export and the naming from Koha read it.

use rusqlite::{Connection, OptionalExtension, params};

use crate::StoreError;

/// What a row of `koha_codes` names. The numbers are stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Coded {
    /// A branch (`952 $a`, `$b`).
    Branch = 1,
    /// A shelving place of a branch (`952 $c`).
    Sublocation = 2,
    /// A collection of a branch (`952 $8`).
    Collection = 3,
    /// A circulation policy, from a Koha item type (`952 $y`, `942 $c`).
    Policy = 4,
}

/// The constant SQL of one per-branch list.
pub(crate) struct List {
    coded: Coded,
    find: &'static str,
    named: &'static str,
    insert: &'static str,
}

/// Shelving places.
pub(crate) const SUBLOCATIONS: List = List {
    coded: Coded::Sublocation,
    find: "SELECT p.id FROM sublocations p
           JOIN koha_codes k ON k.kind = 2 AND k.local_id = p.id
           WHERE p.branch_id = ?1 AND k.code = ?2",
    named: "SELECT 1 FROM sublocations WHERE branch_id = ?1 AND name = ?2",
    insert: "INSERT INTO sublocations (branch_id, name) VALUES (?1, ?2)",
};

/// Collections.
pub(crate) const COLLECTIONS: List = List {
    coded: Coded::Collection,
    find: "SELECT p.id FROM collections p
           JOIN koha_codes k ON k.kind = 3 AND k.local_id = p.id
           WHERE p.branch_id = ?1 AND k.code = ?2",
    named: "SELECT 1 FROM collections WHERE branch_id = ?1 AND name = ?2",
    insert: "INSERT INTO collections (branch_id, name) VALUES (?1, ?2)",
};

/// The row that `code` stands for, if any.
pub(crate) fn by_code(
    connection: &Connection,
    coded: Coded,
    code: &str,
) -> Result<Option<i64>, StoreError> {
    Ok(connection
        .query_row(
            "SELECT local_id FROM koha_codes WHERE kind = ?1 AND code = ?2 ORDER BY local_id",
            params![coded as i64, code],
            |row| row.get(0),
        )
        .optional()?)
}

/// Gives `id` the code `code`.
pub(crate) fn set_code(
    connection: &Connection,
    coded: Coded,
    id: i64,
    code: &str,
) -> Result<(), StoreError> {
    connection.execute(
        "INSERT OR REPLACE INTO koha_codes (kind, local_id, code) VALUES (?1, ?2, ?3)",
        params![coded as i64, id, code],
    )?;
    Ok(())
}

/// The row of `list` in `branch` that `code` stands for, created on first use with the
/// code as its provisional name (item 23b) or with `name` when one is known.
pub(crate) fn place(
    connection: &Connection,
    list: &List,
    branch: i64,
    code: &str,
    name: Option<&str>,
) -> Result<i64, StoreError> {
    let found: Option<i64> = connection
        .query_row(list.find, params![branch, code], |row| row.get(0))
        .optional()?;
    if let Some(id) = found {
        return Ok(id);
    }
    let name = name.unwrap_or(code);
    // Two codes may share a name in Koha; names are unique within a branch here.
    let taken = connection
        .query_row(list.named, params![branch, name], |_| Ok(()))
        .optional()?
        .is_some();
    let name = if taken {
        format!("{name} ({code})")
    } else {
        name.to_owned()
    };
    connection.execute(list.insert, params![branch, name])?;
    let id = connection.last_insert_rowid();
    set_code(connection, list.coded, id, code)?;
    Ok(id)
}

/// An Item as migration 14 found it: id, home branch, location code and label,
/// collection code and label.
type OldItem = (
    i64,
    i64,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// Fills the registries for what migration 14 found: one row per branch that uses each
/// shelving location or collection code, named as the code table named it.
pub(crate) fn backfill(connection: &Connection) -> Result<(), StoreError> {
    connection.execute(
        "INSERT OR IGNORE INTO koha_codes (kind, local_id, code) SELECT 1, id, code FROM branches",
        [],
    )?;
    let mut items = connection.prepare(
        "SELECT i.id, i.home_branch, l.code, l.label, c.code, c.label FROM items i
         LEFT JOIN terms l ON l.id = i.location
         LEFT JOIN terms c ON c.id = i.collection
         ORDER BY i.id",
    )?;
    let rows = items
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        })?
        .collect::<Result<Vec<OldItem>, _>>()?;
    for (item, branch, location, location_name, collection, collection_name) in rows {
        let sublocation = location
            .map(|code| {
                place(
                    connection,
                    &SUBLOCATIONS,
                    branch,
                    &code,
                    location_name.as_deref(),
                )
            })
            .transpose()?;
        let collection = collection
            .map(|code| {
                place(
                    connection,
                    &COLLECTIONS,
                    branch,
                    &code,
                    collection_name.as_deref(),
                )
            })
            .transpose()?;
        connection.execute(
            "UPDATE items SET sublocation_id = ?2, collection_id = ?3 WHERE id = ?1",
            params![item, sublocation, collection],
        )?;
    }
    Ok(())
}
