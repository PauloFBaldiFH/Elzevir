// SPDX-License-Identifier: GPL-3.0-or-later
//! Guard workbook XML and translate saved cells to the existing tabular input.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Display;
use std::io::Cursor;
use std::path::Path;

use calamine::{Data, DataRef, Ods, Reader, Xlsx};
use quick_xml::events::{BytesStart, Event};

use super::{
    Error, Kind, MAX_ARCHIVE_EXPANDED_BYTES, MAX_ARCHIVE_NAME_BYTES, MAX_WORKBOOK_CELLS,
    MAX_WORKBOOK_SHEETS, Selection,
};
use crate::import::table::{
    MAX_TABLE_CELL_BYTES, MAX_TABLE_COLUMNS, MAX_TABLE_ROW_BYTES, MAX_TABLE_ROWS, TableRow,
};

pub(super) fn read(
    kind: Kind,
    bytes: &[u8],
    choice: Option<&Selection>,
) -> Result<(Vec<String>, Option<Vec<TableRow>>), Error> {
    let cursor = Cursor::new(bytes);
    match kind {
        Kind::Xlsx => {
            let mut book = Xlsx::new(cursor).map_err(|_| Error::Workbook)?;
            let names = book.sheet_names();
            let rows = sheet(&names, choice)?
                .map(|name| xlsx(&mut book, name))
                .transpose()?;
            Ok((names, rows))
        }
        Kind::Ods => {
            let mut book = Ods::new(cursor).map_err(|_| Error::Workbook)?;
            let names = book.sheet_names();
            let rows = sheet(&names, choice)?
                .map(|name| ods(&mut book, name))
                .transpose()?;
            Ok((names, rows))
        }
        Kind::File(_) => Err(Error::Workbook),
    }
}

fn sheet<'a>(names: &[String], choice: Option<&'a Selection>) -> Result<Option<&'a str>, Error> {
    if names.len() > MAX_WORKBOOK_SHEETS {
        return Err(Error::Limit("sheet count"));
    }
    let mut seen = BTreeSet::new();
    if names.iter().any(|name| {
        name.is_empty()
            || name.len() > MAX_ARCHIVE_NAME_BYTES
            || name.chars().any(char::is_control)
            || !seen.insert(name)
    }) {
        return Err(Error::Workbook);
    }
    let Some(choice) = choice else {
        return Ok(None);
    };
    let name = choice
        .sheet
        .as_deref()
        .filter(|name| names.iter().any(|value| value == name))
        .ok_or(Error::Selection)?;
    if choice.format.is_some() {
        return Err(Error::Selection);
    }
    Ok(Some(name))
}

fn xlsx(book: &mut Xlsx<Cursor<&[u8]>>, name: &str) -> Result<Vec<TableRow>, Error> {
    let mut source = book
        .worksheet_cells_reader(name)
        .map_err(|_| Error::Workbook)?;
    let mut rows = Rows::default();
    let mut examined = 0;
    while let Some(cell) = source.next_cell().map_err(|_| Error::Workbook)? {
        examined += 1;
        if examined > MAX_WORKBOOK_CELLS {
            return Err(Error::Limit("worksheet cells"));
        }
        let (row, column) = cell.get_position();
        let at = (u64::from(row) + 1, column as usize + 1);
        match cell.get_value() {
            DataRef::Empty => {}
            DataRef::Error(_) => return Err(cell_error(at)),
            DataRef::SharedString(text) => rows.cell(at, text)?,
            DataRef::String(text) => rows.cell(at, text)?,
            value => rows.cell(at, &Data::from(value.clone()))?,
        }
    }
    rows.finish()
}

fn ods(book: &mut Ods<Cursor<&[u8]>>, name: &str) -> Result<Vec<TableRow>, Error> {
    let range = book.worksheet_range(name).map_err(|_| Error::Workbook)?;
    let mut rows = Rows::default();
    let Some((top, left)) = range.start() else {
        return Ok(Vec::new());
    };
    for (row, values) in range.rows().enumerate() {
        for (column, value) in values.iter().enumerate() {
            let at = (u64::from(top) + row as u64 + 1, left as usize + column + 1);
            match value {
                Data::Empty => {}
                Data::Error(_) => return Err(cell_error(at)),
                value => rows.cell(at, value)?,
            }
        }
    }
    rows.finish()
}

fn cell_error((row, column): (u64, usize)) -> Error {
    Error::Cell { row, column }
}

#[derive(Default)]
struct Rows {
    source: BTreeMap<u64, SavedRow>,
    width: usize,
    bytes: u64,
}

#[derive(Default)]
struct SavedRow {
    cells: BTreeMap<usize, String>,
    bytes: usize,
}

impl Rows {
    fn cell(&mut self, (row, column): (u64, usize), value: &impl Display) -> Result<(), Error> {
        if row > MAX_TABLE_ROWS || column > MAX_TABLE_COLUMNS {
            return Err(Error::Limit("worksheet dimensions"));
        }
        let value = value.to_string();
        if value.is_empty() {
            return Ok(());
        }
        if value.len() > MAX_TABLE_CELL_BYTES {
            return Err(Error::Limit("cell bytes"));
        }
        let saved = self.source.entry(row).or_default();
        saved.bytes += value.len();
        self.bytes += value.len() as u64;
        if saved.bytes > MAX_TABLE_ROW_BYTES || self.bytes > MAX_ARCHIVE_EXPANDED_BYTES {
            return Err(Error::Limit("worksheet value bytes"));
        }
        if saved.cells.insert(column, value).is_some() {
            return Err(Error::Workbook);
        }
        self.width = self.width.max(column);
        Ok(())
    }

    fn finish(mut self) -> Result<Vec<TableRow>, Error> {
        let Some((&first, _)) = self.source.first_key_value() else {
            return Ok(Vec::new());
        };
        let (&last, _) = self.source.last_key_value().ok_or(Error::Workbook)?;
        let height =
            usize::try_from(last - first + 1).map_err(|_| Error::Limit("worksheet dimensions"))?;
        grid(height, self.width)?;
        (first..=last)
            .map(|number| {
                let mut cells = vec![String::new(); self.width];
                if let Some(row) = self.source.remove(&number) {
                    for (column, value) in row.cells {
                        *cells.get_mut(column - 1).ok_or(Error::Workbook)? = value;
                    }
                }
                Ok(TableRow {
                    number,
                    offset: 0,
                    cells,
                })
            })
            .collect()
    }
}

fn grid(rows: usize, columns: usize) -> Result<usize, Error> {
    let count = rows.saturating_mul(columns);
    if rows as u64 > MAX_TABLE_ROWS || columns > MAX_TABLE_COLUMNS || count > MAX_WORKBOOK_CELLS {
        return Err(Error::Limit("worksheet dimensions"));
    }
    Ok(count)
}

#[derive(Default)]
pub(super) struct Bounds {
    ods: OdsBounds,
    cells: usize,
    shared_strings: usize,
    sheets: usize,
}

impl Bounds {
    pub(super) fn inspect(&mut self, kind: Kind, name: &str, input: &[u8]) -> Result<(), Error> {
        if kind == Kind::Ods && name == "mimetype" {
            return (input == b"application/vnd.oasis.opendocument.spreadsheet")
                .then_some(())
                .ok_or(Error::Workbook);
        }
        let xml = input
            .strip_prefix(b"\xef\xbb\xbf")
            .unwrap_or(input)
            .trim_ascii_start();
        let xml_part = Path::new(name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("xml") || ext.eq_ignore_ascii_case("rels"));
        if !xml_part && !xml.starts_with(b"<") {
            return Ok(());
        }
        std::str::from_utf8(input).map_err(|_| Error::Workbook)?;
        let mut reader = quick_xml::Reader::from_reader(input);
        reader.config_mut().expand_empty_elements = true;
        let mut depth = 0;
        let mut expected_xlsx_type = false;
        let mut value_span = None;
        let ods = kind == Kind::Ods && name == "content.xml";
        let value_budget = elzevir_marc::XmlLimits::default().max_element_bytes as u64;
        loop {
            let start = reader.buffer_position();
            match reader.read_event().map_err(|_| Error::Workbook)? {
                Event::Start(element) => {
                    depth += 1;
                    if depth > elzevir_marc::XmlLimits::default().max_depth {
                        return Err(Error::Limit("workbook XML depth"));
                    }
                    if element.len() as u64 > value_budget {
                        return Err(Error::Limit("workbook XML element bytes"));
                    }
                    if kind == Kind::Xlsx {
                        self.xlsx_element(&element)?;
                        if value_span.is_none()
                            && matches!(element.local_name().as_ref(), b"c" | b"si")
                        {
                            value_span = Some((depth, start));
                        }
                        if name == "[Content_Types].xml"
                            && element.local_name().as_ref() == b"Override"
                            && attr(&element, b"PartName")?.as_deref() == Some(b"/xl/workbook.xml")
                            && attr(&element, b"ContentType")?.as_deref() == Some(b"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml")
                        {
                            expected_xlsx_type = true;
                        }
                    } else if ods {
                        self.ods.open(&element, start)?;
                    }
                }
                Event::End(element) => {
                    if let Some((opened, offset)) = value_span
                        && depth == opened
                    {
                        if reader.buffer_position() - offset > value_budget {
                            return Err(Error::Limit("workbook XML element bytes"));
                        }
                        value_span = None;
                    }
                    depth = depth.checked_sub(1).ok_or(Error::Workbook)?;
                    if ods {
                        self.ods
                            .close(element.name().as_ref(), reader.buffer_position())?;
                    }
                }
                Event::Decl(decl) => {
                    if decl
                        .encoding()
                        .transpose()
                        .map_err(|_| Error::Workbook)?
                        .is_some_and(|encoding| !encoding.eq_ignore_ascii_case(b"utf-8"))
                    {
                        return Err(Error::Workbook);
                    }
                }
                Event::Text(text) if text.len() as u64 > value_budget => {
                    return Err(Error::Limit("workbook XML text bytes"));
                }
                Event::CData(text) if text.len() as u64 > value_budget => {
                    return Err(Error::Limit("workbook XML text bytes"));
                }
                Event::Eof if depth == 0 => break,
                Event::DocType(_) | Event::Eof => return Err(Error::Workbook),
                _ => {}
            }
        }
        if kind == Kind::Xlsx && name == "[Content_Types].xml" && !expected_xlsx_type {
            return Err(Error::Workbook);
        }
        Ok(())
    }

    fn xlsx_element(&mut self, element: &BytesStart<'_>) -> Result<(), Error> {
        match element.local_name().as_ref() {
            b"c" => {
                self.cells += 1;
                if self.cells > MAX_WORKBOOK_CELLS {
                    return Err(Error::Limit("worksheet cells"));
                }
                if let Some(value) = attr(element, b"r")? {
                    coordinate(&value)?;
                }
            }
            b"dimension" => {
                if let Some(value) = attr(element, b"ref")? {
                    for part in value.split(|byte| *byte == b':') {
                        coordinate(part)?;
                    }
                }
            }
            b"row" => {
                if let Some(value) = attr(element, b"r")?
                    && !number(&value).is_some_and(|row| (1..=MAX_TABLE_ROWS).contains(&row))
                {
                    return Err(Error::Limit("worksheet dimensions"));
                }
            }
            b"sheet" => {
                self.sheets += 1;
                if self.sheets > MAX_WORKBOOK_SHEETS {
                    return Err(Error::Limit("sheet count"));
                }
                checked_sheet_name(element, b"name")?;
            }
            b"sst" => {
                // The workbook reader reserves space from the declared unique count.
                if let Some(value) = attr(element, b"uniqueCount")?
                    && number(&value).is_none_or(|count| count > MAX_WORKBOOK_CELLS as u64)
                {
                    return Err(Error::Limit("workbook shared strings"));
                }
            }
            b"si" => {
                self.shared_strings += 1;
                if self.shared_strings > MAX_WORKBOOK_CELLS {
                    return Err(Error::Limit("workbook shared strings"));
                }
            }
            _ => {}
        }
        Ok(())
    }
}

fn coordinate(value: &[u8]) -> Result<(), Error> {
    let split = value
        .iter()
        .position(u8::is_ascii_digit)
        .ok_or(Error::Workbook)?;
    let (col, row) = value.split_at(split);
    let mut column = 0usize;
    for letter in col {
        if !letter.is_ascii_alphabetic() {
            return Err(Error::Workbook);
        }
        column = column
            .saturating_mul(26)
            .saturating_add(usize::from(letter.to_ascii_uppercase() - b'A' + 1));
    }
    if column == 0
        || column > MAX_TABLE_COLUMNS
        || !number(row).is_some_and(|row| (1..=MAX_TABLE_ROWS).contains(&row))
    {
        return Err(Error::Limit("worksheet dimensions"));
    }
    Ok(())
}

fn attr<'a>(element: &'a BytesStart<'_>, name: &[u8]) -> Result<Option<Cow<'a, [u8]>>, Error> {
    for attr in element.attributes() {
        let attr = attr.map_err(|_| Error::Workbook)?;
        if attr.key.as_ref() == name {
            return Ok(Some(attr.value));
        }
    }
    Ok(None)
}

fn checked_sheet_name(element: &BytesStart<'_>, name: &[u8]) -> Result<(), Error> {
    if let Some(name) = attr(element, name)?
        && name.len() > 6 * MAX_ARCHIVE_NAME_BYTES
    {
        return Err(Error::Limit("sheet name bytes"));
    }
    Ok(())
}

fn number(value: &[u8]) -> Option<u64> {
    std::str::from_utf8(value).ok()?.parse().ok()
}

#[derive(Default)]
struct OdsBounds {
    grid: Option<(usize, usize)>,
    row: Option<OdsRow>,
    cell: Option<OdsCell>,
    sheets: usize,
    cells: usize,
    bytes: u64,
}

struct OdsRow {
    repeat: usize,
    columns: usize,
    bytes: u64,
}

struct OdsCell {
    repeat: usize,
    start: u64,
    spaces: u64,
}

impl OdsBounds {
    fn open(&mut self, element: &BytesStart<'_>, start: u64) -> Result<(), Error> {
        match element.name().as_ref() {
            b"table:table" if self.grid.is_none() => {
                checked_sheet_name(element, b"table:name")?;
                self.sheets += 1;
                if self.sheets > MAX_WORKBOOK_SHEETS {
                    return Err(Error::Limit("sheet count"));
                }
                self.grid = Some((0, 0));
            }
            b"table:table-row" if self.grid.is_some() && self.row.is_none() => {
                self.row = Some(OdsRow {
                    repeat: repetitions(element, b"table:number-rows-repeated", MAX_TABLE_ROWS)?,
                    columns: 0,
                    bytes: 0,
                });
            }
            b"table:table-cell" | b"table:covered-table-cell"
                if self.row.is_some() && self.cell.is_none() =>
            {
                self.cell = Some(OdsCell {
                    repeat: repetitions(
                        element,
                        b"table:number-columns-repeated",
                        MAX_TABLE_COLUMNS as u64,
                    )?,
                    start,
                    spaces: 0,
                });
            }
            b"table:table"
            | b"table:table-row"
            | b"table:table-cell"
            | b"table:covered-table-cell" => {
                return Err(Error::Workbook);
            }
            b"text:s" => {
                if let Some(cell) = &mut self.cell {
                    cell.spaces +=
                        repetitions(element, b"text:c", MAX_TABLE_CELL_BYTES as u64)? as u64;
                    if cell.spaces > MAX_TABLE_CELL_BYTES as u64 {
                        return Err(Error::Limit("cell bytes"));
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn close(&mut self, name: &[u8], end: u64) -> Result<(), Error> {
        match name {
            b"table:table-cell" | b"table:covered-table-cell" => {
                let cell = self.cell.take().ok_or(Error::Workbook)?;
                let row = self.row.as_mut().ok_or(Error::Workbook)?;
                let bytes = end - cell.start + cell.spaces;
                if bytes > (4 * MAX_TABLE_CELL_BYTES) as u64 {
                    return Err(Error::Limit("cell bytes"));
                }
                row.columns += cell.repeat;
                row.bytes += bytes * cell.repeat as u64;
                if row.columns > MAX_TABLE_COLUMNS || row.bytes > MAX_TABLE_ROW_BYTES as u64 {
                    return Err(Error::Limit("worksheet row"));
                }
            }
            b"table:table-row" => {
                let row = self.row.take().ok_or(Error::Workbook)?;
                let (height, width) = self.grid.as_mut().ok_or(Error::Workbook)?;
                *height += row.repeat;
                *width = (*width).max(row.columns);
                grid(*height, *width)?;
                self.bytes += row.bytes * row.repeat as u64;
                if self.bytes > MAX_ARCHIVE_EXPANDED_BYTES {
                    return Err(Error::Limit("worksheet value bytes"));
                }
            }
            b"table:table" => {
                let (height, width) = self.grid.take().ok_or(Error::Workbook)?;
                self.cells += grid(height, width)?;
                if self.cells > MAX_WORKBOOK_CELLS {
                    return Err(Error::Limit("worksheet cells"));
                }
            }
            _ => {}
        }
        Ok(())
    }
}

fn repetitions(element: &BytesStart<'_>, key: &[u8], max: u64) -> Result<usize, Error> {
    let Some(value) = attr(element, key)? else {
        return Ok(1);
    };
    number(&value)
        .filter(|value| (1..=max).contains(value))
        .and_then(|value| usize::try_from(value).ok())
        .ok_or(Error::Limit("worksheet repetition"))
}
