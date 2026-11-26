// SPDX-License-Identifier: GPL-3.0-or-later
//! Export of the catalog as MARC21 (docs/specs/catalogo/linha-de-comando.md).
//!
//! Records are read in Instance order, a batch at a time, and written back with
//! `bibframe_to_marc` starting from the stored source record: a record nobody edited comes
//! out as it came in.

use std::collections::BTreeSet;
use std::io::{BufRead, Write};
use std::num::NonZeroUsize;

use elzevir_domain::catalog::{InstanceId, WorkId};
use elzevir_marc::{
    Limits, MARCXML_COLLECTION_FOOTER, MARCXML_COLLECTION_HEADER, MarcReader, Record, RecordRef,
};

use crate::catalog::{CatalogReadError, NewRecord};
use crate::marc_mapping::{bibframe_to_marc, marc_to_bibframe};
use crate::rdf::HeldItem;

/// One stored record, as the model holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRecord {
    /// The Instance.
    pub instance: InstanceId,
    /// Its Work.
    pub work: WorkId,
    /// Its Work, Instance and Items; `source` is the stored source record.
    pub record: NewRecord,
}

/// Port: reads the stored records in Instance order. The store implements it.
pub trait RecordSource {
    /// Up to `limit` records whose Instance comes after `after` (from the first when
    /// `None`), in Instance order.
    ///
    /// # Errors
    ///
    /// When storage fails or holds a value this build does not understand.
    fn records_after(
        &self,
        after: Option<InstanceId>,
        limit: NonZeroUsize,
    ) -> Result<Vec<StoredRecord>, CatalogReadError>;

    /// The Items of `instance` as the RDF export describes them, by the names of the
    /// registries (bibframe-completo.md, item 28b), in Item order.
    ///
    /// # Errors
    ///
    /// When storage fails or holds a value this build does not understand.
    fn held_items(&self, instance: InstanceId) -> Result<Vec<HeldItem>, CatalogReadError>;
}

/// Settings of one export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportOptions {
    /// Limits applied when reading back the stored source records.
    pub limits: Limits,
    /// Records read from storage at a time.
    pub batch: NonZeroUsize,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            limits: Limits::default(),
            batch: NonZeroUsize::new(1024).unwrap_or(NonZeroUsize::MIN),
        }
    }
}

/// Why one record was not exported. The export goes on with the next one.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExportFailure {
    /// The stored source record is not valid ISO 2709 any more.
    #[error("the stored source record is unreadable: {0}")]
    Source(elzevir_marc::RecordError),
    /// The record does not fit in ISO 2709 (too long, too many fields).
    #[error(transparent)]
    Write(elzevir_marc::WriteError),
}

/// One record left out of the export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedExport {
    /// The Instance.
    pub instance: InstanceId,
    /// What went wrong.
    pub failure: ExportFailure,
}

/// What an export did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExportReport {
    /// Records written.
    pub written: u64,
    /// Records left out, in Instance order.
    pub failures: Vec<FailedExport>,
}

/// Why an export stopped. What was written stays written.
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    /// The catalog could not be read.
    #[error(transparent)]
    Read(#[from] CatalogReadError),
    /// The output could not be written.
    #[error("cannot write the output: {0}")]
    Io(#[from] std::io::Error),
}

/// Writes every stored record as ISO 2709 MARC21 (UTF-8), in Instance order.
///
/// # Errors
///
/// [`ExportError`] when reading the catalog or writing the output fails.
pub fn export_marc21<W: Write, S: RecordSource>(
    source: &S,
    out: &mut W,
    options: &ExportOptions,
) -> Result<ExportReport, ExportError> {
    let mut report = ExportReport::default();
    let mut after = None;
    let mut bytes = Vec::new();
    loop {
        let batch = source.records_after(after, options.batch)?;
        let Some(last) = batch.last() else {
            break;
        };
        after = Some(last.instance);
        for stored in batch {
            bytes.clear();
            match to_marc(&stored.record, &options.limits, &mut bytes) {
                Ok(()) => {
                    out.write_all(&bytes)?;
                    report.written = report.written.saturating_add(1);
                }
                Err(failure) => report.failures.push(FailedExport {
                    instance: stored.instance,
                    failure,
                }),
            }
        }
    }
    out.flush()?;
    Ok(report)
}

/// The format of a one-record export (docs/specs/catalogo/listagem.md, item 34).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordFormat {
    /// ISO 2709, UTF-8.
    Marc21,
    /// A MARCXML document with one `<record>`.
    MarcXml,
    /// UNIMARC in ISO 2709, UTF-8 (docs/specs/catalogo/unimarc.md).
    Unimarc,
    /// UNIMARC in the MARCXML layout.
    UnimarcXml,
}

impl RecordFormat {
    /// The format a `format=` value names: `marc21` (also when absent), `marcxml`,
    /// `unimarc` or `unimarcxml`.
    #[must_use]
    pub fn of(name: Option<&str>) -> Option<Self> {
        match name {
            None | Some("marc21") => Some(Self::Marc21),
            Some("marcxml") => Some(Self::MarcXml),
            Some("unimarc") => Some(Self::Unimarc),
            Some("unimarcxml") => Some(Self::UnimarcXml),
            Some(_) => None,
        }
    }

    /// The `Content-Type` of the file.
    #[must_use]
    pub const fn content_type(self) -> &'static str {
        match self {
            Self::Marc21 | Self::Unimarc => "application/marc",
            Self::MarcXml | Self::UnimarcXml => "application/marcxml+xml; charset=utf-8",
        }
    }

    /// The end of the file's name: `.mrc` and `.xml`, after `-unimarc` for UNIMARC.
    #[must_use]
    pub const fn file_suffix(self) -> &'static str {
        match self {
            Self::Marc21 => ".mrc",
            Self::MarcXml => ".xml",
            Self::Unimarc => "-unimarc.mrc",
            Self::UnimarcXml => "-unimarc.xml",
        }
    }
}

/// Why one record could not be exported.
#[derive(Debug, thiserror::Error)]
pub enum RecordExportError {
    /// The catalog could not be read.
    #[error(transparent)]
    Read(#[from] CatalogReadError),
    /// The record could not be written.
    #[error(transparent)]
    Failure(#[from] ExportFailure),
}

/// One stored record in `format`, written as the batch export writes it; `None` when the
/// Instance does not exist.
///
/// # Errors
///
/// [`RecordExportError`] when reading the catalog or writing the record fails.
pub fn export_record<S: RecordSource + ?Sized>(
    source: &S,
    instance: InstanceId,
    format: RecordFormat,
    limits: &Limits,
) -> Result<Option<Vec<u8>>, RecordExportError> {
    let Some(stored) = stored_record(source, instance)? else {
        return Ok(None);
    };
    let mut bytes = Vec::new();
    let mut xml = String::new();
    write_record(
        &stored.record,
        format,
        limits,
        &mut bytes,
        &mut xml,
        Layout::Document,
    )?;
    Ok(Some(if xml.is_empty() {
        bytes
    } else {
        xml.into_bytes()
    }))
}

/// Whether a MARCXML record stands alone or goes inside a `<collection>`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Layout {
    Document,
    Collection,
}

/// `record` in `format`: ISO 2709 appended to `bytes`, MARCXML to `xml`.
fn write_record(
    record: &NewRecord,
    format: RecordFormat,
    limits: &Limits,
    bytes: &mut Vec<u8>,
    xml: &mut String,
    layout: Layout,
) -> Result<(), ExportFailure> {
    if matches!(format, RecordFormat::Unimarc | RecordFormat::UnimarcXml) {
        let unimarc = to_unimarc(record, limits)?;
        return match (format, layout) {
            (RecordFormat::Unimarc, _) => unimarc.write_iso2709(bytes),
            (_, Layout::Document) => unimarc.write_unimarc_xml_document(xml),
            (_, Layout::Collection) => unimarc.write_unimarc_xml_collection_record(xml),
        }
        .map_err(ExportFailure::Write);
    }
    let mut marc21 = Vec::new();
    to_marc(record, limits, &mut marc21)?;
    if format == RecordFormat::Marc21 {
        bytes.extend_from_slice(&marc21);
        return Ok(());
    }
    let record =
        Record::from_ref(&RecordRef::parse(&marc21, limits).map_err(ExportFailure::Source)?);
    match layout {
        Layout::Document => record.write_marcxml_document(xml),
        Layout::Collection => record.write_marcxml_collection_record(xml),
    }
    .map_err(ExportFailure::Write)
}

/// `record` as UNIMARC (docs/specs/catalogo/unimarc.md, items 17 to 19): a record read from
/// UNIMARC and not changed since leaves as it came, in UTF-8; any other goes through its
/// MARC21 export, with the fields of its UNIMARC original that the projection does not
/// read.
fn to_unimarc(record: &NewRecord, limits: &Limits) -> Result<Record, ExportFailure> {
    let original = record
        .instance
        .original
        .as_ref()
        .filter(|original| original.family == "UNIMARC")
        .and_then(|original| RecordRef::parse(&original.bytes, limits).ok())
        .map(|parsed| Record::from_ref(&parsed))
        // An original the import read always decodes again; one that does not is left out.
        .and_then(|mut original| crate::unimarc::decode(&mut original).ok().map(|_| original));
    if let (Some(original), Some(source)) = (&original, record.source.as_deref()) {
        let projection =
            Record::from_ref(&RecordRef::parse(source, limits).map_err(ExportFailure::Source)?);
        if unchanged(record, &projection) {
            let mut out = original.clone();
            out.leader[9] = b' ';
            return Ok(out);
        }
    }
    let mut marc21 = Vec::new();
    to_marc(record, limits, &mut marc21)?;
    let marc21 =
        Record::from_ref(&RecordRef::parse(&marc21, limits).map_err(ExportFailure::Source)?);
    let mut out = crate::unimarc::from_marc21(&marc21);
    if let Some(original) = &original {
        crate::unimarc::keep_unread(&mut out, original);
    }
    Ok(out)
}

/// One stored record as the export writes it, read back as a MARC record; `None` when
/// the Instance does not exist (docs/specs/opac/oai-pmh.md, item 15).
///
/// # Errors
///
/// [`RecordExportError`] when reading the catalog or writing the record fails.
pub fn marc_record<S: RecordSource + ?Sized>(
    source: &S,
    instance: InstanceId,
    limits: &Limits,
) -> Result<Option<Record>, RecordExportError> {
    let Some(stored) = stored_record(source, instance)? else {
        return Ok(None);
    };
    let mut marc21 = Vec::new();
    to_marc(&stored.record, limits, &mut marc21)?;
    let record = RecordRef::parse(&marc21, limits).map_err(ExportFailure::Source)?;
    Ok(Some(Record::from_ref(&record)))
}

/// A selection of records written as one file (docs/specs/catalogo/lote.md, items 7
/// and 8).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SelectionExport {
    /// The file: the records one after the other, or one MARCXML collection.
    pub bytes: Vec<u8>,
    /// Records left out, in the order asked.
    pub failures: Vec<FailedExport>,
}

/// The records of `instances`, in that order, in `format`, each written as the batch
/// export writes it. Instances that do not exist are left out silently; records that
/// cannot be written are left out and named.
///
/// # Errors
///
/// When the catalog cannot be read.
pub fn export_selection<S: RecordSource + ?Sized>(
    source: &S,
    instances: &[InstanceId],
    format: RecordFormat,
    limits: &Limits,
) -> Result<SelectionExport, CatalogReadError> {
    let mut export = SelectionExport::default();
    let xml_format = matches!(format, RecordFormat::MarcXml | RecordFormat::UnimarcXml);
    let mut xml = String::new();
    if xml_format {
        xml.push_str(MARCXML_COLLECTION_HEADER);
    }
    for &instance in instances {
        let Some(stored) = stored_record(source, instance)? else {
            continue;
        };
        // A record that fails leaves nothing behind: each is written apart first.
        let mut bytes = Vec::new();
        let mut one = String::new();
        match write_record(
            &stored.record,
            format,
            limits,
            &mut bytes,
            &mut one,
            Layout::Collection,
        ) {
            Ok(()) => {
                export.bytes.extend_from_slice(&bytes);
                xml.push_str(&one);
            }
            Err(failure) => export.failures.push(FailedExport { instance, failure }),
        }
    }
    if xml_format {
        xml.push_str(MARCXML_COLLECTION_FOOTER);
        export.bytes = xml.into_bytes();
    }
    Ok(export)
}

/// One stored record; `None` when the Instance does not exist.
///
/// # Errors
///
/// When storage fails or holds a value this build does not understand.
pub fn stored_record<S: RecordSource + ?Sized>(
    source: &S,
    instance: InstanceId,
) -> Result<Option<StoredRecord>, CatalogReadError> {
    // Records are read in Instance order, so the one after the id just below is this one,
    // when it exists.
    let after = instance.0.checked_sub(1).map(InstanceId);
    Ok(source
        .records_after(after, NonZeroUsize::MIN)?
        .into_iter()
        .find(|stored| stored.instance == instance))
}

fn to_marc(record: &NewRecord, limits: &Limits, out: &mut Vec<u8>) -> Result<(), ExportFailure> {
    let Some(source) = record.source.as_deref() else {
        return bibframe_to_marc(record, None)
            .write_iso2709(out)
            .map_err(ExportFailure::Write);
    };
    let original =
        Record::from_ref(&RecordRef::parse(source, limits).map_err(ExportFailure::Source)?);
    // A record nobody changed goes out as the stored bytes: rewriting it would give the
    // same bytes, except for records the writer would refuse but readers accept (an
    // ISO 2709 delimiter inside a control field, as some LC records have).
    if unchanged(record, &original) {
        out.extend_from_slice(source);
        return Ok(());
    }
    bibframe_to_marc(record, Some(&original))
        .write_iso2709(out)
        .map_err(ExportFailure::Write)
}

/// Whether the model still says what `original` maps to.
fn unchanged(record: &NewRecord, original: &Record) -> bool {
    marc_to_bibframe(original, None).is_ok_and(|mapped| same_description(&mapped, record))
}

/// Every part of two records but their source. Destructured whole, so that a new part
/// cannot be forgotten here.
fn same_description(a: &NewRecord, b: &NewRecord) -> bool {
    let NewRecord {
        work_title,
        work_description,
        hub,
        relations,
        classification,
        classification_scheme,
        summary,
        audience,
        languages,
        contributions,
        subjects,
        instance,
        items,
        source: _,
    } = a;
    *work_title == b.work_title
        && *work_description == b.work_description
        && *hub == b.hub
        && *relations == b.relations
        && *classification == b.classification
        && *classification_scheme == b.classification_scheme
        && *summary == b.summary
        && *audience == b.audience
        && *languages == b.languages
        && *contributions == b.contributions
        && *subjects == b.subjects
        && instance.title == b.instance.title
        && instance.description == b.instance.description
        && instance.edition == b.instance.edition
        && instance.place == b.instance.place
        && instance.publisher == b.instance.publisher
        && instance.date == b.instance.date
        && instance.extent == b.instance.extent
        && instance.series == b.instance.series
        && instance.identifiers == b.instance.identifiers
        && instance.notes.iter().map(|n| (n.kind, &n.text)).eq(b
            .instance
            .notes
            .iter()
            .map(|n| (n.kind, &n.text)))
        && instance.opac_hidden == b.instance.opac_hidden
        && *items == b.items
}

/// The branch and item type codes the Items of a MARC21 file refer to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemCodes {
    /// Home and holding branches.
    pub branches: BTreeSet<String>,
    /// Item types.
    pub item_types: BTreeSet<String>,
}

/// Reads a MARC21 file and collects the codes its Items refer to, so that they can be
/// created before importing it. Records that cannot be read or mapped are passed over:
/// the import reports them.
///
/// # Errors
///
/// When the input cannot be read.
pub fn item_codes<R: BufRead>(input: R, limits: Limits) -> Result<ItemCodes, std::io::Error> {
    let mut codes = ItemCodes::default();
    let mut reader = MarcReader::new(input, limits);
    while let Some(next) = reader.next_record() {
        let parsed = match next {
            Ok(parsed) => parsed,
            Err(elzevir_marc::ReadError::Io(error)) => return Err(error),
            Err(elzevir_marc::ReadError::Record { .. }) => continue,
        };
        let mut record = Record::from_ref(&parsed);
        record.marc21_to_utf8();
        let Ok(mapped) = marc_to_bibframe(&record, None) else {
            continue;
        };
        for item in mapped.items.into_marc() {
            codes.branches.insert(item.home_branch);
            codes.branches.insert(item.holding_branch);
            codes.item_types.insert(item.item_type);
        }
    }
    Ok(codes)
}
