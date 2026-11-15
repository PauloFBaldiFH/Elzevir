// SPDX-License-Identifier: GPL-3.0-or-later
//! `catalog.name_codes`: names for the code tables, creating the missing codes
//! (docs/specs/catalogo/tabelas-de-codigos.md, items 2 to 5).

use elzevir_app::catalog::{CatalogOutput, CatalogRefusal};
use elzevir_app::codes::{CodeTable, NamedCode, NamingCount, checked};
use elzevir_app::commands::WriteRefusal;
use elzevir_domain::catalog::Vocabulary;
use rusqlite::{Connection, OptionalExtension, params};

use crate::catalog::refused;
use crate::{CommandFailed, StoreError, WriteTx};

/// How one table is read and written: `name` takes the code (and the vocabulary as `?2`
/// for terms), `insert` and `rename` the code, the name (and the vocabulary as `?3`).
struct Statements {
    name: &'static str,
    insert: &'static str,
    rename: &'static str,
    vocabulary: Option<u8>,
}

const fn statements(table: CodeTable) -> Statements {
    const TERM_NAME: &str = "SELECT label FROM terms WHERE code = ?1 AND vocabulary = ?2";
    const TERM_INSERT: &str = "INSERT INTO terms (vocabulary, code, label) VALUES (?3, ?1, ?2)";
    const TERM_RENAME: &str = "UPDATE terms SET label = ?2 WHERE code = ?1 AND vocabulary = ?3";
    match table {
        CodeTable::Branch => Statements {
            name: "SELECT name FROM branches WHERE code = ?1",
            insert: "INSERT INTO branches (code, name) VALUES (?1, ?2)",
            rename: "UPDATE branches SET name = ?2, row_version = row_version + 1 WHERE code = ?1",
            vocabulary: None,
        },
        CodeTable::ItemType => Statements {
            name: "SELECT description FROM item_types WHERE code = ?1",
            insert: "INSERT INTO item_types (code, description) VALUES (?1, ?2)",
            rename: "UPDATE item_types SET description = ?2, row_version = row_version + 1
                     WHERE code = ?1",
            vocabulary: None,
        },
        CodeTable::Location => Statements {
            name: TERM_NAME,
            insert: TERM_INSERT,
            rename: TERM_RENAME,
            vocabulary: Some(Vocabulary::Location.code()),
        },
        CodeTable::Collection => Statements {
            name: TERM_NAME,
            insert: TERM_INSERT,
            rename: TERM_RENAME,
            vocabulary: Some(Vocabulary::Collection.code()),
        },
    }
}

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
        match name(connection, entry)? {
            Change::Created => count.created += 1,
            Change::Renamed => count.renamed += 1,
            Change::Unchanged => count.unchanged += 1,
        }
    }
    crate::places::sync_branches(connection)?;
    Ok(CatalogOutput::CodesNamed(counts))
}

enum Change {
    Created,
    Renamed,
    Unchanged,
}

fn name(connection: &Connection, entry: &NamedCode) -> Result<Change, StoreError> {
    let sql = statements(entry.table);
    let current: Option<String> = match sql.vocabulary {
        None => connection.query_row(sql.name, params![entry.code], |row| row.get(0)),
        Some(vocabulary) => {
            connection.query_row(sql.name, params![entry.code, vocabulary], |row| row.get(0))
        }
    }
    .optional()?;
    let write = |statement: &str| match sql.vocabulary {
        None => connection.execute(statement, params![entry.code, entry.name]),
        Some(vocabulary) => {
            connection.execute(statement, params![entry.code, entry.name, vocabulary])
        }
    };
    Ok(match current {
        None => {
            write(sql.insert)?;
            Change::Created
        }
        Some(current) if current == entry.name => Change::Unchanged,
        Some(_) => {
            write(sql.rename)?;
            Change::Renamed
        }
    })
}
