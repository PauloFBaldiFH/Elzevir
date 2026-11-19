// SPDX-License-Identifier: GPL-3.0-or-later
//! `catalog.change_saved_list`: lists of families the team saves
//! (docs/specs/catalogo/lote.md, items 15 to 21).

use elzevir_app::catalog::{CatalogOutput, CatalogRefusal};
use elzevir_app::commands::WriteRefusal;
use elzevir_app::listing::FamilyHead;
use elzevir_app::worklist::{
    MAX_LIST_FAMILIES, SavedListChange, SavedListId, SavedListRefusal, checked,
};
use rusqlite::{Connection, OptionalExtension, params};

use crate::catalog::refused;
use crate::{CommandFailed, StoreError, WriteTx};

fn refusal(refusal: SavedListRefusal) -> CommandFailed<WriteRefusal> {
    refused(CatalogRefusal::SavedList(refusal))
}

/// `catalog.change_saved_list`.
pub(crate) fn change_saved_list(
    tx: &WriteTx<'_>,
    change: &SavedListChange,
) -> Result<CatalogOutput, CommandFailed<WriteRefusal>> {
    let connection = tx.connection();
    let change = checked(change.clone()).map_err(refusal)?;
    let list = match change {
        SavedListChange::Create { name, heads } => {
            if name_taken(connection, &name)? {
                return Err(refusal(SavedListRefusal::NameTaken));
            }
            connection
                .execute("INSERT INTO saved_lists (name) VALUES (?1)", params![name])
                .map_err(StoreError::from)?;
            let list = SavedListId(connection.last_insert_rowid());
            add(connection, list, &heads)?;
            list
        }
        SavedListChange::Add { list, heads } => {
            known(connection, list)?;
            add(connection, list, &heads)?;
            list
        }
        SavedListChange::Remove { list, heads } => {
            known(connection, list)?;
            for head in heads {
                connection
                    .prepare_cached(
                        "DELETE FROM saved_list_families WHERE list_id = ?1 AND head = ?2",
                    )
                    .and_then(|mut statement| statement.execute(params![list.0, head.name()]))
                    .map_err(StoreError::from)?;
            }
            list
        }
        SavedListChange::Delete { list } => {
            known(connection, list)?;
            connection
                .execute(
                    "DELETE FROM saved_list_families WHERE list_id = ?1",
                    params![list.0],
                )
                .and_then(|_| {
                    connection.execute("DELETE FROM saved_lists WHERE id = ?1", params![list.0])
                })
                .map_err(StoreError::from)?;
            return Ok(CatalogOutput::SavedListChanged(list));
        }
    };
    connection
        .execute(
            "UPDATE saved_lists SET row_version = row_version + 1 WHERE id = ?1",
            params![list.0],
        )
        .map_err(StoreError::from)?;
    Ok(CatalogOutput::SavedListChanged(list))
}

/// Whether another list has `name`, case aside in any script (lote.md, item 17).
fn name_taken(connection: &Connection, name: &str) -> Result<bool, StoreError> {
    let wanted = name.to_lowercase();
    let mut statement = connection.prepare_cached("SELECT name FROM saved_lists")?;
    let names = statement.query_map([], |row| row.get::<_, String>(0))?;
    for stored in names {
        if stored?.to_lowercase() == wanted {
            return Ok(true);
        }
    }
    Ok(false)
}

fn known(connection: &Connection, list: SavedListId) -> Result<(), CommandFailed<WriteRefusal>> {
    let found = connection
        .query_row(
            "SELECT 1 FROM saved_lists WHERE id = ?1",
            params![list.0],
            |_| Ok(()),
        )
        .optional()
        .map_err(StoreError::from)?;
    found.ok_or_else(|| refusal(SavedListRefusal::Unknown))
}

/// `heads` added after what `list` holds, each once, refused whole past the limit.
fn add(
    connection: &Connection,
    list: SavedListId,
    heads: &[FamilyHead],
) -> Result<(), CommandFailed<WriteRefusal>> {
    let held = |head: &FamilyHead| -> Result<bool, StoreError> {
        Ok(connection
            .prepare_cached("SELECT 1 FROM saved_list_families WHERE list_id = ?1 AND head = ?2")?
            .exists(params![list.0, head.name()])?)
    };
    let mut new: Vec<FamilyHead> = Vec::new();
    for head in heads {
        if !new.contains(head) && !held(head)? {
            new.push(*head);
        }
    }
    let (held_count, last_added): (i64, i64) = connection
        .query_row(
            "SELECT count(*), coalesce(max(added), 0) FROM saved_list_families WHERE list_id = ?1",
            params![list.0],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(StoreError::from)?;
    let held_count = usize::try_from(held_count).unwrap_or(usize::MAX);
    if held_count.saturating_add(new.len()) > MAX_LIST_FAMILIES {
        return Err(refusal(SavedListRefusal::TooManyFamilies));
    }
    for (added, head) in (last_added.saturating_add(1)..).zip(new) {
        connection
            .prepare_cached(
                "INSERT INTO saved_list_families (list_id, head, added) VALUES (?1, ?2, ?3)",
            )
            .and_then(|mut statement| statement.execute(params![list.0, head.name(), added]))
            .map_err(StoreError::from)?;
    }
    Ok(())
}
