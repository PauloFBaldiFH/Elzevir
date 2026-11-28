// SPDX-License-Identifier: GPL-3.0-or-later
//! Batch import of MARC21 files (docs/specs/catalogo/importacao.md).
//!
//! Reading and mapping are pure, so they run on several threads; the records then go to
//! the writer one by one, in file order. The order is what keeps a parallel import equal
//! to a serial one (docs/arquitetura/03-estrategia-reconstrucao.md, §1.2 items 2 and 8).

pub mod batches;
pub mod biblivre;
pub mod containers;
pub mod edits;
pub mod koha_sql;
pub mod postgres;
pub mod preview;
pub mod relations;
pub mod sheet;
pub mod table;
mod unimarc;

use std::collections::VecDeque;
use std::io::BufRead;
use std::num::NonZeroUsize;

use elzevir_domain::catalog::InstanceId;
use elzevir_marc::{DetectedEncoding, Limits, MarcReader, ReadError, Record, RecordRef};

use crate::catalog::{
    CatalogOutput, CatalogRefusal, ImportOutcome, ImportedItems, NewRecord, RefusedItem,
};
use crate::koha_types::TypeTranslations;
use crate::marc_mapping::{MappingError, koha_record_type, marc_to_bibframe};
use crate::matching::Matching;
pub use crate::parallelism::Parallelism;

/// Settings of one import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportOptions {
    /// Explicit interpretation of Biblivre local holdings.
    pub biblivre: Option<biblivre::Profile>,
    /// Stable source installation for local identifiers.
    pub origin: Option<String>,
    /// Visibility explicitly selected for local notes.
    pub local_note_visibility: Option<crate::catalog::NoteVisibility>,
    /// Unedited ISO originals for records altered by a staged recipe.
    pub originals: std::collections::BTreeMap<u64, crate::catalog::OriginalRecord>,
    /// Approved scopes in the order of the rendered records.
    pub reviewed: Vec<crate::matching::MatchScope>,
    /// Limits applied to every record read.
    pub limits: Limits,
    /// How decoding and mapping run.
    pub parallelism: Parallelism,
    /// Records read and mapped together before they are handed to the writer.
    pub chunk: NonZeroUsize,
    /// How records are matched with stored ones; `None` stores every record
    /// (docs/specs/catalogo/casamento.md).
    pub matching: Option<Matching>,
    /// The library's translation of its Koha item types, applied to what each record does
    /// not already say (bibframe-completo.md, item 23e).
    pub translations: TypeTranslations,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            biblivre: None,
            limits: Limits::default(),
            parallelism: Parallelism::Serial,
            chunk: NonZeroUsize::new(1024).unwrap_or(NonZeroUsize::MIN),
            origin: None,
            local_note_visibility: None,
            reviewed: Vec::new(),
            originals: std::collections::BTreeMap::new(),
            matching: None,
            translations: TypeTranslations::default(),
        }
    }
}

/// Port: where mapped records go. The store implements it with the writer queue.
pub trait RecordSink {
    /// A record that was queued and has not been answered yet.
    type Pending: PendingRecord;

    /// Queues one record, waiting while the queue is full. Records are applied in the
    /// order they are queued. With `matching`, the writer matches the record with the
    /// stored ones and decides what to store.
    ///
    /// # Errors
    ///
    /// [`SinkClosed`] when the writer has stopped.
    fn submit(
        &self,
        record: NewRecord,
        matching: Option<Matching>,
    ) -> Result<Self::Pending, SinkClosed>;
}

/// The answer to one queued record.
pub trait PendingRecord {
    /// Waits until the record is committed or refused.
    ///
    /// # Errors
    ///
    /// Why the record was not stored.
    fn wait(self) -> Result<CatalogOutput, RecordRejected>;
}

/// The writer stopped, so nothing more can be imported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the writer has stopped")]
pub struct SinkClosed;

/// Why the writer did not store a record.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RecordRejected {
    /// A catalog rule refused it (unknown branch, barcode taken...).
    #[error(transparent)]
    Refused(CatalogRefusal),
    /// Storage failed; the message comes from the store.
    #[error("storage failed: {0}")]
    Storage(String),
    /// The writer stopped before answering.
    #[error("the writer stopped before answering")]
    WriterGone,
}

/// Why one record of the file was not imported. The import goes on with the next one.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RecordFailure {
    /// A Biblivre holding or its source identity cannot be interpreted unambiguously.
    #[error("Biblivre: {0}")]
    Biblivre(String),
    /// Authority and holdings records cannot be ingested as bibliographic descriptions.
    #[error("expected a bibliographic record")]
    NotBibliographic,
    /// A declared record character set cannot be decoded safely.
    #[error("unsupported or inconsistent UNIMARC character encoding")]
    Encoding,
    /// The bytes are not a valid ISO 2709 record (or break a limit).
    #[error(transparent)]
    Unreadable(elzevir_marc::RecordError),
    /// The record lacks what the model requires.
    #[error(transparent)]
    Mapping(MappingError),
    /// The converted record could not be written back as ISO 2709 for `source_records`.
    #[error(transparent)]
    Source(elzevir_marc::WriteError),
    /// The writer refused or failed to store it.
    #[error(transparent)]
    Rejected(RecordRejected),
}

/// One record that was not imported, and where it is in the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedRecord {
    /// Position of the record in the file (0-based).
    pub index: u64,
    /// Byte offset of the record's first byte.
    pub offset: u64,
    /// What went wrong.
    pub failure: RecordFailure,
}

/// What an import did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportReport {
    /// Records found in the file, imported or not.
    pub read: u64,
    /// Records stored.
    pub imported: u64,
    /// Items stored, with new records or added to matched ones.
    pub items: u64,
    /// Stored Items overwritten by imported ones with the same barcode.
    pub replaced_items: u64,
    /// Records that matched a stored one, whether they were stored again or not.
    pub matched: Vec<MatchedRecord>,
    /// Matched records whose description was replaced (`on_match = Replace`).
    pub replaced: u64,
    /// Records that matched nothing and were not stored (`on_no_match = Ignore`).
    pub skipped: u64,
    /// Records not stored, in file order.
    pub failures: Vec<FailedRecord>,
    /// Items refused one by one while the rest of their record went in, in file order.
    pub item_failures: Vec<FailedItem>,
}

/// A record of the file that matched a stored one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchedRecord {
    /// Position of the record in the file (0-based).
    pub index: u64,
    /// The stored Instance it matched.
    pub instance: InstanceId,
}

/// An Item that was refused on its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedItem {
    /// Position of the record in the file (0-based).
    pub index: u64,
    /// Position of the Item in the record (0-based).
    pub position: usize,
    /// Why it was refused.
    pub refusal: CatalogRefusal,
}

/// Why an import stopped before the end of the file. Records already stored stay.
#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    /// The input could not be read.
    #[error("cannot read the input: {0}")]
    Io(#[from] std::io::Error),
    /// The writer stopped.
    #[error(transparent)]
    SinkClosed(#[from] SinkClosed),
    /// A malformed or oversized XML input stopped its stream.
    #[error(transparent)]
    Xml(#[from] elzevir_marc::MarcXmlError),
    /// A malformed or oversized `MarcEdit` input stopped its stream.
    #[error(transparent)]
    MarcText(#[from] elzevir_marc::MarcTextError),
    /// A Koha dump failed complete validation before any submission.
    #[error(transparent)]
    KohaSql(#[from] koha_sql::DumpError),
}

/// A record read from the file, with its place in it.
struct TextOriginal {
    declared: bool,
    bytes: Vec<u8>,
    serialization: &'static str,
}
impl TextOriginal {
    fn evidence(self, family: String) -> crate::catalog::OriginalRecord {
        crate::catalog::OriginalRecord {
            bytes: self.bytes,
            declared: if self.declared { "UTF-8" } else { "undeclared" }.into(),
            decoded: "UTF-8".into(),
            serialization: self.serialization.into(),
            family,
        }
    }
}

struct RawRecord {
    text: Option<TextOriginal>,
    index: u64,
    offset: u64,
    bytes: Vec<u8>,
}

/// Imports every MARC21 record of an ISO 2709 file.
///
/// Records are decoded to UTF-8 and mapped (in parallel when `options` say so), then
/// queued in file order. A broken, unmappable or refused record is reported and skipped.
///
/// # Errors
///
/// [`ImportError`] when reading the input fails or the writer stops; what was already
/// committed stays.
pub fn import_marc21<R: BufRead, S: RecordSink>(
    input: R,
    sink: &S,
    options: &ImportOptions,
) -> Result<ImportReport, ImportError> {
    let mut reader = MarcReader::new(input, options.limits);
    import_chunks(
        |report| read_chunk(&mut reader, options.chunk.get(), report),
        sink,
        options,
    )
}

/// Imports UTF-8 MARCXML through the same mapping, queue and matching as ISO 2709.
/// Converted ISO bytes are retained as source; XML structural errors are terminal.
///
/// # Errors
///
/// [`ImportError`] on XML/I/O failure or a stopped writer; committed records stay.
pub fn import_marcxml<R: BufRead, S: RecordSink>(
    input: R,
    sink: &S,
    options: &ImportOptions,
) -> Result<ImportReport, ImportError> {
    let mut records = xml_records(input);
    import_converted(|| records.next().transpose(), sink, options)
}

fn xml_records(
    input: impl BufRead,
) -> impl Iterator<Item = Result<(Record, u64, TextOriginal), ImportError>> {
    let mut reader = elzevir_marc::MarcXmlReader::new(input, elzevir_marc::XmlLimits::default());
    std::iter::from_fn(move || {
        reader.next().map(|record| {
            Ok((
                record?,
                reader.record_offset(),
                TextOriginal {
                    bytes: reader.take_record_bytes(),
                    serialization: "MARCXML",
                    declared: reader.declares_utf8(),
                },
            ))
        })
    })
}

/// Imports UTF-8 `MarcEdit` mnemonic records through the ISO mapping, queue and matching.
/// Converted ISO bytes are retained as source.
///
/// # Errors
/// [`ImportError`] when the text stream fails or the writer stops. Records already
/// committed stay.
pub fn import_marc_text<R: BufRead, S: RecordSink>(
    input: R,
    sink: &S,
    options: &ImportOptions,
) -> Result<ImportReport, ImportError> {
    let mut records = mnemonic_records(input);
    import_converted(|| records.next().transpose(), sink, options)
}

fn mnemonic_records(
    input: impl BufRead,
) -> impl Iterator<Item = Result<(Record, u64, TextOriginal), ImportError>> {
    let mut reader = elzevir_marc::MarcTextReader::new(input, elzevir_marc::MrkLimits::default());
    std::iter::from_fn(move || {
        reader.next().map(|record| {
            Ok((
                record?,
                reader.record_offset(),
                TextOriginal {
                    bytes: reader.take_record_bytes(),
                    serialization: "MRK",
                    declared: false,
                },
            ))
        })
    })
}

/// Imports a validated partial Koha dump through the existing MARC writer path.
///
/// # Errors
/// Reports an invalid dump before submitting records, or a stopped writer.
pub fn import_koha_sql<R: BufRead, S: RecordSink>(
    input: R,
    sink: &S,
    options: &ImportOptions,
) -> Result<ImportReport, ImportError> {
    let mut records = koha_sql::with_originals(input)?.into_iter().map(|r| {
        (
            r.record,
            r.offset,
            TextOriginal {
                bytes: r.xml,
                declared: r.declares_utf8,
                serialization: "MARCXML",
            },
        )
    });
    import_converted(|| Ok(records.next()), sink, options)
}

/// Imports the catalog read from a whole Koha database through the same path as
/// [`import_koha_sql`] (docs/specs/migracao/troca-do-koha.md, item 8).
///
/// # Errors
/// A stopped writer; committed records stay.
pub fn import_koha_database<S: RecordSink>(
    catalog: koha_sql::DatabaseCatalog,
    sink: &S,
    options: &ImportOptions,
) -> Result<ImportReport, ImportError> {
    let mut records = catalog.records.into_iter().map(|r| {
        (
            r.record,
            r.offset,
            TextOriginal {
                bytes: r.xml,
                declared: r.declares_utf8,
                serialization: "MARCXML",
            },
        )
    });
    import_converted(|| Ok(records.next()), sink, options)
}

fn import_converted<S: RecordSink>(
    mut next: impl FnMut() -> Result<Option<(Record, u64, TextOriginal)>, ImportError>,
    sink: &S,
    options: &ImportOptions,
) -> Result<ImportReport, ImportError> {
    import_chunks(
        |report| {
            let mut chunk = Vec::with_capacity(options.chunk.get());
            while chunk.len() < options.chunk.get() {
                let Some((record, offset, text)) = next()? else {
                    break;
                };
                let place = RawPlace {
                    index: report.read,
                    offset,
                };
                report.read = report.read.saturating_add(1);
                match encode_source(&record, place, text) {
                    Ok(record) => chunk.push(record),
                    Err(failed) => report.failures.push(failed),
                }
            }
            Ok(chunk)
        },
        sink,
        options,
    )
}

fn encode_source(
    record: &Record,
    place: RawPlace,
    text: TextOriginal,
) -> Result<RawRecord, FailedRecord> {
    let mut converted;
    let record = if unimarc::recognizes(record) {
        // XML and mnemonic readers already validated and decoded their UTF-8 text.
        converted = record.clone();
        unimarc::declare_utf8(&mut converted);
        &converted
    } else {
        record
    };
    let mut bytes = Vec::new();
    record
        .write_iso2709(&mut bytes)
        .map_err(|error| place.failed(RecordFailure::Source(error)))?;
    Ok(RawRecord {
        text: Some(text),
        index: place.index,
        offset: place.offset,
        bytes,
    })
}

fn import_chunks<S: RecordSink>(
    mut read: impl FnMut(&mut ImportReport) -> Result<Vec<RawRecord>, ImportError>,
    sink: &S,
    options: &ImportOptions,
) -> Result<ImportReport, ImportError> {
    let mut report = ImportReport::default();
    // The previous chunk's answers, collected after the next chunk is mapped so that the
    // writer commits one chunk while the threads map the next.
    let mut waiting: VecDeque<(RawPlace, S::Pending)> = VecDeque::new();
    loop {
        let chunk = read(&mut report)?;
        if chunk.is_empty() {
            break;
        }
        let mapped = map_chunk(chunk, options);
        collect(&mut waiting, &mut report);
        for (place, result) in mapped {
            match result {
                Ok(record) => {
                    waiting.push_back((place, sink.submit(record, options.matching)?));
                }
                Err(failure) => report.failures.push(place.failed(failure)),
            }
        }
    }
    collect(&mut waiting, &mut report);
    report.failures.sort_by_key(|failed| failed.index);
    Ok(report)
}

/// Index and offset of a record, kept after its bytes are gone.
#[derive(Debug, Clone, Copy)]
struct RawPlace {
    index: u64,
    offset: u64,
}

impl RawPlace {
    fn failed(self, failure: RecordFailure) -> FailedRecord {
        FailedRecord {
            index: self.index,
            offset: self.offset,
            failure,
        }
    }
}

fn read_chunk<R: BufRead>(
    reader: &mut MarcReader<R>,
    size: usize,
    report: &mut ImportReport,
) -> Result<Vec<RawRecord>, ImportError> {
    let mut chunk = Vec::with_capacity(size);
    while chunk.len() < size {
        let index = report.read;
        let offset = reader.offset();
        match reader.next_record() {
            None => break,
            Some(Ok(record)) => {
                report.read = report.read.saturating_add(1);
                chunk.push(RawRecord {
                    text: None,
                    index,
                    offset,
                    bytes: record.as_bytes().to_vec(),
                });
            }
            Some(Err(ReadError::Record {
                index,
                offset,
                error,
            })) => {
                report.read = report.read.saturating_add(1);
                report.failures.push(FailedRecord {
                    index,
                    offset,
                    failure: RecordFailure::Unreadable(error),
                });
            }
            Some(Err(ReadError::Io(error))) => return Err(ImportError::Io(error)),
        }
    }
    Ok(chunk)
}

fn map_chunk(
    chunk: Vec<RawRecord>,
    options: &ImportOptions,
) -> Vec<(RawPlace, Result<NewRecord, RecordFailure>)> {
    options.parallelism.map(chunk, |raw| prepare(raw, options))
}

/// Decodes and maps one record. Pure: the same bytes give the same result on any thread.
fn prepare(
    raw: RawRecord,
    options: &ImportOptions,
) -> (RawPlace, Result<NewRecord, RecordFailure>) {
    let place = RawPlace {
        index: raw.index,
        offset: raw.offset,
    };
    (
        place,
        to_model(raw.bytes, &options.limits, &options.translations).and_then(|mut record| {
            for note in &mut record.instance.notes {
                if note.kind == elzevir_domain::catalog::NoteKind::Local {
                    note.visibility = options.local_note_visibility;
                }
            }
            if let Some(original) = options.originals.get(&place.index) {
                record.instance.original = Some(original.clone());
            } else if let Some(text) = raw.text
                && let Some(evidence) = record.instance.original.take()
            {
                record.instance.original = Some(text.evidence(evidence.family));
            }
            record.instance.origin.clone_from(&options.origin);
            record.instance.reviewed = usize::try_from(place.index)
                .ok()
                .and_then(|index| options.reviewed.get(index))
                .cloned()
                .map(Box::new);
            if let Some(profile) = &options.biblivre {
                profile.map(&mut record)?;
            }
            Ok(record)
        }),
    )
}

fn decode_evidence(record: &mut Record) -> Result<(String, String, String), RecordFailure> {
    if unimarc::recognizes(record) {
        let (declared, decoded) = unimarc::decode(record)?;
        return Ok((declared, decoded, "UNIMARC".into()));
    }
    let declared = marc21_declaration(record.leader[9]);
    let decoded = record.marc21_to_utf8();
    Ok((declared.into(), format!("{decoded:?}"), "MARC21".into()))
}

fn marc21_declaration(code: u8) -> &'static str {
    match code {
        b'a' => "UTF-8",
        b' ' => "MARC-8",
        _ => "unknown",
    }
}

fn to_model(
    bytes: Vec<u8>,
    limits: &Limits,
    translations: &TypeTranslations,
) -> Result<NewRecord, RecordFailure> {
    let parsed = RecordRef::parse(&bytes, limits).map_err(RecordFailure::Unreadable)?;
    if matches!(
        parsed.leader().get(6),
        Some(b'x' | b'y' | b'z' | b'u' | b'v')
    ) {
        return Err(RecordFailure::NotBibliographic);
    }
    let already_utf8 = parsed.leader().get(9) == Some(&b'a');
    let mut record = Record::from_ref(&parsed);
    if unimarc::recognizes(&record) {
        return unimarc::map(record, bytes);
    }
    let detected = record.marc21_to_utf8();
    // Keep the bytes as they arrived when they already were UTF-8 MARC21; otherwise keep
    // the converted record, since `source_records` holds UTF-8 (spec §2).
    let original = crate::catalog::OriginalRecord {
        serialization: "ISO 2709".into(),
        bytes: bytes.clone(),
        declared: marc21_declaration(parsed.leader()[9]).into(),
        decoded: format!("{detected:?}"),
        family: "MARC21".into(),
    };
    let source = if already_utf8 && detected == DetectedEncoding::Utf8 {
        bytes
    } else {
        let mut converted = Vec::with_capacity(bytes.len());
        record
            .write_iso2709(&mut converted)
            .map_err(RecordFailure::Source)?;
        converted
    };
    let mut model = marc_to_bibframe(&record, Some(source)).map_err(RecordFailure::Mapping)?;
    model.instance.original = Some(original);
    if let Some(code) = koha_record_type(&record) {
        translations.apply(
            &code,
            &mut model.work_description,
            &mut model.instance.description,
        );
    }
    Ok(model)
}

fn collect<P: PendingRecord>(waiting: &mut VecDeque<(RawPlace, P)>, report: &mut ImportReport) {
    while let Some((place, pending)) = waiting.pop_front() {
        match pending.wait() {
            Ok(CatalogOutput::Record { items, .. }) => {
                report.imported = report.imported.saturating_add(1);
                report.items = report
                    .items
                    .saturating_add(u64::try_from(items.len()).unwrap_or(u64::MAX));
            }
            Ok(CatalogOutput::Imported(outcome)) => record_outcome(place, outcome, report),
            Ok(_) => report.imported = report.imported.saturating_add(1),
            Err(rejected) => report
                .failures
                .push(place.failed(RecordFailure::Rejected(rejected))),
        }
    }
}

fn record_outcome(place: RawPlace, outcome: ImportOutcome, report: &mut ImportReport) {
    let mut matched = |instance| {
        report.matched.push(MatchedRecord {
            index: place.index,
            instance,
        });
    };
    let items = match outcome {
        ImportOutcome::Created {
            matched: found,
            items,
            ..
        } => {
            if let Some(instance) = found {
                matched(instance);
            }
            report.imported = report.imported.saturating_add(1);
            items
        }
        ImportOutcome::Replaced { instance, items } => {
            matched(instance);
            report.replaced = report.replaced.saturating_add(1);
            items
        }
        ImportOutcome::Matched { instance, items } => {
            matched(instance);
            items
        }
        ImportOutcome::Skipped => {
            report.skipped = report.skipped.saturating_add(1);
            return;
        }
    };
    let ImportedItems {
        added,
        replaced,
        refused,
    } = items;
    let count = |ids: &[_]| u64::try_from(ids.len()).unwrap_or(u64::MAX);
    report.items = report.items.saturating_add(count(&added));
    report.replaced_items = report.replaced_items.saturating_add(count(&replaced));
    report.item_failures.extend(
        refused
            .into_iter()
            .map(|RefusedItem { position, refusal }| FailedItem {
                index: place.index,
                position,
                refusal,
            }),
    );
}
