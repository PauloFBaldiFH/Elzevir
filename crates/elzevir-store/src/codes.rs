// SPDX-License-Identifier: GPL-3.0-or-later
//! `catalog.name_codes`: names that Koha gives its codes, written to the registries
//! through the edge table (docs/specs/catalogo/tabelas-de-codigos.md, items 2 to 5, and
//! bibframe-completo.md, item 23b).

use elzevir_app::catalog::{CatalogOutput, CatalogRefusal};
use elzevir_app::codes::{CodeTable, NamedCode, NamingCount, checked};
use elzevir_app::commands::WriteRefusal;
use rusqlite::{Connection, OptionalExtension, params};

use crate::catalog::refused;
use crate::places::{self, Coded};
use crate::{CommandFailed, StoreError, WriteTx};

/// `catalog.name_codes` (items 2 to 4).
pub(crate) fn name_codes(
    tx: &WriteTx<'_>,
    codes: &[NamedCode],
) -> Result<CatalogOutput, CommandFailed<WriteRefusal>> {
    let codes =
        checked(codes).map_err(|position| refused(CatalogRefusal::BadCodeName(position)))?;
    let connection = tx.connection();
    let mut counts = [NamingCount::default(); 4];
    for entry in &codes {
        let count = CodeTable::ALL
            .iter()
            .position(|table| *table == entry.table)
            .and_then(|index| counts.get_mut(index));
        let Some(count) = count else { continue };
        let change = match entry.table {
            CodeTable::Branch => branch(connection, entry)?,
            CodeTable::ItemType => item_type(connection, entry)?,
            CodeTable::Location => places(connection, "sublocations", Coded::Sublocation, entry)?,
            CodeTable::Collection => places(connection, "collections", Coded::Collection, entry)?,
        };
        match change {
            Change::Created => count.created += 1,
            Change::Renamed => count.renamed += 1,
            Change::Unchanged => count.unchanged += 1,
            Change::Unused => count.unused += 1,
        }
    }
    Ok(CatalogOutput::CodesNamed(counts))
}

enum Change {
    Created,
    Renamed,
    Unchanged,
    /// A shelving location or collection code no branch uses yet: its rows are created
    /// per branch by the import, with this name once it is known (item 23b).
    Unused,
}

/// A branch is found by its edge code; a new code creates the branch.
fn branch(connection: &Connection, entry: &NamedCode) -> Result<Change, StoreError> {
    let Some(id) = places::by_code(connection, Coded::Branch, &entry.code)? else {
        connection.execute(
            "INSERT INTO branches (name) VALUES (?1)",
            params![entry.name],
        )?;
        let id = connection.last_insert_rowid();
        places::set_code(connection, Coded::Branch, id, &entry.code)?;
        return Ok(Change::Created);
    };
    let current: String = connection.query_row(
        "SELECT name FROM branches WHERE id = ?1",
        params![id],
        |row| row.get(0),
    )?;
    if current == entry.name {
        return Ok(Change::Unchanged);
    }
    connection.execute(
        "UPDATE branches SET name = ?2, row_version = row_version + 1 WHERE id = ?1",
        params![id, entry.name],
    )?;
    Ok(Change::Renamed)
}

/// Item types keep their code until stage 4b.
fn item_type(connection: &Connection, entry: &NamedCode) -> Result<Change, StoreError> {
    let current: Option<String> = connection
        .query_row(
            "SELECT description FROM item_types WHERE code = ?1",
            params![entry.code],
            |row| row.get(0),
        )
        .optional()?;
    Ok(match current {
        None => {
            connection.execute(
                "INSERT INTO item_types (code, description) VALUES (?1, ?2)",
                params![entry.code, entry.name],
            )?;
            Change::Created
        }
        Some(current) if current == entry.name => Change::Unchanged,
        Some(_) => {
            connection.execute(
                "UPDATE item_types SET description = ?2, row_version = row_version + 1
                 WHERE code = ?1",
                params![entry.code, entry.name],
            )?;
            Change::Renamed
        }
    })
}

/// Every row of `table` with the code takes the name; Koha's lists are not per branch.
/// Where another row of the branch has the name, the row takes "name (code)", as the
/// import does (item 23b).
fn places(
    connection: &Connection,
    table: &str,
    coded: Coded,
    entry: &NamedCode,
) -> Result<Change, StoreError> {
    // `table` is one of two constants, never data.
    let rows: Vec<(i64, i64, String)> = connection
        .prepare(&format!(
            "SELECT p.id, p.branch_id, p.name FROM {table} p
             JOIN koha_codes k ON k.kind = ?1 AND k.local_id = p.id WHERE k.code = ?2"
        ))?
        .query_map(params![coded as i64, entry.code], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?
        .collect::<Result<_, _>>()?;
    if rows.is_empty() {
        return Ok(Change::Unused);
    }
    let taken = format!("SELECT 1 FROM {table} WHERE branch_id = ?1 AND name = ?2 AND id <> ?3");
    let mut renamed = false;
    for (id, branch, current) in rows {
        let free = connection
            .query_row(&taken, params![branch, entry.name, id], |_| Ok(()))
            .optional()?
            .is_none();
        let name = if free {
            entry.name.clone()
        } else {
            format!("{} ({})", entry.name, entry.code)
        };
        if name == current {
            continue;
        }
        connection.execute(
            &format!("UPDATE {table} SET name = ?2, row_version = row_version + 1 WHERE id = ?1"),
            params![id, name],
        )?;
        renamed = true;
    }
    Ok(if renamed {
        Change::Renamed
    } else {
        Change::Unchanged
    })
}
