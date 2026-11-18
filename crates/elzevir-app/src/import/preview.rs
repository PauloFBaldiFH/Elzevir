// SPDX-License-Identifier: GPL-3.0-or-later
//! Full-file MARC preparation with a read-only view of the catalog.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, Read};

use elzevir_domain::catalog::InstanceId;
use elzevir_domain::statement::{EntityKind, check_description};
use elzevir_marc::{MarcTextReader, MarcXmlReader, MrkLimits, ReadError, XmlLimits};

use super::{
    FailedRecord, ImportError, ImportOptions, MarcReader, RawPlace, RawRecord, RecordFailure,
    encode_source, prepare,
};
use crate::catalog::{CatalogReadError, CatalogRefusal, ItemData, NewRecord};
use crate::import_detection::{
    Detection, Evidence, ImportFormat, MAX_DETECTION_BYTES, detect_format,
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
    /// The oldest stored instance with this key.
    ///
    /// # Errors
    /// Returns a catalog read failure.
    fn candidate(&self, key: &MatchKey) -> Result<Option<InstanceId>, CatalogReadError>;
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
}

/// Results of inspecting the whole source against one catalog view.
pub struct PreviewReport {
    /// Provisional format and its evidence.
    pub detection: Detection,
    /// Total records read.
    pub read: u64,
    /// Records that passed conversion and mapping.
    pub prepared: u64,
    /// First few prepared records.
    pub samples: Vec<PreviewSample>,
    /// Existing catalog candidates, indexed by source record.
    pub matches: Vec<(u64, InstanceId)>,
    /// All blocking diagnostics.
    pub issues: Vec<PreviewIssue>,
}

/// Prepares a MARC stream against an existing catalog or an empty catalog.
///
/// # Errors
/// Returns format, stream, limit and catalog read errors.
pub fn preview(
    mut input: impl BufRead,
    catalog: Option<&dyn PreviewCatalog>,
    options: &ImportOptions,
) -> Result<PreviewReport, PreviewError> {
    let mut prefix = Vec::new();
    input
        .by_ref()
        .take(MAX_DETECTION_BYTES as u64)
        .read_to_end(&mut prefix)
        .map_err(ImportError::Io)?;
    let detection = detect_format(&prefix);
    let input = prefix.as_slice().chain(input);
    let mut inspection = Inspection {
        options,
        catalog,
        keys: BTreeMap::new(),
        barcodes: BTreeSet::new(),
        report: PreviewReport {
            detection,
            read: 0,
            prepared: 0,
            samples: Vec::new(),
            matches: Vec::new(),
            issues: Vec::new(),
        },
    };
    if detection.evidence == Evidence::EmptyInput {
        // Recheck the provisional empty classification through EOF.
        let mut input = input.take(MAX_IMPORT_FILE_BYTES as u64 + 1);
        loop {
            let bytes = input.fill_buf().map_err(ImportError::Io)?;
            if bytes.is_empty() {
                check_bytes(MAX_IMPORT_FILE_BYTES as u64 + 1 - input.limit())?;
                return Ok(inspection.report);
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
    match detection.format {
        ImportFormat::Iso2709 => {
            let mut reader =
                MarcReader::new(input.take(MAX_IMPORT_FILE_BYTES as u64 + 1), options.limits);
            loop {
                let offset = reader.offset();
                let place = RawPlace {
                    index: inspection.report.read,
                    offset,
                };
                let record = match reader.next_record() {
                    Some(Ok(record)) => Ok(RawRecord {
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
                inspection.record(record)?;
            }
        }
        ImportFormat::MarcXml => {
            let mut reader = MarcXmlReader::new(input, XmlLimits::default());
            while let Some(record) = reader.next() {
                let place = RawPlace {
                    index: inspection.report.read,
                    offset: reader.record_offset(),
                };
                inspection.record(encode_source(&record.map_err(ImportError::Xml)?, place))?;
            }
        }
        ImportFormat::MarcText => {
            let mut reader = MarcTextReader::new(input, MrkLimits::default());
            while let Some(record) = reader.next() {
                let place = RawPlace {
                    index: inspection.report.read,
                    offset: reader.record_offset(),
                };
                inspection.record(encode_source(
                    &record.map_err(ImportError::MarcText)?,
                    place,
                ))?;
            }
        }
        _ => return Err(PreviewError::Format(detection)),
    }
    Ok(inspection.report)
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
    keys: BTreeMap<MatchKey, u64>,
    barcodes: BTreeSet<String>,
    report: PreviewReport,
}

impl Inspection<'_> {
    fn issue(&mut self, place: RawPlace, item: Option<usize>, reason: impl Into<String>) {
        self.report.issues.push(PreviewIssue {
            index: place.index,
            offset: place.offset,
            item,
            reason: reason.into(),
        });
    }

    fn record(&mut self, raw: Result<RawRecord, FailedRecord>) -> Result<(), PreviewError> {
        if self.report.read == MAX_IMPORT_RECORDS {
            return Err(PreviewError::Limit("record count"));
        }
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
                    error.failure.to_string(),
                );
                return Ok(());
            }
        };
        self.report.prepared += 1;
        if self.report.samples.len() < MAX_PREVIEW_RECORDS {
            self.report.samples.push(PreviewSample {
                index: place.index,
                work: record.work_title.clone(),
                instance: record.instance.title.clone(),
                items: record.items.len(),
            });
        }
        let decision = self.predict(place, &record)?;
        if decision.record != RecordAction::Ignore {
            for (kind, description) in [
                (EntityKind::Work, &record.work_description),
                (EntityKind::Instance, &record.instance.description),
            ] {
                if let Err(error) = check_description(kind, description) {
                    self.issue(place, None, error.to_string());
                }
            }
        }
        if decision.items != ItemsDecision::Ignore {
            for (position, item) in record.items.iter().enumerate() {
                self.item(place, position, item, decision.items)?;
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
        let (mut stored, mut earlier) = (None, None);
        for key in match_keys(record)
            .into_iter()
            .filter(|key| key.rule == matching.rule)
        {
            if let Some(catalog) = self.catalog
                && let Some(candidate) = catalog.candidate(&key)?
            {
                stored = stored.into_iter().chain([candidate]).min();
            }
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
                format!("possible match within the batch with record {}", index + 1),
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
            Some(CatalogRefusal::UnknownBranch(_)) => Some("unknown branch"),
            Some(CatalogRefusal::UnknownItemType(_)) => Some("unknown item type"),
            Some(_) => Some("item registry refusal"),
            None => None,
        };
        if let Some(reason) = reason {
            self.issue(place, Some(position), reason);
        }
        let Some(barcode) = &item.barcode else {
            self.issue(place, Some(position), "missing barcode");
            return Ok(());
        };
        let repeated = !self.barcodes.insert(barcode.clone());
        if action != ItemsDecision::Replace {
            if repeated {
                self.issue(
                    place,
                    Some(position),
                    "barcode already present in the batch",
                );
            }
            if let Some(catalog) = self.catalog
                && catalog.barcode_exists(barcode)?
            {
                self.issue(place, Some(position), "barcode already in use");
            }
        }
        Ok(())
    }
}
