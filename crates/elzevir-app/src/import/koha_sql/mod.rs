// SPDX-License-Identifier: GPL-3.0-or-later
//! Interpret a bounded Koha dump as catalog data at the MARC edge.

use std::io::BufRead;

use elzevir_marc::Record;

use super::preview::MAX_IMPORT_RECORDS;

mod assemble;
mod lexer;
mod parser;

/// Maximum source bytes in one SQL statement, including discarded values.
pub const MAX_SQL_STATEMENT_BYTES: u64 = 16 * 1024 * 1024;
/// Maximum width of one table declaration or INSERT row.
pub const MAX_SQL_COLUMNS: usize = 1_024;
/// Maximum bytes in a recognized SQL identifier.
pub const MAX_SQL_IDENTIFIER_BYTES: usize = 64;
/// Maximum decoded bytes in one retained catalog cell.
pub const MAX_SQL_VALUE_BYTES: usize = 1024 * 1024;
/// Maximum rows retained from the items table.
pub const MAX_SQL_ITEMS: usize = 1_000_000;
/// Maximum parentheses nested inside a table definition.
pub const MAX_SQL_DDL_DEPTH: usize = 32;

/// Located SQL preparation failure, without raw values.
#[derive(Debug, thiserror::Error)]
#[error("Koha SQL at byte {offset}: {problem}")]
pub struct DumpError {
    /// Zero-based byte in the original SQL stream.
    pub offset: u64,
    /// The validation failure.
    pub problem: DumpProblem,
}

/// Why preparation stopped before submitting records.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum DumpProblem {
    /// Input could not be read.
    #[error("cannot read dump input")]
    Read,
    /// A named read/write budget was exceeded.
    #[error("limit exceeded ({0})")]
    Limit(&'static str),
    /// The input did not match the fixed grammar.
    #[error("{0}")]
    Syntax(&'static str),
    /// A command outside the supported subset was found.
    #[error("unsupported dump statement")]
    Unsupported,
    /// Catalog, circulation and configuration declarations were observed.
    #[error("complete Koha backup; use a database restoration workflow")]
    FullBackup,
    /// The stream has statements but declares no supported catalog tables.
    #[error("no Koha catalog tables in this dump")]
    MissingCatalog,
    /// An INSERT failed to identify all required columns.
    #[error("declare the required Koha catalog columns before inserting rows")]
    MissingColumns,
    /// Catalog identifiers must be positive decimal integers.
    #[error("required positive catalog identifier ({0})")]
    Identifier(&'static str),
    /// A key, column or table declaration was repeated.
    #[error("duplicate {0}")]
    Duplicate(&'static str),
    /// One metadata value did not yield exactly one MARC21 bibliographic record.
    #[error("expected one UTF-8 MARC21 XML record in biblio_metadata.metadata")]
    Metadata,
    /// An item points at a biblio not supplied by the dump.
    #[error("orphan item; no biblio_metadata row for its biblionumber")]
    Orphan,
    /// The embedded identifiers or values disagree with the inventory.
    #[error("conflicting inventory and MARC data ({0})")]
    Conflict(&'static str),
}

/// Validates the complete dump and joins its rows to bibliographic MARC records.
/// The associated byte offset points to the metadata tuple.
///
/// # Errors
/// Returns a located, value-free grammar, budget, inventory or MARCXML failure.
pub fn prepare(input: impl BufRead) -> Result<Vec<(Record, u64)>, DumpError> {
    assemble::records(parser::parse(input)?)
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Table {
    Metadata,
    Items,
    Ignored,
}

impl Table {
    fn from_name(name: &str) -> Self {
        match name {
            "biblio_metadata" => Self::Metadata,
            "items" => Self::Items,
            _ => Self::Ignored,
        }
    }

    fn is_catalog(self) -> bool {
        self != Self::Ignored
    }

    fn has_keys(self, layout: &[Column]) -> bool {
        layout.contains(&Column::Biblio)
            && layout.contains(&match self {
                Self::Metadata => Column::Metadata,
                Self::Items => Column::Item(b'9'),
                Self::Ignored => return false,
            })
    }

    fn column(self, name: &str) -> Column {
        match (self, name) {
            (Self::Metadata | Self::Items, "biblionumber") => Column::Biblio,
            (Self::Metadata, "id") => Column::MetadataId,
            (Self::Metadata, "metadata") => Column::Metadata,
            (Self::Metadata, "format") => Column::Format,
            (Self::Metadata, "schema") => Column::Schema,
            (Self::Items, name) => item_column(name),
            _ => Column::Ignored,
        }
    }
}

// Koha: Koha::Item::as_marc_field. Only the catalog attributes in the
// MARC mapping spec, plus the source identifier, are projected from SQL.
fn item_column(name: &str) -> Column {
    elzevir_data::marc21_field("952")
        .and_then(|field| {
            field.subfields.iter().find(|subfield| {
                matches!(
                    subfield.code,
                    '0' | '1'
                        | '4'
                        | '5'
                        | '7'
                        | '8'
                        | '9'
                        | 'a'
                        | 'b'
                        | 'c'
                        | 'd'
                        | 'g'
                        | 'i'
                        | 'o'
                        | 'p'
                        | 't'
                        | 'v'
                        | 'y'
                ) && subfield
                    .kohafield
                    .and_then(|value| value.strip_prefix("items."))
                    == Some(name)
            })
        })
        .and_then(|subfield| u8::try_from(subfield.code).ok())
        .map_or(Column::Ignored, Column::Item)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Column {
    Biblio,
    MetadataId,
    Metadata,
    Format,
    Schema,
    Item(u8),
    Ignored,
}

type Cells = Vec<(Column, Option<Vec<u8>>)>;

#[derive(Default)]
struct CatalogRows {
    biblios: Vec<Biblio>,
    items: Vec<Item>,
}

struct Biblio {
    offset: u64,
    number: u64,
    primary_key: Option<u64>,
    xml: Vec<u8>,
}

struct Item {
    offset: u64,
    number: u64,
    biblio: u64,
    values: Vec<(u8, Option<Vec<u8>>)>,
}

impl CatalogRows {
    fn push(&mut self, table: Table, offset: u64, mut cells: Cells) -> Result<(), DumpError> {
        let error = |problem| DumpError { offset, problem };
        let mut take = |column| {
            cells
                .iter_mut()
                .find(|(name, _)| *name == column)
                .and_then(|(_, value)| value.take())
        };
        let biblio = take(Column::Biblio)
            .as_deref()
            .and_then(positive)
            .ok_or_else(|| error(DumpProblem::Identifier("biblionumber")))?;
        if table == Table::Metadata {
            if self.biblios.len() as u64 == MAX_IMPORT_RECORDS {
                return Err(error(DumpProblem::Limit("record count")));
            }
            let xml = take(Column::Metadata).ok_or_else(|| error(DumpProblem::Metadata))?;
            let mut primary_key = None;
            for (column, value) in cells {
                match column {
                    Column::MetadataId => {
                        primary_key =
                            Some(value.as_deref().and_then(positive).ok_or_else(|| {
                                error(DumpProblem::Identifier("biblio_metadata.id"))
                            })?);
                    }
                    Column::Format
                        if !value
                            .as_ref()
                            .is_some_and(|value| value.eq_ignore_ascii_case(b"marcxml")) =>
                    {
                        return Err(error(DumpProblem::Metadata));
                    }
                    Column::Schema
                        if !value
                            .as_ref()
                            .is_some_and(|value| value.eq_ignore_ascii_case(b"marc21")) =>
                    {
                        return Err(error(DumpProblem::Metadata));
                    }
                    _ => {}
                }
            }
            self.biblios.push(Biblio {
                offset,
                number: biblio,
                primary_key,
                xml,
            });
        } else {
            if self.items.len() == MAX_SQL_ITEMS {
                return Err(error(DumpProblem::Limit("item count")));
            }
            let number = take(Column::Item(b'9'))
                .as_deref()
                .and_then(positive)
                .ok_or_else(|| error(DumpProblem::Identifier("itemnumber")))?;
            let mut values = Vec::new();
            for (column, value) in cells {
                if let Column::Item(code) = column
                    && code != b'9'
                {
                    if value.as_ref().is_some_and(|value| {
                        std::str::from_utf8(value).is_err()
                            || value.iter().any(|byte| matches!(byte, 0 | 0x1d..=0x1f))
                    }) {
                        return Err(error(DumpProblem::Syntax("invalid UTF-8 MARC item value")));
                    }
                    values.push((code, value.filter(|value| !value.is_empty())));
                }
            }
            self.items.push(Item {
                offset,
                number,
                biblio,
                values,
            });
        }
        Ok(())
    }
}

fn positive(bytes: &[u8]) -> Option<u64> {
    std::str::from_utf8(bytes)
        .ok()?
        .trim()
        .parse()
        .ok()
        .filter(|number| *number > 0)
}
