// SPDX-License-Identifier: GPL-3.0-or-later
//! Draft sources: other catalogues a cataloguer copies a record from (interface §17.1,
//! docs/specs/catalogo/fontes-de-rascunho.md). A draft lives only in the answer; nothing
//! is stored until it is approved.

use elzevir_domain::catalog::IdentifierKind;
use elzevir_marc::{Field, Record, WriteError};

use crate::catalog::{NewRecord, RecordItems};
use crate::marc_mapping::{MappingError, marc_to_bibframe};

/// Most characters of one query value.
pub const MAX_QUERY_VALUE_CHARS: usize = 256;
/// Records in one page of results, as in Koha (`hits_per_page`).
pub const DRAFTS_PER_PAGE: u32 = 20;
/// Last page that may be asked for.
pub const MAX_DRAFT_PAGE: u32 = 50;

/// The fields a draft search fills; at least one has a value.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DraftQuery {
    /// ISBN, without hyphens or spaces.
    pub isbn: Option<String>,
    /// ISSN.
    pub issn: Option<String>,
    /// Title words.
    pub title: Option<String>,
    /// Author words.
    pub author: Option<String>,
    /// Words in any field.
    pub any: Option<String>,
}

/// The fields of a draft search as typed.
#[derive(Debug, Clone, Copy, Default)]
pub struct TypedQuery<'a> {
    /// ISBN.
    pub isbn: &'a str,
    /// ISSN.
    pub issn: &'a str,
    /// Title words.
    pub title: &'a str,
    /// Author words.
    pub author: &'a str,
    /// Words in any field.
    pub any: &'a str,
}

/// Why a draft query cannot be sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DraftQueryError {
    /// Every field is empty.
    #[error("fill in at least one field")]
    Empty,
    /// A value is longer than [`MAX_QUERY_VALUE_CHARS`].
    #[error("a value is longer than {MAX_QUERY_VALUE_CHARS} characters")]
    TooLong,
}

impl DraftQuery {
    /// The query of what was typed: values trimmed, empty ones dropped, the ISBN without
    /// hyphens and spaces (item 7: a wrong check digit is sent as typed).
    ///
    /// # Errors
    ///
    /// When nothing is filled in or a value is too long.
    pub fn new(typed: &TypedQuery<'_>) -> Result<Self, DraftQueryError> {
        let value = |text: &str| -> Result<Option<String>, DraftQueryError> {
            let text = text.trim();
            if text.chars().count() > MAX_QUERY_VALUE_CHARS {
                return Err(DraftQueryError::TooLong);
            }
            Ok((!text.is_empty()).then(|| text.to_owned()))
        };
        let isbn = value(typed.isbn)?
            .map(|isbn| {
                isbn.chars()
                    .filter(|c| *c != '-' && !c.is_whitespace())
                    .collect()
            })
            .filter(|isbn: &String| !isbn.is_empty());
        let query = Self {
            isbn,
            issn: value(typed.issn)?,
            title: value(typed.title)?,
            author: value(typed.author)?,
            any: value(typed.any)?,
        };
        if query.fields().next().is_none() {
            return Err(DraftQueryError::Empty);
        }
        Ok(query)
    }

    /// The filled fields by name, in alphabetical order of the name, as Koha joins them
    /// (`C4::Breeding::_build_query`).
    pub fn fields(&self) -> impl Iterator<Item = (DraftField, &str)> {
        [
            (DraftField::Any, &self.any),
            (DraftField::Author, &self.author),
            (DraftField::Isbn, &self.isbn),
            (DraftField::Issn, &self.issn),
            (DraftField::Title, &self.title),
        ]
        .into_iter()
        .filter_map(|(field, value)| value.as_deref().map(|value| (field, value)))
    }
}

/// One field of a draft query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DraftField {
    /// Any field (Koha `srchany`).
    Any,
    /// Author.
    Author,
    /// ISBN.
    Isbn,
    /// ISSN.
    Issn,
    /// Title.
    Title,
}

impl DraftField {
    /// The field's name in the configuration.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::Author => "author",
            Self::Isbn => "isbn",
            Self::Issn => "issn",
            Self::Title => "title",
        }
    }
}

/// What a list of results shows of a draft.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DraftSummary {
    /// Title proper.
    pub title: String,
    /// The primary contribution's heading, if any.
    pub author: Option<String>,
    /// Edition statement.
    pub edition: Option<String>,
    /// Place, publisher and date, the parts it has.
    pub publication: Option<String>,
    /// ISBNs as recorded.
    pub isbns: Vec<String>,
}

impl DraftSummary {
    /// The summary of a MARC21 record, by the same mapping as the import, without the
    /// source library's `9XX` fields. A record the mapping refuses has no summary.
    #[must_use]
    pub fn of(record: &Record) -> Option<Self> {
        let mapped = marc_to_bibframe(&without_local_fields(record), None).ok()?;
        let instance = mapped.instance;
        let publication = [&instance.place, &instance.publisher, &instance.date]
            .into_iter()
            .flatten()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(" · ");
        Some(Self {
            author: mapped
                .contributions
                .iter()
                .find(|contribution| contribution.primary)
                .or_else(|| mapped.contributions.first())
                .map(|contribution| contribution.heading.clone()),
            edition: instance.edition,
            publication: (!publication.is_empty()).then_some(publication),
            isbns: instance
                .identifiers
                .into_iter()
                .filter(|identifier| identifier.kind == IdentifierKind::Isbn)
                .map(|identifier| identifier.value)
                .collect(),
            title: instance.title,
        })
    }
}

/// A record found in a source, with where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    /// `sru:<server name>`, kept as the source record's origin on approval.
    pub origin: String,
    /// The record, in UTF-8.
    pub record: Record,
    /// What the list shows.
    pub summary: DraftSummary,
}

/// What one server answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceOutcome {
    /// The server's total and the drafts of the page; `skipped` counts records that were
    /// not valid MARC21.
    Found {
        /// Records the server says it has.
        total: u64,
        /// The page's drafts.
        drafts: Vec<Draft>,
        /// Records of the page left out because they could not be read.
        skipped: u32,
    },
    /// No answer within the time limit, or the connection failed.
    NoResponse,
    /// The server answered with a refusal: its diagnostic or the HTTP status.
    Refused(String),
}

/// One server's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceResult {
    /// The server's configured name.
    pub source: String,
    /// What it answered.
    pub outcome: SourceOutcome,
}

/// The sources configured for the institution, asked together.
pub trait DraftSource: Send + Sync {
    /// Asks every enabled server for page `page` (from 1) of `query`, in parallel, and
    /// answers in the configured order once each one answered or ran out of time.
    fn search(&self, query: &DraftQuery, page: u32) -> Vec<SourceResult>;

    /// The enabled servers' names, in the configured order.
    fn sources(&self) -> Vec<String>;

    /// Asks server `index` (from 0, in the configured order) alone, so that a screen can
    /// show each answer as it arrives (estacao-de-captura.md, item 4). `None` when there
    /// is no such server.
    fn search_one(&self, index: usize, query: &DraftQuery, page: u32) -> Option<SourceResult>;
}

/// Why an approved draft cannot become a new record.
#[derive(Debug, thiserror::Error)]
pub enum ApprovalError {
    /// The mapping refuses the record.
    #[error(transparent)]
    Mapping(#[from] MappingError),
    /// The record cannot be written as ISO 2709.
    #[error(transparent)]
    Write(#[from] WriteError),
}

/// The new family an approved draft starts (estacao-de-captura.md, items 11 and 13): the
/// record's mapping without the source library's local `9XX` fields and without Items,
/// with the record, also without `9XX`, as its source and the draft's origin as the
/// Instance's.
///
/// # Errors
///
/// When the mapping refuses the record, or it cannot be written as ISO 2709.
pub fn approved(draft: &Draft) -> Result<NewRecord, ApprovalError> {
    let record = without_local_fields(&draft.record);
    let mut source = Vec::new();
    record.write_iso2709(&mut source)?;
    let mut new = marc_to_bibframe(&record, Some(source))?;
    new.items = RecordItems::default();
    new.instance.origin = Some(draft.origin.clone());
    Ok(new)
}

/// `record` without its `9XX` fields: the holdings, types and record number of the
/// library the draft came from, which describe its collection rather than the book (and
/// whose incomplete `952` would make the mapping refuse the record).
fn without_local_fields(record: &Record) -> Record {
    let mut record = record.clone();
    record
        .fields
        .retain(|field: &Field| field.tag().as_bytes().first() != Some(&b'9'));
    record
}

#[cfg(test)]
mod tests {
    use elzevir_marc::{Subfield, Tag};

    use super::*;

    fn data(tag: [u8; 3], subfields: &[(u8, &str)]) -> Field {
        Field::Data {
            tag: Tag::new(tag),
            indicators: *b"  ",
            subfields: subfields
                .iter()
                .map(|(code, value)| Subfield {
                    code: *code,
                    value: value.as_bytes().to_vec(),
                })
                .collect(),
        }
    }

    #[test]
    fn an_approved_draft_leaves_the_source_librarys_local_fields_behind() {
        let record = Record {
            leader: *b"00000nam a2200000 a 4500",
            fields: vec![
                data(*b"020", &[(b'a', "9780000000002")]),
                data(*b"245", &[(b'a', "O rio das águas claras")]),
                data(*b"942", &[(b'c', "BK")]),
                data(*b"952", &[(b'p', "000123"), (b'y', "BK")]),
                data(*b"999", &[(b'c', "4321")]),
            ],
        };
        let summary = DraftSummary::of(&record).unwrap();
        let draft = Draft {
            origin: "sru:Exemplo".to_owned(),
            record,
            summary,
        };
        let new = approved(&draft).unwrap();
        assert_eq!(new.instance.origin.as_deref(), Some("sru:Exemplo"));
        assert!(new.items.is_empty());
        assert_eq!(new.instance.title, "O rio das águas claras");
        let source = new.source.unwrap();
        let kept =
            elzevir_marc::RecordRef::parse(&source, &elzevir_marc::Limits::default()).unwrap();
        let tags: Vec<_> = kept.fields().map(|field| *field.tag().as_bytes()).collect();
        assert_eq!(tags, [*b"020", *b"245"]);
    }

    #[test]
    fn a_query_keeps_filled_fields_in_koha_order_and_normalises_the_isbn() {
        let query = DraftQuery::new(&TypedQuery {
            isbn: " 978-85-359-0277-1 ",
            title: "Dom Casmurro",
            author: "  ",
            ..TypedQuery::default()
        })
        .unwrap();
        assert_eq!(query.isbn.as_deref(), Some("9788535902771"));
        assert_eq!(
            query.fields().collect::<Vec<_>>(),
            [
                (DraftField::Isbn, "9788535902771"),
                (DraftField::Title, "Dom Casmurro")
            ]
        );
        assert_eq!(
            DraftQuery::new(&TypedQuery {
                isbn: " ",
                ..TypedQuery::default()
            }),
            Err(DraftQueryError::Empty)
        );
        assert_eq!(
            DraftQuery::new(&TypedQuery {
                isbn: "- -",
                ..TypedQuery::default()
            }),
            Err(DraftQueryError::Empty)
        );
        let long = "a".repeat(MAX_QUERY_VALUE_CHARS + 1);
        assert_eq!(
            DraftQuery::new(&TypedQuery {
                title: &long,
                ..TypedQuery::default()
            }),
            Err(DraftQueryError::TooLong)
        );
    }
}
