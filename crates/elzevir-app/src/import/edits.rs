// SPDX-License-Identifier: GPL-3.0-or-later
//! Structured MARC edits applied before preparation and mapping.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::io::{BufRead, Write};

use elzevir_marc::{Field, Limits, Record, RecordRef, Subfield, Tag};
use serde::{Deserialize, Serialize};

use super::preview::{
    IssueKind, MAX_IMPORT_FILE_BYTES, MAX_IMPORT_RECORDS, MAX_PREVIEW_RECORDS, PreviewError,
    PreviewReport, batch_match_reason, scan_as,
};
use super::sheet::{PreparedTable, TableSettings};
use super::{ImportError, RawPlace, RecordFailure};
use crate::import_detection::{Detection, ImportFormat};
use crate::matching::Matching;

/// Maximum edits in one batch.
pub const MAX_IMPORT_EDITS: usize = 10_000;
const MAX_FIELD_DATA: usize = 9_998;
const DIRECTORY_ENTRY_BYTES: usize = 12;
const RECORD_OVERHEAD: usize = 26;

/// Edits, record exclusions and matching selected for this revision.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Recipe {
    /// Stable source installation for local identifiers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    /// Explicit access policy for local notes in this source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_note_visibility: Option<crate::catalog::NoteVisibility>,
    /// Operations in application order.
    pub edits: Vec<Edit>,
    /// One-based source record numbers explicitly excluded.
    pub exclude: BTreeSet<u64>,
    /// One-based records whose items can be stored without barcodes.
    pub accept_missing_barcodes: BTreeSet<u64>,
    /// Existing import matching options.
    pub matching: Option<Matching>,
    /// Settings of a native CSV or TSV revision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table: Option<TableSettings>,
    /// Exact member and optional worksheet selected from a container inventory.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archive: Option<super::containers::Selection>,
}

/// A change to one tag, on one record or the entire batch.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edit {
    /// One-based source record number. Omit to apply throughout the batch.
    pub record: Option<u64>,
    /// Three-digit MARC field tag.
    pub tag: String,
    /// Operation affecting fields with this tag.
    pub change: Change,
}

/// Operations are interpreted against the owned MARC record.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Change {
    /// Append a field.
    AddField {
        /// New content for the tagged field.
        field: FieldValue,
    },
    /// Remove all fields with the tag.
    RemoveField,
    /// Replace each field with the tag in place.
    ReplaceField {
        /// Replacement content.
        field: FieldValue,
    },
    /// Append a subfield to each data field with the tag.
    AddSubfield {
        /// ASCII subfield code.
        code: char,
        /// Literal text.
        value: String,
    },
    /// Remove subfields with the given code from each selected field.
    RemoveSubfield {
        /// ASCII subfield code.
        code: char,
    },
    /// Replace each occurrence of a subfield value.
    ReplaceSubfield {
        /// ASCII subfield code.
        code: char,
        /// Replacement text.
        value: String,
    },
    /// Replace literal text in the values of selected fields.
    ReplaceText {
        /// Nonempty sequence to find.
        from: String,
        /// Literal replacement.
        to: String,
    },
}

/// Complete content of a control or data field.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FieldValue {
    /// Control field.
    Control {
        /// Literal text.
        value: String,
    },
    /// Data field.
    Data {
        /// Two ASCII indicator characters.
        indicators: [char; 2],
        /// Ordered subfields, with repetitions allowed.
        subfields: Vec<TextSubfield>,
    },
}

/// A code and value for a new data field.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextSubfield {
    /// ASCII subfield code.
    pub code: char,
    /// Literal text.
    pub value: String,
}

fn tag(value: &str) -> Option<Tag> {
    let bytes: [u8; 3] = value.as_bytes().try_into().ok()?;
    (bytes != *b"000" && bytes.iter().all(u8::is_ascii_digit)).then(|| Tag::new(bytes))
}

fn text(value: &str) -> bool {
    value.len() <= MAX_FIELD_DATA && !value.bytes().any(|b| (0x1d..=0x1f).contains(&b))
}

fn field_size(field: &Field) -> usize {
    match field {
        Field::Control { value, .. } => value.len(),
        Field::Data { subfields, .. } => {
            2 + subfields
                .iter()
                .map(|sub| 2 + sub.value.len())
                .sum::<usize>()
        }
    }
}

impl FieldValue {
    fn build(&self, tag: Tag) -> Option<Field> {
        let field = match self {
            Self::Control { value } if tag.is_control() && text(value) => Field::Control {
                tag,
                value: value.as_bytes().to_vec(),
            },
            Self::Data {
                indicators,
                subfields,
            } if !tag.is_control()
                && indicators
                    .iter()
                    .all(|ch| ch.is_ascii_alphanumeric() || *ch == ' ')
                && subfields
                    .iter()
                    .all(|sub| sub.code.is_ascii_alphanumeric() && text(&sub.value))
                && subfields
                    .iter()
                    .try_fold(2_usize, |size, sub| {
                        size.checked_add(2 + sub.value.len())
                            .filter(|size| *size <= MAX_FIELD_DATA)
                    })
                    .is_some() =>
            {
                Field::Data {
                    tag,
                    indicators: indicators.map(|ch| ch as u8),
                    subfields: subfields
                        .iter()
                        .map(|sub| Subfield {
                            code: sub.code as u8,
                            value: sub.value.as_bytes().to_vec(),
                        })
                        .collect(),
                }
            }
            _ => return None,
        };
        Some(field)
    }
}

impl Recipe {
    /// Reads the revision's recipe, including versioned TOML spreadsheet mappings.
    ///
    /// # Errors
    /// Reports a malformed or invalid recipe.
    pub fn parse(input: &str) -> Result<Self, PreviewError> {
        let recipe: Self = if input.trim_start().starts_with('{') {
            serde_json::from_str(input).map_err(|error| {
                PreviewError::Edit(format!(
                    "invalid recipe JSON at line {}, column {}",
                    error.line(),
                    error.column()
                ))
            })?
        } else {
            toml::from_str(input).map_err(|error| {
                PreviewError::Edit(format!("invalid recipe TOML: {}", error.message()))
            })?
        };
        recipe.validate()?;
        Ok(recipe)
    }

    /// Validates the syntax and limits of the stored recipe.
    ///
    /// # Errors
    /// Returns a located error for invalid selectors, fields or operations.
    pub fn validate(&self) -> Result<(), PreviewError> {
        if self
            .origin
            .as_deref()
            .is_some_and(|s| s.trim().is_empty() || s.trim() != s || s.len() > 256)
        {
            return Err(PreviewError::Edit(
                "invalid source installation identifier".into(),
            ));
        }
        if self.table.is_some() && !self.edits.is_empty() {
            return Err(PreviewError::Edit(
                "MARC edit rules do not apply to a spreadsheet".into(),
            ));
        }
        if self.edits.len() > MAX_IMPORT_EDITS {
            return Err(PreviewError::Limit("edit count"));
        }
        let limit = if self.table.is_some() || self.archive.is_some() {
            super::table::MAX_TABLE_ROWS
        } else {
            MAX_IMPORT_RECORDS
        };
        let in_range = |index| (1..=limit).contains(&index);
        if !self
            .exclude
            .iter()
            .chain(&self.accept_missing_barcodes)
            .copied()
            .all(in_range)
        {
            return Err(PreviewError::Edit("invalid selection position".into()));
        }
        for (index, edit) in self.edits.iter().enumerate() {
            if !edit.record.is_none_or(in_range) || !edit.valid() {
                return Err(PreviewError::Edit(format!("invalid edit {}", index + 1)));
            }
        }
        Ok(())
    }
}

impl Edit {
    fn valid(&self) -> bool {
        let Some(tag) = tag(&self.tag) else {
            return false;
        };
        match &self.change {
            Change::AddField { field } | Change::ReplaceField { field } => {
                field.build(tag).is_some()
            }
            Change::RemoveField => true,
            Change::AddSubfield { code, value } | Change::ReplaceSubfield { code, value } => {
                !tag.is_control() && code.is_ascii_alphanumeric() && text(value)
            }
            Change::RemoveSubfield { code } => !tag.is_control() && code.is_ascii_alphanumeric(),
            Change::ReplaceText { from, to } => !from.is_empty() && text(from) && text(to),
        }
    }

    fn apply(
        &self,
        record: &mut Record,
        size: &mut usize,
        limits: Limits,
    ) -> Result<(), &'static str> {
        let tag = tag(&self.tag).ok_or("invalid tag")?;
        match &self.change {
            Change::AddField { field } => {
                let field = field.build(tag).ok_or("invalid field")?;
                if record.fields.len() >= limits.max_fields
                    || *size + field_size(&field) + DIRECTORY_ENTRY_BYTES + 1
                        > limits.max_record_len
                {
                    return Err("record size");
                }
                *size += field_size(&field) + DIRECTORY_ENTRY_BYTES + 1;
                record.fields.push(field);
            }
            Change::RemoveField => {
                record.fields.retain(|field| {
                    if field.tag() == tag {
                        *size -= field_size(field) + DIRECTORY_ENTRY_BYTES + 1;
                        false
                    } else {
                        true
                    }
                });
            }
            change => {
                for field in record.fields.iter_mut().filter(|field| field.tag() == tag) {
                    let before = field_size(field);
                    let limit =
                        MAX_FIELD_DATA.min(limits.max_record_len.saturating_sub(*size - before));
                    change.apply_to(field, limit)?;
                    *size = *size - before + field_size(field);
                }
            }
        }
        Ok(())
    }
}

impl Change {
    fn apply_to(&self, field: &mut Field, limit: usize) -> Result<(), &'static str> {
        if let Self::ReplaceField { field: replacement } = self {
            let replacement = replacement.build(field.tag()).ok_or("invalid field")?;
            if field_size(&replacement) > limit {
                return Err("record size");
            }
            *field = replacement;
            return Ok(());
        }
        let mut size = field_size(field);
        if let Self::ReplaceText { from, to } = self {
            let mut replace = |value: &mut Vec<u8>| {
                let input = std::str::from_utf8(value).map_err(|_| "invalid UTF-8 field")?;
                let count = input.matches(from).count();
                let new_size = value.len() - count * from.len() + count.saturating_mul(to.len());
                resize(&mut size, value.len(), new_size, limit)?;
                if count != 0 {
                    *value = input.replace(from, to).into_bytes();
                }
                Ok(())
            };
            return match field {
                Field::Control { value, .. } => replace(value),
                Field::Data { subfields, .. } => subfields
                    .iter_mut()
                    .try_for_each(|sub| replace(&mut sub.value)),
            };
        }
        let Field::Data { subfields, .. } = field else {
            return Err("subfield requires a data field");
        };
        match self {
            Self::AddSubfield { code, value } => {
                resize(&mut size, 0, 2 + value.len(), limit)?;
                subfields.push(Subfield {
                    code: *code as u8,
                    value: value.as_bytes().to_vec(),
                });
            }
            Self::RemoveSubfield { code } => subfields.retain(|sub| sub.code != *code as u8),
            Self::ReplaceSubfield { code, value } => {
                for sub in subfields.iter_mut().filter(|sub| sub.code == *code as u8) {
                    resize(&mut size, sub.value.len(), value.len(), limit)?;
                    sub.value = value.as_bytes().to_vec();
                }
            }
            _ => return Err("invalid field operation"),
        }
        Ok(())
    }
}

fn resize(size: &mut usize, old: usize, new: usize, limit: usize) -> Result<(), &'static str> {
    let next = (*size - old).saturating_add(new);
    if next > limit {
        return Err("field or record size");
    }
    *size = next;
    Ok(())
}

fn protected(record: &Record) -> impl Iterator<Item = (Tag, &Subfield)> {
    record
        .fields
        .iter()
        .filter_map(|field| match field {
            Field::Data { tag, subfields, .. } => Some((*tag, subfields)),
            Field::Control { .. } => None,
        })
        .flat_map(|(tag, subfields)| {
            subfields
                .iter()
                .filter(move |sub| {
                    matches!(
                        (tag.as_bytes(), sub.code),
                        (b"999", b'c' | b'd') | (b"952", b'9')
                    )
                })
                .map(move |sub| (tag, sub))
        })
}

/// One changed record shown in the preview.
pub struct ChangedSample {
    /// One-based position in the original source.
    pub record: u64,
    /// First changed field groups, in tag order.
    pub fields: Vec<FieldChange>,
}

/// Before and after values for the occurrences of one tag.
pub struct FieldChange {
    /// Field tag.
    pub tag: Tag,
    /// Original field values, in order.
    pub before: Vec<String>,
    /// Edited field values, in order.
    pub after: Vec<String>,
}

fn field_changes(original: &Record, edited: &Record) -> Vec<FieldChange> {
    let tags: BTreeSet<_> = original
        .fields
        .iter()
        .chain(&edited.fields)
        .map(Field::tag)
        .collect();
    tags.into_iter()
        .filter_map(|tag| {
            let before: Vec<_> = original
                .fields
                .iter()
                .filter(|field| field.tag() == tag)
                .collect();
            let after: Vec<_> = edited
                .fields
                .iter()
                .filter(|field| field.tag() == tag)
                .collect();
            (before != after).then(|| FieldChange {
                tag,
                before: before.into_iter().map(field_text).collect(),
                after: after.into_iter().map(field_text).collect(),
            })
        })
        .take(MAX_PREVIEW_RECORDS)
        .collect()
}

fn field_text(field: &Field) -> String {
    match field {
        Field::Control { value, .. } => String::from_utf8_lossy(value).into_owned(),
        Field::Data {
            indicators,
            subfields,
            ..
        } => {
            let mut text = String::from_utf8_lossy(indicators).into_owned();
            for subfield in subfields {
                text.push('$');
                text.push(char::from(subfield.code));
                text.push_str(&String::from_utf8_lossy(&subfield.value));
            }
            text
        }
    }
}

/// Effect of rendering the recipe without writing to the catalog.
pub struct RenderReport {
    /// Format of the original.
    pub detection: Detection,
    /// Records read in the original.
    pub read: u64,
    /// Records excluded by the recipe.
    pub excluded: u64,
    /// Records whose MARC contents were changed.
    pub changed: u64,
    /// First few changes.
    pub samples: Vec<ChangedSample>,
    /// The selected page of source records.
    pub source: Vec<SourceSample>,
    /// Decoding evidence for originals that were edited by this recipe, by rendered index.
    pub originals: std::collections::BTreeMap<u64, crate::catalog::OriginalRecord>,
    positions: Vec<RawPlace>,
}

/// Source and edited MARC rendered as plain text for the review page.
pub struct SourceSample {
    /// One-based position in the uploaded file.
    pub record: u64,
    /// Original MARC presentation.
    pub before: String,
    /// Edited MARC presentation when changed.
    pub after: Option<String>,
}

impl RenderReport {
    /// Positions and one page of review metadata from a prepared spreadsheet.
    #[must_use]
    pub fn for_table(table: &PreparedTable, from: Option<u64>) -> Self {
        let positions = table
            .records()
            .iter()
            .map(|row| RawPlace {
                index: row.row - 1,
                offset: row.offset,
            })
            .collect();
        let start = from
            .and_then(|start| usize::try_from(start).ok())
            .unwrap_or_default();
        let source = table
            .records()
            .iter()
            .skip(start)
            .take(MAX_PREVIEW_RECORDS)
            .map(|row| SourceSample {
                record: row.row,
                before: String::new(),
                after: None,
            })
            .collect();
        Self {
            detection: Detection {
                format: ImportFormat::Delimited,
                evidence: crate::import_detection::Evidence::DelimitedRows,
            },
            read: table.read,
            excluded: table.excluded,
            changed: 0,
            originals: std::collections::BTreeMap::new(),
            samples: Vec::new(),
            source,
            positions,
        }
    }

    /// Finds a record by its original position for a diagnostic link.
    #[must_use]
    pub fn selected_index(&self, index: u64) -> Option<usize> {
        self.positions
            .binary_search_by_key(&index, |place| place.index)
            .ok()
    }

    /// Maps an ISO stream position back to the zero-based index and byte in the source.
    #[must_use]
    pub fn source_position(&self, index: u64, offset: u64) -> (u64, u64) {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.positions.get(index))
            .map_or((index, offset), |place| (place.index, place.offset))
    }

    /// Restores source locations and applies explicit choices to a catalog preview.
    pub fn review(&self, report: &mut PreviewReport, recipe: &Recipe) {
        report.detection = self.detection;
        let original = |index| self.source_position(index, 0).0;
        for sample in &mut report.samples {
            sample.index = original(sample.index);
        }
        for (index, _) in &mut report.scopes {
            *index = original(*index);
        }
        for (index, _) in &mut report.matches {
            *index = original(*index);
        }
        report.issues.retain_mut(|issue| {
            (issue.index, issue.offset) = self.source_position(issue.index, issue.offset);
            match issue.kind {
                IssueKind::MissingBarcode => {
                    !recipe.accept_missing_barcodes.contains(&(issue.index + 1))
                }
                IssueKind::BatchMatch(index) => {
                    issue.reason = batch_match_reason(original(index));
                    issue.kind = IssueKind::BatchMatch(original(index));
                    true
                }
                _ => true,
            }
        });
    }
}

/// Renders the selected source records as a bounded ISO 2709 stream.
///
/// # Errors
/// Reports an invalid source, recipe, edit or output limit.
pub fn render(
    input: impl BufRead,
    output: impl Write,
    recipe: &Recipe,
    max_bytes: usize,
) -> Result<RenderReport, PreviewError> {
    render_page(input, output, recipe, max_bytes, None, None)
}

/// Renders the bounded ISO stream and optionally presents a page of its original MARC.
///
/// # Errors
/// Reports the same errors as [`render`].
pub fn render_page(
    input: impl BufRead,
    mut output: impl Write,
    recipe: &Recipe,
    max_bytes: usize,
    from: Option<u64>,
    selected: Option<ImportFormat>,
) -> Result<RenderReport, PreviewError> {
    recipe.validate()?;
    let limits = Limits::default();
    let mut remaining = max_bytes.min(MAX_IMPORT_FILE_BYTES);
    let (mut read, mut excluded, mut changed) = (0, 0, 0);
    let mut samples = Vec::new();
    let mut source = Vec::new();
    let mut originals = std::collections::BTreeMap::new();
    let mut positions = Vec::new();
    let detection = scan_as(input, limits, selected, |raw| {
        let position = raw
            .as_ref()
            .map_or_else(|error| error.index, |record| record.index)
            + 1;
        read = position;
        if recipe.exclude.contains(&position) {
            excluded += 1;
            return Ok(());
        }
        let raw = raw.map_err(|error| {
            PreviewError::Edit(format!(
                "record {} at byte {}: {}",
                error.index + 1,
                error.offset,
                error.failure
            ))
        })?;
        let rules = recipe
            .edits
            .iter()
            .filter(|edit| edit.record.is_none_or(|index| index == position));
        let edited = edit_bytes(&raw.bytes, rules, limits, position, &mut samples)?;
        if edited.is_some() {
            changed += 1;
        }
        if edited.is_some() || raw.text.is_some() {
            originals.insert(
                positions.len() as u64,
                original_record(&raw.bytes, raw.text, limits)?,
            );
        }
        let bytes = edited.as_deref().unwrap_or(&raw.bytes);
        if from.is_some_and(|from| positions.len() as u64 >= from)
            && source.len() < MAX_PREVIEW_RECORDS
        {
            source.push(SourceSample {
                record: position,
                before: display_record(&raw.bytes, limits)?,
                after: edited
                    .as_deref()
                    .map(|bytes| display_record(bytes, limits))
                    .transpose()?,
            });
        }
        remaining = remaining
            .checked_sub(bytes.len())
            .ok_or(PreviewError::Limit("rendered bytes"))?;
        output.write_all(bytes).map_err(ImportError::Io)?;
        positions.push(RawPlace {
            index: raw.index,
            offset: raw.offset,
        });
        Ok(())
    })?;
    if recipe
        .exclude
        .last()
        .is_some_and(|position| *position > read)
        || recipe
            .accept_missing_barcodes
            .last()
            .is_some_and(|position| *position > read)
        || recipe
            .edits
            .iter()
            .any(|edit| edit.record.is_some_and(|position| position > read))
    {
        return Err(PreviewError::Edit(
            "recipe refers to a record outside the source".into(),
        ));
    }
    Ok(RenderReport {
        detection,
        read,
        excluded,
        changed,
        samples,
        source,
        originals,
        positions,
    })
}

fn original_record(
    bytes: &[u8],
    text: Option<super::TextOriginal>,
    limits: Limits,
) -> Result<crate::catalog::OriginalRecord, PreviewError> {
    let parsed = RecordRef::parse(bytes, &limits).map_err(|e| PreviewError::Edit(e.to_string()))?;
    let (declared, decoded, family) = super::decode_evidence(&mut Record::from_ref(&parsed))
        .map_err(|e| PreviewError::Edit(e.to_string()))?;
    Ok(match text {
        Some(text) => text.evidence(family),
        None => crate::catalog::OriginalRecord {
            bytes: bytes.to_vec(),
            declared,
            decoded,
            family,
            serialization: "ISO 2709".into(),
        },
    })
}

fn display_record(bytes: &[u8], limits: Limits) -> Result<String, PreviewError> {
    let parsed =
        RecordRef::parse(bytes, &limits).map_err(|error| PreviewError::Edit(error.to_string()))?;
    let mut record = Record::from_ref(&parsed);
    super::decode_evidence(&mut record).map_err(|e| PreviewError::Edit(e.to_string()))?;
    let mut display = format!("LDR  {}\n", String::from_utf8_lossy(&record.leader));
    for field in &record.fields {
        writeln!(display, "{}  {}", field.tag(), field_text(field))
            .map_err(|error| PreviewError::Edit(error.to_string()))?;
    }
    Ok(display)
}

fn edit_bytes<'a>(
    bytes: &[u8],
    mut rules: impl Iterator<Item = &'a Edit>,
    limits: Limits,
    position: u64,
    samples: &mut Vec<ChangedSample>,
) -> Result<Option<Vec<u8>>, PreviewError> {
    let Some(first) = rules.next() else {
        return Ok(None);
    };
    let fail = |reason| PreviewError::Edit(format!("record {position}: {reason}"));
    let parsed = RecordRef::parse(bytes, &limits).map_err(|_| fail("invalid ISO source"))?;
    let mut original = Record::from_ref(&parsed);
    super::decode_evidence(&mut original).map_err(|e| PreviewError::Edit(e.to_string()))?;
    let mut total = RECORD_OVERHEAD
        + original
            .fields
            .iter()
            .map(|field| field_size(field) + DIRECTORY_ENTRY_BYTES + 1)
            .sum::<usize>();
    let mut record = original.clone();
    for rule in std::iter::once(first).chain(rules) {
        rule.apply(&mut record, &mut total, limits).map_err(fail)?;
    }
    if original == record {
        return Ok(None);
    }
    if !protected(&original).eq(protected(&record)) {
        return Err(fail("protected source identifiers changed"));
    }
    let mut result = Vec::new();
    record.write_iso2709(&mut result).map_err(|error| {
        PreviewError::Edit(format!(
            "record {position}: {}",
            RecordFailure::Source(error)
        ))
    })?;
    if samples.len() < MAX_PREVIEW_RECORDS {
        samples.push(ChangedSample {
            record: position,
            fields: field_changes(&original, &record),
        });
    }
    Ok(Some(result))
}
