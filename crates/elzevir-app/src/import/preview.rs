// SPDX-License-Identifier: GPL-3.0-or-later
//! Full-file MARC preparation with a read-only view of the catalog.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, Read};

use elzevir_domain::catalog::InstanceId;
use elzevir_domain::statement::{EntityKind, check_description};
use elzevir_marc::{Limits, ReadError, Record};

use super::{
    FailedRecord, ImportError, ImportOptions, MarcReader, RawPlace, RawRecord, RecordFailure,
    encode_source, prepare,
};
use crate::catalog::{CatalogReadError, CatalogRefusal, ItemData, NewRecord, RecordItem};
use crate::import_detection::{
    Detection, Evidence, ImportFormat, MAX_DETECTION_BYTES, detect_format, select_prepared,
};
use crate::matching::{Decision, ItemsDecision, MatchKey, RecordAction, decide, match_keys};

/// Maximum source size, including terminators.
pub const MAX_IMPORT_FILE_BYTES: usize = 512 * 1024 * 1024;
/// Maximum number of records inspected.
pub const MAX_IMPORT_RECORDS: u64 = 100_000;
/// Number of mapped records shown as samples.
pub const MAX_PREVIEW_RECORDS: usize = 3;

/// Read-only catalog queries used for predictions.
pub trait PreviewCatalog {
    /// All matching Instances and affected catalog rows in this read snapshot.
    ///
    /// # Errors
    /// Returns a catalog read failure.
    fn scope(
        &self,
        record: &NewRecord,
        matching: &crate::matching::Matching,
        origin: Option<&str>,
    ) -> Result<crate::matching::MatchScope, CatalogReadError>;
    /// Whether a barcode already belongs to a stored item.
    ///
    /// # Errors
    /// Returns a catalog read failure.
    fn barcode_exists(&self, barcode: &str) -> Result<bool, CatalogReadError>;
    /// Checks the branch and policy codes required by the writer.
    ///
    /// # Errors
    /// Returns a catalog read failure.
    fn item_refusal(&self, item: &ItemData) -> Result<Option<CatalogRefusal>, CatalogReadError>;
}

/// A preparation stopped before reaching the end of the source.
#[derive(Debug, thiserror::Error)]
pub enum PreviewError {
    /// Format without a supported preparer.
    #[error("unsupported or inconclusive preview format: {0:?}")]
    Format(Detection),
    /// An input, structural or conversion stream failure.
    #[error(transparent)]
    Input(#[from] ImportError),
    /// A named preview limit was exceeded.
    #[error("preview limit exceeded: {0}")]
    Limit(&'static str),
    /// A needed read could not be completed.
    #[error(transparent)]
    Catalog(#[from] CatalogReadError),
    /// A recipe is invalid or cannot be applied to its selected record.
    #[error("{0}")]
    Edit(String),
    /// The selected spreadsheet could not be prepared.
    #[error(transparent)]
    Sheet(#[from] super::sheet::SheetError),
}

/// A mapped record sample, without the raw source.
pub struct PreviewSample {
    /// Zero-based source record index.
    pub index: u64,
    /// Work title.
    pub work: String,
    /// Instance title.
    pub instance: String,
    /// Items described by this record.
    pub items: usize,
    /// The prepared record, as the writer would receive it, for the Work, Instance
    /// and Item blocks of the review screen.
    pub record: Box<NewRecord>,
}

/// A blocking diagnostic in source order.
pub struct PreviewIssue {
    /// Zero-based record index.
    pub index: u64,
    /// Source byte offset of the record.
    pub offset: u64,
    /// Zero-based item position when applicable.
    pub item: Option<usize>,
    /// Reason for review or refusal.
    pub reason: String,
    /// Classification for localized diagnostics.
    pub kind: IssueKind,
}

/// Reason a preview requires review.
#[derive(Clone, Copy)]
pub enum IssueKind {
    /// Source record could not be converted or mapped.
    InvalidRecord,
    /// A prepared description failed validation.
    InvalidDescription,
    /// An item refers to a missing branch.
    UnknownBranch,
    /// An item refers to a missing type.
    UnknownItemType,
    /// An item failed another registry check.
    ItemRegistry,
    /// No barcode was supplied.
    MissingBarcode,
    /// The barcode is repeated in the source.
    RepeatedBarcode,
    /// The barcode already belongs to a catalog item.
    UsedBarcode,
    /// A possible match with an earlier zero-based source record.
    BatchMatch(u64),
}

pub(super) fn batch_match_reason(index: u64) -> String {
    format!("possible match within the batch with record {}", index + 1)
}

/// Results of inspecting the whole source against one catalog view.
pub struct PreviewReport {
    /// Provisional format and its evidence.
    pub detection: Detection,
    /// Total records read.
    pub read: u64,
    /// Records that passed conversion and mapping.
    pub prepared: u64,
    /// Items the selected matching action would submit.
    pub items: u64,
    /// First few prepared records.
    pub samples: Vec<PreviewSample>,
    /// Existing catalog candidates, indexed by source record.
    pub matches: Vec<(u64, InstanceId)>,
    /// Scope of each prepared unit, in submission order.
    pub scopes: Vec<(u64, crate::matching::MatchScope)>,
    /// All blocking diagnostics.
    pub issues: Vec<PreviewIssue>,
}

/// Prepares a MARC stream against an existing catalog or an empty catalog.
///
/// # Errors
/// Returns format, stream, limit and catalog read errors.
pub fn preview(
    input: impl BufRead,
    catalog: Option<&dyn PreviewCatalog>,
    options: &ImportOptions,
) -> Result<PreviewReport, PreviewError> {
    preview_page(input, catalog, options, 0)
}

/// Validates the whole input and returns samples starting at the selected record.
///
/// # Errors
/// Reports the same preparation errors as [`preview`].
pub fn preview_page(
    input: impl BufRead,
    catalog: Option<&dyn PreviewCatalog>,
    options: &ImportOptions,
    from: u64,
) -> Result<PreviewReport, PreviewError> {
    let mut inspection = inspection(catalog, options, from);
    inspection.report.detection = scan(input, options.limits, |record| inspection.record(record))?;
    Ok(inspection.report)
}

/// Reviews fully mapped native records against the same catalog prediction rules.
///
/// # Errors
/// Returns a catalog read failure.
pub fn preview_table(
    table: &super::sheet::PreparedTable,
    catalog: Option<&dyn PreviewCatalog>,
    options: &ImportOptions,
    from: u64,
) -> Result<PreviewReport, PreviewError> {
    let mut inspection = inspection(catalog, options, from);
    inspection.report.detection = Detection {
        format: ImportFormat::Delimited,
        evidence: Evidence::DelimitedRows,
    };
    for (index, row) in table.records().iter().enumerate() {
        inspection.report.read += 1;
        let place = RawPlace {
            index: index as u64,
            offset: row.offset,
        };
        inspection.prepared(place, &row.record)?;
    }
    Ok(inspection.report)
}

fn inspection<'a>(
    catalog: Option<&'a dyn PreviewCatalog>,
    options: &'a ImportOptions,
    from: u64,
) -> Inspection<'a> {
    Inspection {
        options,
        catalog,
        from,
        keys: BTreeMap::new(),
        barcodes: BTreeSet::new(),
        report: PreviewReport {
            detection: Detection {
                format: ImportFormat::Unknown,
                evidence: Evidence::Inconclusive,
            },
            read: 0,
            prepared: 0,
            items: 0,
            samples: Vec::new(),
            scopes: Vec::new(),
            matches: Vec::new(),
            issues: Vec::new(),
        },
    }
}

pub(super) fn scan(
    input: impl BufRead,
    limits: Limits,
    visit: impl FnMut(Result<RawRecord, FailedRecord>) -> Result<(), PreviewError>,
) -> Result<Detection, PreviewError> {
    scan_as(input, limits, None, visit)
}

pub(super) fn scan_as(
    mut input: impl BufRead,
    limits: Limits,
    selected: Option<ImportFormat>,
    mut visit: impl FnMut(Result<RawRecord, FailedRecord>) -> Result<(), PreviewError>,
) -> Result<Detection, PreviewError> {
    let mut prefix = Vec::new();
    input
        .by_ref()
        .take(MAX_DETECTION_BYTES as u64)
        .read_to_end(&mut prefix)
        .map_err(ImportError::Io)?;
    let mut detection = preview_format(detect_format(&prefix), selected)?;
    let input = prefix.as_slice().chain(input);
    let mut index = 0;
    let mut visit = |index, value| {
        if index >= MAX_IMPORT_RECORDS {
            return Err(PreviewError::Limit("record count"));
        }
        visit(value)
    };
    if detection.evidence == Evidence::EmptyInput && detection.format != ImportFormat::KohaSqlDump {
        return verify_empty(input).map(|()| detection);
    }
    match detection.format {
        ImportFormat::Iso2709 => {
            let mut reader = MarcReader::new(input.take(MAX_IMPORT_FILE_BYTES as u64 + 1), limits);
            loop {
                let offset = reader.offset();
                let place = RawPlace { index, offset };
                let record = match reader.next_record() {
                    Some(Ok(record)) => Ok(RawRecord {
                        text: None,
                        index: place.index,
                        offset,
                        bytes: record.as_bytes().to_vec(),
                    }),
                    Some(Err(ReadError::Record { error, .. })) => {
                        Err(place.failed(RecordFailure::Unreadable(error)))
                    }
                    Some(Err(ReadError::Io(error))) => return Err(ImportError::Io(error).into()),
                    None => {
                        check_bytes(reader.offset())?;
                        break;
                    }
                };
                check_bytes(reader.offset())?;
                visit(index, record)?;
                index += 1;
            }
        }
        ImportFormat::MarcXml => scan_text(super::xml_records(input), &mut visit)?,
        ImportFormat::MarcText => scan_text(super::mnemonic_records(input), &mut visit)?,
        ImportFormat::KohaSqlDump => {
            let records = super::koha_sql::with_originals(input).map_err(ImportError::from)?;
            if !records.is_empty() {
                detection.evidence = Evidence::KohaCatalogTables;
            }
            for (index, row) in records.into_iter().enumerate() {
                let offset = row.offset;
                let place = RawPlace {
                    index: index as u64,
                    offset,
                };
                visit(
                    place.index,
                    encode_source(
                        &row.record,
                        place,
                        super::TextOriginal {
                            bytes: row.xml,
                            declared: row.declares_utf8,
                            serialization: "MARCXML",
                        },
                    ),
                )?;
            }
        }
        _ => return Err(PreviewError::Format(detection)),
    }
    Ok(detection)
}

fn scan_text(
    records: impl Iterator<Item = Result<(Record, u64, super::TextOriginal), ImportError>>,
    visit: &mut impl FnMut(u64, Result<RawRecord, FailedRecord>) -> Result<(), PreviewError>,
) -> Result<(), PreviewError> {
    for (index, row) in (0..).zip(records) {
        let (record, offset, text) = row?;
        visit(
            index,
            encode_source(&record, RawPlace { index, offset }, text),
        )?;
    }
    Ok(())
}

fn preview_format(
    detected: Detection,
    selected: Option<ImportFormat>,
) -> Result<Detection, PreviewError> {
    if detected.format == ImportFormat::KohaFullBackup {
        return Err(ImportError::from(super::koha_sql::DumpError {
            offset: 0,
            problem: super::koha_sql::DumpProblem::FullBackup,
        })
        .into());
    }
    if selected.is_some() {
        select_prepared(detected, selected).ok_or(PreviewError::Format(detected))
    } else {
        Ok(detected)
    }
}

fn verify_empty(input: impl BufRead) -> Result<(), PreviewError> {
    // Recheck the provisional empty classification through EOF.
    let mut input = input.take(MAX_IMPORT_FILE_BYTES as u64 + 1);
    loop {
        let bytes = input.fill_buf().map_err(ImportError::Io)?;
        if bytes.is_empty() {
            return check_bytes(MAX_IMPORT_FILE_BYTES as u64 + 1 - input.limit());
        }
        if !bytes.iter().all(u8::is_ascii_whitespace) {
            return Err(PreviewError::Format(Detection {
                format: ImportFormat::Unknown,
                evidence: Evidence::Inconclusive,
            }));
        }
        let count = bytes.len();
        input.consume(count);
    }
}

fn check_bytes(bytes: u64) -> Result<(), PreviewError> {
    if bytes > MAX_IMPORT_FILE_BYTES as u64 {
        return Err(PreviewError::Limit("input bytes"));
    }
    Ok(())
}

struct Inspection<'a> {
    options: &'a ImportOptions,
    catalog: Option<&'a dyn PreviewCatalog>,
    from: u64,
    keys: BTreeMap<MatchKey, u64>,
    barcodes: BTreeSet<String>,
    report: PreviewReport,
}

impl Inspection<'_> {
    fn issue(
        &mut self,
        place: RawPlace,
        item: Option<usize>,
        kind: IssueKind,
        reason: impl Into<String>,
    ) {
        self.report.issues.push(PreviewIssue {
            index: place.index,
            offset: place.offset,
            item,
            reason: reason.into(),
            kind,
        });
    }

    fn record(&mut self, raw: Result<RawRecord, FailedRecord>) -> Result<(), PreviewError> {
        self.report.read += 1;
        let prepared = raw.and_then(|raw| {
            let (place, mapped) = prepare(raw, self.options);
            mapped
                .map(|record| (place, record))
                .map_err(|error| place.failed(error))
        });
        let (place, record) = match prepared {
            Ok(record) => record,
            Err(error) => {
                self.issue(
                    RawPlace {
                        index: error.index,
                        offset: error.offset,
                    },
                    None,
                    IssueKind::InvalidRecord,
                    error.failure.to_string(),
                );
                return Ok(());
            }
        };
        self.prepared(place, &record)
    }

    fn prepared(&mut self, place: RawPlace, record: &NewRecord) -> Result<(), PreviewError> {
        self.report.prepared += 1;
        if place.index >= self.from && self.report.samples.len() < MAX_PREVIEW_RECORDS {
            self.report.samples.push(PreviewSample {
                index: place.index,
                work: record.work_title.clone(),
                instance: record.instance.title.clone(),
                items: record.items.len(),
                record: Box::new(record.clone()),
            });
        }
        let decision = self.predict(place, record)?;
        if decision.record != RecordAction::Ignore {
            for (kind, description) in [
                (EntityKind::Work, &record.work_description),
                (EntityKind::Instance, &record.instance.description),
            ] {
                if let Err(error) = check_description(kind, description) {
                    self.issue(
                        place,
                        None,
                        IssueKind::InvalidDescription,
                        error.to_string(),
                    );
                }
            }
        }
        if decision.items != ItemsDecision::Ignore {
            self.report.items += record.items.len() as u64;
            for (position, item) in record.items.entries().enumerate() {
                match item {
                    RecordItem::Marc(item) => self.item(place, position, item, decision.items)?,
                    RecordItem::Native(item) => {
                        if let Some(barcode) = &item.fields.barcode {
                            self.barcode(place, position, barcode, decision.items)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn predict(
        &mut self,
        place: RawPlace,
        record: &NewRecord,
    ) -> Result<Decision, CatalogReadError> {
        let Some(matching) = self.options.matching else {
            return Ok(Decision {
                record: RecordAction::CreateNew,
                items: ItemsDecision::Add,
            });
        };
        let mut earlier = None;
        let scope = self
            .catalog
            .map(|catalog| catalog.scope(record, &matching, self.options.origin.as_deref()))
            .transpose()?
            .unwrap_or_default();
        let stored = match scope.result() {
            crate::matching::MatchResult::Unique(id) => Some(id),
            crate::matching::MatchResult::Absent => None,
            result => {
                let reason = match result {
                    crate::matching::MatchResult::Conflict(reason) => reason,
                    _ => "multiple candidate Instances",
                };
                self.issue(place, None, IssueKind::InvalidDescription, reason);
                None
            }
        };
        self.report.scopes.push((place.index, scope));
        for key in match_keys(record)
            .into_iter()
            .filter(|key| key.rule == matching.rule)
        {
            let first = *self.keys.entry(key).or_insert(place.index);
            if first != place.index {
                earlier = earlier.into_iter().chain([first]).min();
            }
        }
        if let Some(instance) = stored {
            self.report.matches.push((place.index, instance));
        }
        if let Some(index) = earlier {
            self.issue(
                place,
                None,
                IssueKind::BatchMatch(index),
                batch_match_reason(index),
            );
        }
        let matched = stored.is_some() || earlier.is_some();
        let mut decision = decide(&matching, matched);
        if !matched && decision.record == RecordAction::Ignore {
            decision.items = ItemsDecision::Ignore;
        }
        Ok(decision)
    }

    fn item(
        &mut self,
        place: RawPlace,
        position: usize,
        item: &ItemData,
        action: ItemsDecision,
    ) -> Result<(), CatalogReadError> {
        let refusal = match self.catalog {
            Some(catalog) => catalog.item_refusal(item)?,
            None => Some(CatalogRefusal::UnknownBranch(String::new())),
        };
        let reason = match refusal {
            Some(CatalogRefusal::UnknownBranch(_)) => {
                Some((IssueKind::UnknownBranch, "unknown branch"))
            }
            Some(CatalogRefusal::UnknownItemType(_)) => {
                Some((IssueKind::UnknownItemType, "unknown item type"))
            }
            Some(_) => Some((IssueKind::ItemRegistry, "item registry refusal")),
            None => None,
        };
        if let Some((kind, reason)) = reason {
            self.issue(place, Some(position), kind, reason);
        }
        let Some(barcode) = &item.barcode else {
            self.issue(
                place,
                Some(position),
                IssueKind::MissingBarcode,
                "missing barcode",
            );
            return Ok(());
        };
        self.barcode(place, position, barcode, action)
    }

    fn barcode(
        &mut self,
        place: RawPlace,
        position: usize,
        barcode: &str,
        action: ItemsDecision,
    ) -> Result<(), CatalogReadError> {
        let repeated = !self.barcodes.insert(barcode.to_owned());
        if action != ItemsDecision::Replace {
            if repeated {
                self.issue(
                    place,
                    Some(position),
                    IssueKind::RepeatedBarcode,
                    "barcode already present in the batch",
                );
            }
            if let Some(catalog) = self.catalog
                && catalog.barcode_exists(barcode)?
            {
                self.issue(
                    place,
                    Some(position),
                    IssueKind::UsedBarcode,
                    "barcode already in use",
                );
            }
        }
        Ok(())
    }
}
