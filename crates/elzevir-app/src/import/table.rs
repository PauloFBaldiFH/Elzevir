// SPDX-License-Identifier: GPL-3.0-or-later
//! Bounded CSV and TSV decoding with logical row positions.

use std::io::{self, BufRead, Chain, Cursor, Read, Write};

use elzevir_data::bibframe::{Entity, ontology, properties_of};
use serde::{Deserialize, Serialize};

use super::preview::MAX_IMPORT_FILE_BYTES;

/// Maximum number of logical rows, including the header.
pub const MAX_TABLE_ROWS: u64 = 1_000_000;
/// Maximum number of cells in a row.
pub const MAX_TABLE_COLUMNS: usize = 1_024;
/// Maximum decoded bytes in one cell.
pub const MAX_TABLE_CELL_BYTES: usize = 64 * 1024;
/// Maximum decoded bytes across the cells of one row.
pub const MAX_TABLE_ROW_BYTES: usize = 1024 * 1024;

/// A supported tabular delimiter, selected before reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Delimiter {
    /// Comma-separated values.
    Comma,
    /// CSV with semicolon separator.
    Semicolon,
    /// Tab-separated values.
    Tab,
}

impl Delimiter {
    pub(crate) const fn byte(self) -> u8 {
        match self {
            Self::Comma => b',',
            Self::Semicolon => b';',
            Self::Tab => b'\t',
        }
    }
}

/// A confirmed source encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Encoding {
    /// UTF-8 with an optional initial BOM.
    Utf8,
    /// ISO-8859-1, explicitly selected.
    Latin1,
}

/// One logical row, with unmodified column order and empty cells.
#[derive(Debug, PartialEq, Eq)]
pub struct TableRow {
    /// One-based logical row number including the header.
    pub number: u64,
    /// Zero-based byte offset in the original file.
    pub offset: u64,
    /// Decoded cell values.
    pub cells: Vec<String>,
}

/// A located read or syntax failure, without a copy of the input.
#[derive(Debug, thiserror::Error)]
#[error("row {row}, column {column} (byte {offset}): {kind}")]
pub struct TableError {
    /// One-based logical row number.
    pub row: u64,
    /// One-based column position.
    pub column: usize,
    /// Byte offset of the row start in the source.
    pub offset: u64,
    /// Reason for terminating the read.
    #[source]
    pub kind: ErrorKind,
}

/// Why a table could not be read.
#[derive(Debug, thiserror::Error)]
pub enum ErrorKind {
    /// A named limit was exceeded, including the first extra cell or byte.
    #[error("table limit exceeded: {0}")]
    Limit(&'static str),
    /// The selected UTF-8 encoding could not decode the cell.
    #[error("invalid UTF-8")]
    InvalidUtf8,
    /// A quote appeared outside the start of a field.
    #[error("unexpected quote or characters after a closing quote")]
    InvalidQuote,
    /// The file ended during a quoted field.
    #[error("unclosed quoted field")]
    UnclosedQuote,
    /// The underlying stream failed.
    #[error(transparent)]
    Io(#[from] io::Error),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Start,
    Text,
    Quoted,
    AfterQuote,
}

/// A streaming reader that terminates after its first error.
pub struct TableReader<R> {
    input: Chain<Cursor<Vec<u8>>, R>,
    delimiter: Delimiter,
    encoding: Encoding,
    offset: u64,
    read: u64,
    done: bool,
}

impl<R: BufRead> TableReader<R> {
    /// Begins reading with the selected delimiter and encoding.
    ///
    /// # Errors
    /// Returns an I/O failure while checking for the UTF-8 BOM.
    pub fn new(mut input: R, delimiter: Delimiter, encoding: Encoding) -> Result<Self, TableError> {
        let mut prefix = Vec::new();
        let mut offset = 0;
        if encoding == Encoding::Utf8 {
            input
                .by_ref()
                .take(3)
                .read_to_end(&mut prefix)
                .map_err(|error| TableError {
                    row: 1,
                    column: 1,
                    offset: 0,
                    kind: ErrorKind::Io(error),
                })?;
            if prefix == b"\xef\xbb\xbf" {
                offset = 3;
                prefix.clear();
            }
        }
        Ok(Self {
            input: Cursor::new(prefix).chain(input),
            delimiter,
            encoding,
            offset,
            read: 0,
            done: false,
        })
    }

    fn peek(&mut self) -> Result<Option<u8>, ErrorKind> {
        let next = self.input.fill_buf()?.first().copied();
        if next.is_some() && self.offset >= MAX_IMPORT_FILE_BYTES as u64 {
            return Err(ErrorKind::Limit("input bytes"));
        }
        Ok(next)
    }

    fn byte(&mut self) -> Result<Option<u8>, ErrorKind> {
        let next = self.peek()?;
        if next.is_some() {
            self.input.consume(1);
            self.offset += 1;
        }
        Ok(next)
    }

    fn finish_line(&mut self, byte: u8) -> Result<(), ErrorKind> {
        if byte == b'\r' && self.peek()? == Some(b'\n') {
            self.byte()?;
        }
        Ok(())
    }

    fn next_row(&mut self) -> Result<Option<TableRow>, TableError> {
        let number = self.read + 1;
        let located = |kind, offset, column| TableError {
            row: number,
            column,
            offset,
            kind,
        };
        let first = loop {
            let start = self.offset;
            match self.byte().map_err(|kind| located(kind, start, 1))? {
                Some(end @ (b'\r' | b'\n')) => {
                    self.finish_line(end)
                        .map_err(|kind| located(kind, start, 1))?;
                }
                Some(byte) => break (byte, start),
                None => return Ok(None),
            }
        };
        let (byte, offset) = first;
        if number > MAX_TABLE_ROWS {
            return Err(located(ErrorKind::Limit("rows"), offset, 1));
        }
        let mut row = RowBuilder::default();
        self.read_cells(byte, &mut row)
            .map_err(|kind| located(kind, offset, row.cells.len() + 1))?;
        self.read = number;
        Ok(Some(TableRow {
            number,
            offset,
            cells: row.cells,
        }))
    }

    fn read_cells(&mut self, first: u8, row: &mut RowBuilder) -> Result<(), ErrorKind> {
        let mut state = Field::Start;
        let mut next = Some(first);
        while let Some(byte) = next {
            state = match (state, byte) {
                (Field::Start, b'"') => Field::Quoted,
                (Field::Quoted, b'"') => Field::AfterQuote,
                (Field::AfterQuote, b'"') => {
                    row.push(b'"', self.encoding)?;
                    Field::Quoted
                }
                (Field::Quoted, value) => {
                    row.push(value, self.encoding)?;
                    Field::Quoted
                }
                (_, delimiter) if delimiter == self.delimiter.byte() => {
                    row.finish_cell()?;
                    if row.cells.len() == MAX_TABLE_COLUMNS {
                        return Err(ErrorKind::Limit("columns"));
                    }
                    Field::Start
                }
                (_, end @ (b'\r' | b'\n')) => {
                    self.finish_line(end)?;
                    return row.finish_cell();
                }
                (Field::AfterQuote, _) | (Field::Text, b'"') => {
                    return Err(ErrorKind::InvalidQuote);
                }
                (_, value) => {
                    row.push(value, self.encoding)?;
                    Field::Text
                }
            };
            next = self.byte()?;
        }
        if state == Field::Quoted {
            return Err(ErrorKind::UnclosedQuote);
        }
        row.finish_cell()
    }
}

impl<R: BufRead> Iterator for TableReader<R> {
    type Item = Result<TableRow, TableError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let result = self.next_row();
        self.done = !matches!(result, Ok(Some(_)));
        result.transpose()
    }
}

#[derive(Default)]
struct RowBuilder {
    cells: Vec<String>,
    cell: Vec<u8>,
    bytes: usize,
}

impl RowBuilder {
    fn push(&mut self, byte: u8, encoding: Encoding) -> Result<(), ErrorKind> {
        let mut decoded = [0; 2];
        let literal = [byte];
        let bytes = if encoding == Encoding::Latin1 {
            char::from(byte).encode_utf8(&mut decoded).as_bytes()
        } else {
            &literal
        };
        if self.cell.len() + bytes.len() > MAX_TABLE_CELL_BYTES {
            return Err(ErrorKind::Limit("cell bytes"));
        }
        if self.bytes + bytes.len() > MAX_TABLE_ROW_BYTES {
            return Err(ErrorKind::Limit("row bytes"));
        }
        self.cell.extend_from_slice(bytes);
        self.bytes += bytes.len();
        Ok(())
    }

    fn finish_cell(&mut self) -> Result<(), ErrorKind> {
        let input = std::mem::take(&mut self.cell);
        self.cells
            .push(String::from_utf8(input).map_err(|_| ErrorKind::InvalidUtf8)?);
        Ok(())
    }
}

/// Native item registry and inventory columns included in the generated model.
pub const ITEM_CATALOG_COLUMNS: &[&str] = &[
    "barcode",
    "inventory_number",
    "home_branch",
    "holding_branch",
    "policy",
    "call_number",
    "location",
    "collection",
    "copy_number",
    "price",
];

/// Returns the exact UTF-8 model columns for the pinned BIBFRAME ontology.
#[must_use]
pub fn model_columns() -> Vec<String> {
    let pinned = ontology();
    let mut columns = vec![
        "elzevir-model:1".to_owned(),
        format!("bibframe:{}:{}", pinned.version, pinned.source_sha256),
    ];
    for entity in Entity::ALL {
        let name = entity.class_name();
        columns.push(format!("{name}.@type"));
        columns.extend(properties_of(entity).map(|property| format!("{name}.{}", property.name)));
    }
    columns.extend(
        ITEM_CATALOG_COLUMNS
            .iter()
            .map(|field| format!("Item.catalog.{field}")),
    );
    columns
}

/// Writes the versioned, entity-qualified model header as CSV or TSV.
///
/// # Errors
/// Returns an error from the output stream.
pub fn write_model_header(mut output: impl Write, delimiter: Delimiter) -> io::Result<()> {
    let primary = char::from(delimiter.byte()).to_string();
    writeln!(output, "{}", model_columns().join(&primary))
}
