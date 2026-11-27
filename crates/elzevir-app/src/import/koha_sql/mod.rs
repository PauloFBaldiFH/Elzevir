// SPDX-License-Identifier: GPL-3.0-or-later
//! Interpret a bounded Koha dump as catalog data at the MARC edge.

use std::io::BufRead;

use elzevir_marc::Record;

use super::preview::MAX_IMPORT_RECORDS;

mod assemble;
mod database;
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
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
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
    /// A whole-database dump without the patrons table.
    #[error("no Koha patrons in this dump")]
    MissingPatrons,
    /// A dump without the system preferences table.
    #[error("no Koha system preferences in this dump")]
    MissingPreferences,
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
    Ok(with_originals(input)?
        .into_iter()
        .map(|r| (r.record, r.offset))
        .collect())
}

pub use database::{DatabaseCatalog, KohaCatalogItem, KohaCatalogRecord, read_database_catalog};

pub(super) struct MetadataRecord {
    pub declares_utf8: bool,
    pub record: Record,
    pub offset: u64,
    pub xml: Vec<u8>,
}

pub(super) fn with_originals(input: impl BufRead) -> Result<Vec<MetadataRecord>, DumpError> {
    assemble::records(parser::parse(input, CatalogTarget::default())?.rows, None)
}

/// A record of a whole-database dump whose MARCXML the Elzevir cannot read, set aside by
/// the switch with its Items (troca-do-koha.md, item 8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnreadableRecord {
    /// `biblionumber`.
    pub biblionumber: u64,
    /// Why it cannot be read, without its values.
    pub problem: DumpProblem,
    /// The `itemnumber` of each of its Items, in dump order.
    pub itemnumbers: Vec<u64>,
}

/// Most bytes in a whole Koha database dump (docs/specs/migracao/leitores-do-koha.md, item 3).
pub const MAX_DATABASE_DUMP_BYTES: u64 = 8 * 1024 * 1024 * 1024;

/// What a reader keeps from the statements of a dump: which tables and columns, and
/// what a row becomes. The grammar is the same for every target.
pub(crate) trait Target {
    /// A table whose rows are kept.
    type Table: Copy + Ord;
    /// A column whose values are kept.
    type Column: Copy + PartialEq;
    /// Whether `CREATE DATABASE` and `USE` may appear, as in a whole-database dump.
    const WHOLE_DATABASE: bool;
    /// Most bytes read.
    const MAX_BYTES: u64;

    /// The table `name`, lowercase, or `None` when its rows are skipped. `declared` is
    /// true for `CREATE TABLE` and `INSERT`, false for `DROP` and `LOCK`.
    ///
    /// # Errors
    /// A table this target refuses to read.
    fn table(&mut self, name: &str, declared: bool) -> Result<Option<Self::Table>, DumpProblem>;

    /// The kept column `name` of `table`, lowercase.
    fn column(&self, table: Self::Table, name: &str) -> Option<Self::Column>;

    /// Whether an INSERT with these columns carries what a row of `table` needs.
    fn has_keys(&self, table: Self::Table, layout: &[Option<Self::Column>]) -> bool;

    /// Keeps one row, read at byte `offset`.
    ///
    /// # Errors
    /// A row this target refuses.
    fn push(
        &mut self,
        table: Self::Table,
        offset: u64,
        cells: Vec<(Self::Column, Option<Vec<u8>>)>,
    ) -> Result<(), DumpError>;

    /// Checks the whole dump once read; `any` when it had a statement.
    ///
    /// # Errors
    /// What the dump lacks.
    fn finish(&self, any: bool) -> Result<(), DumpProblem>;
}

/// Reads a dump into `target`.
///
/// # Errors
/// Returns a located, value-free grammar, budget or target failure.
pub(crate) fn read<T: Target>(input: impl BufRead, target: T) -> Result<T, DumpError> {
    parser::parse(input, target)
}

/// The catalog import: `biblio_metadata` and `items` of a partial dump.
#[derive(Default)]
struct CatalogTarget {
    inventory: crate::import_detection::sql::Inventory,
    rows: CatalogRows,
}

impl Target for CatalogTarget {
    type Table = Table;
    type Column = Column;
    const WHOLE_DATABASE: bool = false;
    const MAX_BYTES: u64 = super::preview::MAX_IMPORT_FILE_BYTES as u64;

    fn table(&mut self, name: &str, declared: bool) -> Result<Option<Table>, DumpProblem> {
        use crate::import_detection::ImportFormat;
        if declared {
            self.inventory.observe(name.as_bytes());
            if self.inventory.detection().format == ImportFormat::KohaFullBackup {
                return Err(DumpProblem::FullBackup);
            }
        }
        Ok(Some(Table::from_name(name)).filter(|table| table.is_catalog()))
    }

    fn column(&self, table: Table, name: &str) -> Option<Column> {
        Some(table.column(name)).filter(|column| *column != Column::Ignored)
    }

    fn has_keys(&self, table: Table, layout: &[Option<Column>]) -> bool {
        let layout: Vec<Column> = layout
            .iter()
            .map(|column| column.unwrap_or(Column::Ignored))
            .collect();
        table.has_keys(&layout)
    }

    fn push(&mut self, table: Table, offset: u64, cells: Cells) -> Result<(), DumpError> {
        self.rows.push(table, offset, cells)
    }

    fn finish(&self, any: bool) -> Result<(), DumpProblem> {
        use crate::import_detection::ImportFormat;
        if any && self.inventory.detection().format != ImportFormat::KohaSqlDump {
            return Err(DumpProblem::MissingCatalog);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Table {
    Metadata,
    Items,
    /// `itemtypes`, read only from a whole database (troca-do-koha.md, item 7).
    ItemTypes,
    Ignored,
}

impl Table {
    fn from_name(name: &str) -> Self {
        match name {
            "biblio_metadata" => Self::Metadata,
            "items" => Self::Items,
            "itemtypes" => Self::ItemTypes,
            _ => Self::Ignored,
        }
    }

    fn is_catalog(self) -> bool {
        matches!(self, Self::Metadata | Self::Items)
    }

    fn has_keys(self, layout: &[Column]) -> bool {
        if self == Self::ItemTypes {
            return layout.contains(&Column::ItemType);
        }
        layout.contains(&Column::Biblio)
            && layout.contains(&match self {
                Self::Metadata => Column::Metadata,
                Self::Items => Column::Item(b'9'),
                Self::ItemTypes | Self::Ignored => return false,
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
            (Self::ItemTypes, "itemtype") => Column::ItemType,
            (Self::ItemTypes, "description") => Column::Description,
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
    ItemType,
    Description,
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
