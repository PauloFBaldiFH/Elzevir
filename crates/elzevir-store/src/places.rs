// SPDX-License-Identifier: GPL-3.0-or-later
//! Branches, sublocations and collections as registries with generated ids, and the edge
//! table that keeps their Koha codes (docs/specs/catalogo/bibframe-completo.md, items 23a
//! and 23b).
//!
//! Part 4a-1: the registries are filled and kept in step with the codes the Items still
//! carry; part 4a-2 makes the Items point at them alone.

use rusqlite::{Connection, OptionalExtension, params};

use crate::StoreError;

/// What a row of `koha_codes` names. The numbers are stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Coded {
    /// A branch (`952 $a`, `$b`).
    Branch = 1,
    /// A sublocation of a branch (`952 $c`).
    Sublocation = 2,
    /// A collection of a branch (`952 $8`).
    Collection = 3,
}

/// The constant SQL of one per-branch list.
struct List {
    coded: Coded,
    find: &'static str,
    named: &'static str,
    insert: &'static str,
}

const SUBLOCATIONS: List = List {
    coded: Coded::Sublocation,
    find: "SELECT p.id FROM sublocations p
           JOIN koha_codes k ON k.kind = 2 AND k.local_id = p.id
           WHERE p.branch_id = ?1 AND k.code = ?2",
    named: "SELECT 1 FROM sublocations WHERE branch_id = ?1 AND name = ?2",
    insert: "INSERT INTO sublocations (branch_id, name) VALUES (?1, ?2)",
};

const COLLECTIONS: List = List {
    coded: Coded::Collection,
    find: "SELECT p.id FROM collections p
           JOIN koha_codes k ON k.kind = 3 AND k.local_id = p.id
           WHERE p.branch_id = ?1 AND k.code = ?2",
    named: "SELECT 1 FROM collections WHERE branch_id = ?1 AND name = ?2",
    insert: "INSERT INTO collections (branch_id, name) VALUES (?1, ?2)",
};

/// Gives every branch its row in the edge table. Called after a branch is created.
pub(crate) fn sync_branches(connection: &Connection) -> Result<(), StoreError> {
    connection.execute(
        "INSERT OR IGNORE INTO koha_codes (kind, local_id, code)
         SELECT ?1, id, code FROM branches",
        params![Coded::Branch as i64],
    )?;
    Ok(())
}

/// Points `item` at the sublocation and the collection of its home branch that stand for
/// the codes it carries, creating them on first use. Called after every Item write.
pub(crate) fn sync_item(connection: &Connection, item: i64) -> Result<(), StoreError> {
    let (branch, location, collection): (i64, Option<i64>, Option<i64>) = connection.query_row(
        "SELECT home_branch, location, collection FROM items WHERE id = ?1",
        params![item],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    let sublocation = location
        .map(|term| place(connection, &SUBLOCATIONS, branch, term))
        .transpose()?;
    let collection = collection
        .map(|term| place(connection, &COLLECTIONS, branch, term))
        .transpose()?;
    connection.execute(
        "UPDATE items SET sublocation_id = ?2, collection_id = ?3 WHERE id = ?1",
        params![item, sublocation, collection],
    )?;
    Ok(())
}

/// Fills the registries for everything already stored (migration 14).
pub(crate) fn backfill(connection: &Connection) -> Result<(), StoreError> {
    sync_branches(connection)?;
    let mut items = connection.prepare("SELECT id FROM items ORDER BY id")?;
    let ids = items
        .query_map([], |row| row.get(0))?
        .collect::<Result<Vec<i64>, _>>()?;
    for id in ids {
        sync_item(connection, id)?;
    }
    Ok(())
}

/// The row of `list` in `branch` for the code of `term`, created with the term's name.
fn place(connection: &Connection, list: &List, branch: i64, term: i64) -> Result<i64, StoreError> {
    let (code, label): (String, String) = connection.query_row(
        "SELECT code, label FROM terms WHERE id = ?1",
        params![term],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let found: Option<i64> = connection
        .query_row(list.find, params![branch, code], |row| row.get(0))
        .optional()?;
    if let Some(id) = found {
        return Ok(id);
    }
    // Two codes may share a name in Koha; names are unique within a branch here.
    let taken = connection
        .query_row(list.named, params![branch, label], |_| Ok(()))
        .optional()?
        .is_some();
    let name = if taken {
        format!("{label} ({code})")
    } else {
        label
    };
    connection.execute(list.insert, params![branch, name])?;
    let id = connection.last_insert_rowid();
    connection.execute(
        "INSERT INTO koha_codes (kind, local_id, code) VALUES (?1, ?2, ?3)",
        params![list.coded as i64, id, code],
    )?;
    Ok(id)
}
