// SPDX-License-Identifier: GPL-3.0-or-later
//! ZIP, gzip and spreadsheet packages at the prepared import boundary.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read};

use serde::{Deserialize, Serialize};

use super::preview::MAX_IMPORT_FILE_BYTES;
use super::table::TableRow;
use crate::import_detection::{
    Detection, Evidence, ImportFormat, MAX_DETECTION_BYTES, detect_format,
};

mod book;
mod zip;

/// Entry count across all inspected packages, including directories.
pub const MAX_ARCHIVE_MEMBERS: usize = 1_000;
/// Expanded bytes summed across both container levels.
pub const MAX_ARCHIVE_EXPANDED_BYTES: u64 = 512 * 1024 * 1024;
/// Expansion ratio budget per member and for the complete source.
pub const MAX_ARCHIVE_RATIO: u64 = 100;
/// Maximum count of containers on any path, including spreadsheet packages.
pub const MAX_ARCHIVE_DEPTH: usize = 2;
/// Maximum UTF-8 bytes of a member name or sheet name.
pub const MAX_ARCHIVE_NAME_BYTES: usize = 256;
/// Maximum cell count in a workbook and rectangular area of the selected sheet.
pub const MAX_WORKBOOK_CELLS: usize = 1_000_000;
/// Maximum sheets in a workbook.
pub const MAX_WORKBOOK_SHEETS: usize = 256;

/// An exact path through the reviewed inventory and, when applicable, a sheet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    /// One filename per enclosing archive, or empty for a workbook at the root.
    #[serde(default)]
    pub member: Vec<String>,
    /// Exact sheet name for XLSX and ODS.
    pub sheet: Option<String>,
    /// Optional reader confirmation for a file with inconclusive evidence.
    pub format: Option<ImportFormat>,
}

/// Selectable files and worksheets in source order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Inventory {
    /// Leaves of the container, each with its own detection.
    pub entries: Vec<Entry>,
}

/// A selectable file or workbook.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Exact member path accepted in the recipe.
    pub member: Vec<String>,
    /// Format of the member or workbook.
    pub kind: Kind,
    /// Names of the selectable sheets, in file order.
    pub sheets: Vec<String>,
}

/// Type found by member inspection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    /// A regular member with the usual import detection.
    File(ImportFormat),
    /// An Office Open XML spreadsheet.
    Xlsx,
    /// An `OpenDocument` spreadsheet.
    Ods,
}

/// Data from the selection after validating the entire enclosing inventory.
pub enum Selected {
    /// Bounded source bytes and content evidence.
    File(Vec<u8>, Detection),
    /// Cells from the selected workbook sheet.
    Workbook(Vec<TableRow>),
}

/// Archive and workbook refusals contain no raw values from members.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// An input limit was exceeded.
    #[error("archive limit exceeded ({0})")]
    Limit(&'static str),
    /// The input fails the supported ZIP or gzip structure.
    #[error("invalid or unsupported archive")]
    Archive,
    /// A member name is unsafe or aliases another member.
    #[error("unsafe or duplicate archive member name")]
    Name,
    /// A member is a special file, encrypted, or unsupported.
    #[error("archive member is not a regular supported file")]
    Member,
    /// The workbook package is invalid.
    #[error("invalid or unsupported workbook")]
    Workbook,
    /// The selection is missing or inconsistent with this inventory.
    #[error("choose an existing archive member and, for a workbook, its sheet")]
    Selection,
    /// A workbook cell contains a calculation or type error.
    #[error("worksheet row {row}, column {column}: invalid saved cell value")]
    Cell {
        /// One-based sheet row.
        row: u64,
        /// One-based sheet column.
        column: usize,
    },
}

/// Validates the archive, including every member, and returns the exact choices.
///
/// # Errors
/// Reports invalid names, package structure, expansions and workbook limits.
pub fn inspect(input: &[u8]) -> Result<Inventory, Error> {
    let walker = walk(input, None, &[])?;
    Ok(Inventory {
        entries: walker.entries,
    })
}

/// Reads one explicit selection while validating the same complete inventory.
///
/// # Errors
/// Reports invalid archive data or an incompatible selection.
pub fn select(input: &[u8], selection: &Selection) -> Result<Selected, Error> {
    walk(input, Some(selection), &[])?
        .selected
        .ok_or(Error::Selection)
}

/// Copies complete originals at the requested inventory paths in submission order.
/// The entire archive passes the same validation used for single-member preview.
///
/// # Errors
/// Rejects empty, repeated or missing paths and invalid containers.
pub fn selected_files(input: &[u8], paths: &[Vec<String>]) -> Result<Vec<Vec<u8>>, Error> {
    if paths.is_empty()
        || paths.len() > super::batches::MAX_SOURCE_MEMBERS
        || paths.iter().any(Vec::is_empty)
        || paths.iter().collect::<BTreeSet<_>>().len() != paths.len()
    {
        return Err(Error::Selection);
    }
    let mut files = walk(input, None, paths)?.files;
    paths
        .iter()
        .map(|path| files.remove(path).ok_or(Error::Selection))
        .collect()
}

fn walk<'a>(
    input: &[u8],
    choice: Option<&'a Selection>,
    paths: &'a [Vec<String>],
) -> Result<Walker<'a>, Error> {
    if input.len() > MAX_IMPORT_FILE_BYTES {
        return Err(Error::Limit("input bytes"));
    }
    if !is_container(detect_format(input).evidence) {
        return Err(Error::Archive);
    }
    let mut walker = Walker {
        budget: Budget {
            members: 0,
            remaining: (input.len() as u64 * MAX_ARCHIVE_RATIO).min(MAX_ARCHIVE_EXPANDED_BYTES),
        },
        entries: Vec::new(),
        choice,
        selected: None,
        paths,
        files: BTreeMap::new(),
    };
    walker.visit(input, &mut Vec::new(), 1)?;
    Ok(walker)
}

const fn is_container(evidence: Evidence) -> bool {
    matches!(evidence, Evidence::ZipSignature | Evidence::GzipSignature)
}

struct Budget {
    members: usize,
    remaining: u64,
}

impl Budget {
    fn members(&mut self, count: usize) -> Result<(), Error> {
        self.members += count;
        if self.members > MAX_ARCHIVE_MEMBERS {
            return Err(Error::Limit("member count"));
        }
        Ok(())
    }

    fn expand(&mut self, bytes: u64) -> Result<(), Error> {
        self.remaining = self
            .remaining
            .checked_sub(bytes)
            .ok_or(Error::Limit("total expansion"))?;
        Ok(())
    }
}

struct Walker<'a> {
    budget: Budget,
    entries: Vec<Entry>,
    choice: Option<&'a Selection>,
    selected: Option<Selected>,
    paths: &'a [Vec<String>],
    files: BTreeMap<Vec<String>, Vec<u8>>,
}

impl Walker<'_> {
    fn visit(&mut self, bytes: &[u8], path: &mut Vec<String>, depth: usize) -> Result<(), Error> {
        let detection = detect_format(bytes);
        if is_container(detection.evidence) && depth > MAX_ARCHIVE_DEPTH {
            return Err(Error::Limit("container depth"));
        }
        match detection.evidence {
            Evidence::ZipSignature => self.zip(bytes, path, depth),
            Evidence::GzipSignature => self.gzip(bytes, path, depth),
            _ => self.file(bytes, path, detection),
        }
    }

    fn capture_path(&mut self, path: &[String], bytes: &[u8]) {
        if self.paths.iter().any(|selected| selected == path) {
            self.files.insert(path.to_vec(), bytes.to_vec());
        }
    }

    fn file(&mut self, bytes: &[u8], path: &[String], detection: Detection) -> Result<(), Error> {
        if let Some(choice) = self.choice.filter(|choice| choice.member == path) {
            if choice.sheet.is_some() {
                return Err(Error::Selection);
            }
            self.selected = Some(Selected::File(bytes.to_vec(), detection));
        }
        self.capture_path(path, bytes);
        self.entries.push(Entry {
            member: path.to_vec(),
            kind: Kind::File(detection.format),
            sheets: Vec::new(),
        });
        Ok(())
    }

    fn gzip(
        &mut self,
        mut input: &[u8],
        path: &mut Vec<String>,
        depth: usize,
    ) -> Result<(), Error> {
        let mut names = BTreeSet::new();
        let mut index = 0;
        while !input.is_empty() {
            index += 1;
            self.budget.members(1)?;
            let (name, data, consumed) = gzip_member(input, index, &mut self.budget)?;
            if !names.insert(name.to_ascii_lowercase()) {
                return Err(Error::Name);
            }
            input = input
                .get(consumed..)
                .filter(|_| consumed > 0)
                .ok_or(Error::Archive)?;
            path.push(name);
            self.visit(&data, path, depth + 1)?;
            path.pop();
        }
        Ok(())
    }
}

fn gzip_member(
    input: &[u8],
    index: usize,
    budget: &mut Budget,
) -> Result<(String, Vec<u8>, usize), Error> {
    check_gzip_header(input)?;
    let mut decoder = flate2::bufread::GzDecoder::new(Cursor::new(input));
    let header = decoder.header().ok_or(Error::Archive)?;
    let name = header.filename().map_or_else(
        || Ok(format!("member-{index}")),
        |bytes| checked_name(bytes, false),
    )?;
    let mut data = Vec::new();
    let mut chunk = [0; 8192];
    loop {
        let len = decoder.read(&mut chunk).map_err(|_| Error::Archive)?;
        if len == 0 {
            break;
        }
        ratio(data.len() as u64 + len as u64, decoder.get_ref().position())?;
        budget.expand(len as u64)?;
        data.try_reserve(len)
            .map_err(|_| Error::Limit("expanded bytes"))?;
        data.extend_from_slice(chunk.get(..len).ok_or(Error::Archive)?);
    }
    let consumed = usize::try_from(decoder.into_inner().position()).map_err(|_| Error::Archive)?;
    ratio(data.len() as u64, consumed as u64)?;
    Ok((name, data, consumed))
}

fn ratio(expanded: u64, compressed: u64) -> Result<(), Error> {
    if expanded > compressed.saturating_mul(MAX_ARCHIVE_RATIO) {
        return Err(Error::Limit("compression ratio"));
    }
    Ok(())
}

fn checked_name(raw: &[u8], directory: bool) -> Result<String, Error> {
    let name = std::str::from_utf8(raw).map_err(|_| Error::Name)?;
    let name = if directory {
        name.strip_suffix('/').unwrap_or(name)
    } else {
        name
    };
    if name.len() > MAX_ARCHIVE_NAME_BYTES
        || name
            .chars()
            .any(|ch| ch.is_control() || matches!(ch, '\\' | ':'))
        || name
            .split('/')
            .any(|component| matches!(component, "" | "." | ".."))
    {
        return Err(Error::Name);
    }
    Ok(name.to_owned())
}

fn check_gzip_header(input: &[u8]) -> Result<(), Error> {
    if !input.starts_with(b"\x1f\x8b\x08") {
        return Err(Error::Archive);
    }
    let flags = *input
        .get(3)
        .filter(|flags| **flags & 0xe0 == 0)
        .ok_or(Error::Archive)?;
    let mut tail = input.get(10..).ok_or(Error::Archive)?;
    if flags & 4 != 0 {
        let len = u16::from_le_bytes(
            tail.get(..2)
                .and_then(|bytes| bytes.try_into().ok())
                .ok_or(Error::Archive)?,
        );
        tail = tail.get(2 + usize::from(len)..).ok_or(Error::Archive)?;
    }
    for (flag, limit) in [(8, MAX_ARCHIVE_NAME_BYTES), (16, MAX_DETECTION_BYTES)] {
        if flags & flag != 0 {
            let end = tail
                .iter()
                .take(limit + 1)
                .position(|byte| *byte == 0)
                .ok_or(Error::Limit("gzip header bytes"))?;
            tail = tail.get(end + 1..).ok_or(Error::Archive)?;
        }
    }
    Ok(())
}
