// SPDX-License-Identifier: GPL-3.0-or-later
//! The MARC21 edge of the catalog: `marc_to_bibframe` (docs/specs/catalogo/mapeamento-marc21.md).
//!
//! This is the only place, with the export that will mirror it, that reads MARC tags.
//! Everything it does not map stays in the preserved source record.

mod admin;
mod books;
mod classification;
mod content;
mod description;
mod export;
mod fixed;
mod form;
mod instance;
pub(crate) mod items;
mod locators;
mod national;
mod notes;
mod physical;
mod provision;
mod relations;
mod responsibility;
mod temporal;
mod text;
mod titles;
mod work;

use elzevir_marc::{Field, Limits, Record, RecordRef, Subfield};

use elzevir_domain::statement::{Description, Statement};

use crate::catalog::{NewContribution, NewNote, NewRecord, NewSubject};

pub use export::bibframe_to_marc;

/// Why a MARC21 record could not be mapped.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MappingError {
    /// The record has no title in `245`.
    #[error("the record has no title (245)")]
    MissingTitle,
    /// An item field has no home branch.
    #[error("item field {position} (952) has no home branch ($a)")]
    MissingHomeBranch {
        /// Zero-based position of the `952` among the record's `952` fields.
        position: usize,
    },
    /// An item field has no item type, and the record has no default one.
    #[error("item field {position} (952) has no item type ($y or 942$c)")]
    MissingItemType {
        /// Zero-based position of the `952` among the record's `952` fields.
        position: usize,
    },
}

/// The notes of a stored source record (UTF-8 ISO 2709), for records stored before the
/// model had notes. A record that no longer parses has none.
#[must_use]
pub fn source_notes(source: &[u8]) -> Vec<NewNote> {
    RecordRef::parse(source, &Limits::default())
        .map(|parsed| instance::notes(&Marc(&Record::from_ref(&parsed))))
        .unwrap_or_default()
}

/// The target audience (`521$a`) of a stored source record, for records stored before
/// the model had it.
#[must_use]
pub fn source_audience(source: &[u8]) -> Option<String> {
    let parsed = RecordRef::parse(source, &Limits::default()).ok()?;
    Marc(&Record::from_ref(&parsed)).first_text(b"521", b'a')
}

/// The Work's and the Instance's classes and coded statements of a stored source record,
/// for records stored before the model had them. A record that no longer parses has none.
#[must_use]
pub fn source_description(source: &[u8]) -> Option<(Description, Description)> {
    let parsed = RecordRef::parse(source, &Limits::default()).ok()?;
    let record = Record::from_ref(&parsed);
    let marc = Marc(&record);
    Some((description::work(&marc), description::instance(&marc)))
}

/// The titles other than the preferred one of a stored source record, of the Work and of
/// the Instance, for records stored before the model had them (bibframe-completo.md,
/// item 29, part 5a).
#[must_use]
pub fn source_titles(source: &[u8]) -> Option<(Vec<Statement>, Vec<Statement>)> {
    let parsed = RecordRef::parse(source, &Limits::default()).ok()?;
    let record = Record::from_ref(&parsed);
    let marc = Marc(&record);
    Some((
        titles::of_owner(&marc, titles::Owner::Work),
        titles::of_owner(&marc, titles::Owner::Instance),
    ))
}

/// The typed notes, custodial history and frequency of a stored source record, of the
/// Work and of the Instance, for records stored before the model had them
/// (bibframe-completo.md, item 29, part 5b).
#[must_use]
pub fn source_typed_notes(source: &[u8]) -> Option<(Vec<Statement>, Vec<Statement>)> {
    let parsed = RecordRef::parse(source, &Limits::default()).ok()?;
    let record = Record::from_ref(&parsed);
    let marc = Marc(&record);
    Some((
        notes::of_owner(&marc, titles::Owner::Work),
        notes::of_owner(&marc, titles::Owner::Instance),
    ))
}

/// Production, distribution, manufacture and copyright dates of a stored source record,
/// for records stored before the model had them (bibframe-completo.md, item 29, part 5c).
/// The Work gets none.
#[must_use]
pub fn source_provision(source: &[u8]) -> Option<(Vec<Statement>, Vec<Statement>)> {
    let parsed = RecordRef::parse(source, &Limits::default()).ok()?;
    let record = Record::from_ref(&parsed);
    Some((Vec::new(), provision::of_instance(&Marc(&record))))
}

/// The series and related works of a stored source record, for records stored before the
/// model had them (bibframe-completo.md, item 29, part 5d). The Instance gets none.
#[must_use]
pub fn source_relations(source: &[u8]) -> Option<(Vec<Statement>, Vec<Statement>)> {
    let parsed = RecordRef::parse(source, &Limits::default()).ok()?;
    let record = Record::from_ref(&parsed);
    Some((relations::of_work(&Marc(&record)), Vec::new()))
}

/// The geographic coverage, the languages of the parts and the scripts of the Work and the
/// electronic locations of the Instance of a stored source record, for records stored
/// before the model had them (bibframe-completo.md, item 29, part 5e).
#[must_use]
pub fn source_content(source: &[u8]) -> Option<(Vec<Statement>, Vec<Statement>)> {
    let parsed = RecordRef::parse(source, &Limits::default()).ok()?;
    let record = Record::from_ref(&parsed);
    let marc = Marc(&record);
    Some((content::of_work(&marc), locators::of_instance(&marc)))
}

/// The coded publication of `008` of a stored source record: the original date of a
/// reprint for the Work, the publication and a copyright date for the Instance, for records
/// stored before the model had them (bibframe-completo.md, item 29, part 5e).
#[must_use]
pub fn source_fixed(source: &[u8]) -> Option<(Vec<Statement>, Vec<Statement>)> {
    let parsed = RecordRef::parse(source, &Limits::default()).ok()?;
    let record = Record::from_ref(&parsed);
    let marc = Marc(&record);
    Some((fixed::of_work(&marc), fixed::of_instance(&marc)))
}

/// The statement of responsibility of a stored source record, for records stored before
/// the model had it (bibframe-completo.md, item 29, part 5f). The Work gets none.
#[must_use]
pub fn source_responsibility(source: &[u8]) -> Option<(Vec<Statement>, Vec<Statement>)> {
    let parsed = RecordRef::parse(source, &Limits::default()).ok()?;
    let record = Record::from_ref(&parsed);
    Some((Vec::new(), responsibility::of_instance(&Marc(&record))))
}

/// The contributions of a stored source record, for records stored before the model kept
/// the parts of an agent's heading (bibframe-completo.md, item 29, part 5f).
#[must_use]
pub fn source_contributions(source: &[u8]) -> Option<Vec<NewContribution>> {
    let parsed = RecordRef::parse(source, &Limits::default()).ok()?;
    let record = Record::from_ref(&parsed);
    let marc = Marc(&record);
    Some(
        marc.data_in(&work::CONTRIBUTION_TAGS)
            .filter_map(|(tag, field)| work::contribution_of(*tag, field))
            .collect(),
    )
}

/// The subjects of a stored source record, for records stored before the model kept what
/// their subdivisions name (bibframe-completo.md, item 29, part 5f).
#[must_use]
pub fn source_subjects(source: &[u8]) -> Option<Vec<NewSubject>> {
    let parsed = RecordRef::parse(source, &Limits::default()).ok()?;
    let record = Record::from_ref(&parsed);
    let marc = Marc(&record);
    Some(
        marc.data_in(&work::SUBJECT_TAGS)
            .filter_map(|(tag, field)| work::subject_of(*tag, field))
            .collect(),
    )
}

/// The administrative metadata of a stored source record, for records stored before the
/// model had it (bibframe-completo.md, item 29, part 5e). The Work gets none.
#[must_use]
pub fn source_admin(source: &[u8]) -> Option<(Vec<Statement>, Vec<Statement>)> {
    let parsed = RecordRef::parse(source, &Limits::default()).ok()?;
    let record = Record::from_ref(&parsed);
    Some((Vec::new(), admin::of_instance(&Marc(&record))))
}

/// The classifications of a stored source record, for records stored before the model had
/// them (bibframe-completo.md, item 29, part 5f). The Instance gets none.
#[must_use]
pub fn source_classification(source: &[u8]) -> Option<(Vec<Statement>, Vec<Statement>)> {
    let parsed = RecordRef::parse(source, &Limits::default()).ok()?;
    let record = Record::from_ref(&parsed);
    Some((classification::of_work(&Marc(&record)), Vec::new()))
}

/// The statements of the Work that `008/18-34` of a book codes, of a stored source record,
/// for records stored before the model read them (bibframe-completo.md, item 29, part 5f).
#[must_use]
pub fn source_books(source: &[u8]) -> Option<(Vec<Statement>, Vec<Statement>)> {
    let parsed = RecordRef::parse(source, &Limits::default()).ok()?;
    let record = Record::from_ref(&parsed);
    Some((books::of_work(&Marc(&record)), Vec::new()))
}

/// The time periods of the Work (`045`) and the numbers of national bibliographic agencies
/// of the Instance (`016`) of a stored source record, for records stored before the model
/// read them (bibframe-completo.md, item 29, part 5f).
#[must_use]
pub fn source_periods_and_numbers(source: &[u8]) -> Option<(Vec<Statement>, Vec<Statement>)> {
    let parsed = RecordRef::parse(source, &Limits::default()).ok()?;
    let record = Record::from_ref(&parsed);
    let marc = Marc(&record);
    Some((temporal::of_work(&marc), national::of_instance(&marc)))
}

/// The forms of the Work (`380`) of a stored source record, for records stored before the
/// model read them (bibframe-completo.md, item 29, part 5f).
#[must_use]
pub fn source_forms(source: &[u8]) -> Option<(Vec<Statement>, Vec<Statement>)> {
    let parsed = RecordRef::parse(source, &Limits::default()).ok()?;
    let record = Record::from_ref(&parsed);
    Some((form::of_work(&Marc(&record)), Vec::new()))
}

/// The description conventions Leader/18 of a stored source record names, for records
/// stored before the model read them (bibframe-completo.md, item 29, part 5f).
#[must_use]
pub fn source_leader_conventions(source: &[u8]) -> Option<Statement> {
    let parsed = RecordRef::parse(source, &Limits::default()).ok()?;
    let record = Record::from_ref(&parsed);
    admin::leader_conventions(&Marc(&record))
}

/// Whether a stored source record hides its Instance from the OPAC (`942 $n`), for
/// records stored before the model kept it (docs/specs/catalogo/ocultar-no-opac.md).
#[must_use]
pub fn source_opac_hidden(source: &[u8]) -> bool {
    RecordRef::parse(source, &Limits::default()).is_ok_and(|parsed| {
        let record = Record::from_ref(&parsed);
        instance::map(&Marc(&record)).opac_hidden
    })
}

/// The physical details, dimensions and accompanying material of a stored source record,
/// for records stored before the model had them (bibframe-completo.md, item 29, part 5e).
/// The Work gets none.
#[must_use]
pub fn source_physical(source: &[u8]) -> Option<(Vec<Statement>, Vec<Statement>)> {
    let parsed = RecordRef::parse(source, &Limits::default()).ok()?;
    let record = Record::from_ref(&parsed);
    Some((Vec::new(), physical::of_instance(&Marc(&record))))
}

/// The extent of a stored source record as it was mapped before part 5e (`$a $b $c`) and
/// as it is now (`$a $f $g`), in that order.
#[must_use]
pub fn source_extent(source: &[u8]) -> Option<(Option<String>, Option<String>)> {
    let parsed = RecordRef::parse(source, &Limits::default()).ok()?;
    let record = Record::from_ref(&parsed);
    let field = Marc(&record).first(b"300")?;
    Some((physical::former_extent(field), physical::extent(field)))
}

/// The place and the publisher of a stored source record as they were mapped before part
/// 5c (the first `$a` and `$b`) and as they are now (every `$a` and `$b`), in that order.
#[must_use]
pub fn source_publication(source: &[u8]) -> Option<[(Option<String>, Option<String>); 2]> {
    let parsed = RecordRef::parse(source, &Limits::default()).ok()?;
    let record = Record::from_ref(&parsed);
    let marc = Marc(&record);
    let now = instance::map(&marc);
    let field = instance::publication(&marc)?;
    Some([
        (field.text(b'a'), now.place),
        (field.text(b'b'), now.publisher),
    ])
}

/// The Koha item type of a record: `942 $c`, or else the `$y` of its first `952`. Only
/// the edge translation of types reads it (bibframe-completo.md, item 23e).
#[must_use]
pub fn koha_record_type(record: &Record) -> Option<String> {
    let marc = Marc(record);
    marc.first(b"942")
        .and_then(|field| field.raw(b'c'))
        .or_else(|| marc.data(b"952").find_map(|field| field.raw(b'y')))
}

/// Maps one MARC21 record, already in UTF-8, to a Work, its Instance and its Items.
///
/// `source` is the record as it arrived, kept byte for byte in `source_records`.
///
/// # Errors
///
/// When the record lacks what the model requires: see [`MappingError`].
pub fn marc_to_bibframe(
    record: &Record,
    source: Option<Vec<u8>>,
) -> Result<NewRecord, MappingError> {
    let marc = Marc(record);
    let work = work::map(&marc)?;
    let mut instance = instance::map(&marc);
    instance.description = description::instance(&marc);
    let items = items::map(&marc)?;
    Ok(NewRecord {
        work_title: work.title,
        work_description: description::work(&marc),
        hub: None,
        relations: Vec::new(),
        classification: work.classification,
        classification_scheme: work.classification_scheme,
        summary: work.summary,
        audience: work.audience,
        languages: work.languages,
        contributions: work.contributions,
        subjects: work.subjects,
        instance,
        items: items.into(),
        source,
    })
}

/// Read helpers over a record.
struct Marc<'a>(&'a Record);

/// One data field: indicators and subfields.
#[derive(Clone, Copy)]
struct DataField<'a> {
    indicators: [u8; 2],
    subfields: &'a [Subfield],
}

impl<'a> Marc<'a> {
    fn leader_byte(&self, position: usize) -> u8 {
        self.0.leader.get(position).copied().unwrap_or(b' ')
    }

    fn control(&self, tag: [u8; 3]) -> Option<&'a [u8]> {
        self.0.fields.iter().find_map(|field| match field {
            Field::Control { tag: t, value } if *t.as_bytes() == tag => Some(value.as_slice()),
            _ => None,
        })
    }

    fn data(&self, tag: &'a [u8; 3]) -> impl Iterator<Item = DataField<'a>> + 'a {
        self.0.fields.iter().filter_map(move |field| match field {
            Field::Data {
                tag: t,
                indicators,
                subfields,
            } if t.as_bytes() == tag => Some(DataField {
                indicators: *indicators,
                subfields,
            }),
            _ => None,
        })
    }

    /// Data fields whose tag is in `tags`, in record order, with their tag.
    fn data_in(
        &self,
        tags: &'a [&'a [u8; 3]],
    ) -> impl Iterator<Item = (&'a [u8; 3], DataField<'a>)> + 'a {
        self.0.fields.iter().filter_map(move |field| match field {
            Field::Data {
                tag,
                indicators,
                subfields,
            } => tags
                .iter()
                .find(|wanted| **wanted == tag.as_bytes())
                .map(|wanted| {
                    (
                        *wanted,
                        DataField {
                            indicators: *indicators,
                            subfields,
                        },
                    )
                }),
            Field::Control { .. } => None,
        })
    }

    fn first(&self, tag: &'a [u8; 3]) -> Option<DataField<'a>> {
        self.data(tag).next()
    }

    /// The cleaned first `$code` of the first `tag` field.
    fn first_text(&self, tag: &'a [u8; 3], code: u8) -> Option<String> {
        self.first(tag).and_then(|field| field.text(code))
    }
}

impl<'a> DataField<'a> {
    fn values(self, code: u8) -> impl Iterator<Item = &'a [u8]> {
        self.subfields
            .iter()
            .filter(move |subfield| subfield.code == code)
            .map(|subfield| subfield.value.as_slice())
    }

    /// The cleaned first `$code`, if any.
    fn text(self, code: u8) -> Option<String> {
        self.values(code).find_map(text::clean)
    }

    /// The first `$code` exactly as written, trimmed of spaces.
    fn raw(self, code: u8) -> Option<String> {
        self.values(code).find_map(text::trimmed)
    }

    /// Trimmed values of the listed codes, in field order, joined by a space, punctuation kept.
    fn raw_joined(self, codes: &[u8]) -> Option<String> {
        let parts: Vec<String> = self
            .subfields
            .iter()
            .filter(|subfield| codes.contains(&subfield.code))
            .filter_map(|subfield| text::trimmed(&subfield.value))
            .collect();
        (!parts.is_empty()).then(|| parts.join(" "))
    }

    /// Cleaned values of the listed codes, in field order, joined by `separator`.
    fn joined(self, codes: &[u8], separator: &str) -> Option<String> {
        let parts: Vec<String> = self
            .subfields
            .iter()
            .filter(|subfield| codes.contains(&subfield.code))
            .filter_map(|subfield| text::clean(&subfield.value))
            .collect();
        (!parts.is_empty()).then(|| parts.join(separator))
    }
}
