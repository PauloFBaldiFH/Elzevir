// SPDX-License-Identifier: GPL-3.0-or-later
//! The port of authority sources (docs/specs/catalogo/fontes-de-autoridade.md): servers
//! that answer a name of a class with MARC21 authority records, behind the same outcomes
//! as the draft sources.

use elzevir_domain::catalog::SubjectKind;
use elzevir_marc::Record;

use crate::drafts::{DraftField, DraftQueryError, MAX_QUERY_VALUE_CHARS, SourceResult};
use crate::marc_mapping::{AuthorityHeading, authority_heading};

/// A search of authority sources: a name and the class it should have (items 5 and 6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorityQuery {
    name: String,
    kind: SubjectKind,
}

impl AuthorityQuery {
    /// The query of `name`, trimmed, for class `kind`.
    ///
    /// # Errors
    ///
    /// When the name is empty or longer than [`MAX_QUERY_VALUE_CHARS`].
    pub fn new(name: &str, kind: SubjectKind) -> Result<Self, DraftQueryError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(DraftQueryError::Empty);
        }
        if name.chars().count() > MAX_QUERY_VALUE_CHARS {
            return Err(DraftQueryError::TooLong);
        }
        Ok(Self {
            name: name.to_owned(),
            kind,
        })
    }

    /// The name searched.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The class searched.
    #[must_use]
    pub const fn kind(&self) -> SubjectKind {
        self.kind
    }

    /// The search field the class chooses, as Koha's authority search names them
    /// (item 6).
    ///
    /// Koha: `C4::Breeding::_auth_build_query`.
    #[must_use]
    pub const fn field(&self) -> DraftField {
        match self.kind {
            SubjectKind::Person => DraftField::PersonalName,
            SubjectKind::Organization => DraftField::CorporateName,
            SubjectKind::Meeting => DraftField::MeetingName,
            SubjectKind::Family => DraftField::Name,
            SubjectKind::Title => DraftField::UniformTitle,
            SubjectKind::Topic
            | SubjectKind::Place
            | SubjectKind::GenreForm
            | SubjectKind::Temporal => DraftField::Subject,
        }
    }
}

/// An authority record found in a source, with where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorityDraft {
    /// `sru:<server name>` or `z3950:<server name>`.
    pub origin: String,
    /// What the list shows and approval takes.
    pub heading: AuthorityHeading,
}

impl AuthorityDraft {
    /// The draft of `record` from `origin`; `None` when it is not an authority record
    /// with a heading (item 9).
    #[must_use]
    pub fn of(record: &Record, origin: &str) -> Option<Self> {
        authority_heading(record).map(|heading| Self {
            origin: origin.to_owned(),
            heading,
        })
    }
}

/// The authority servers configured for the institution.
pub trait AuthoritySource: Send + Sync {
    /// Asks every enabled authority server for page `page` (from 1) of `query`, in
    /// parallel, in the configured order.
    fn search_authorities(
        &self,
        query: &AuthorityQuery,
        page: u32,
    ) -> Vec<SourceResult<AuthorityDraft>>;

    /// The enabled authority servers' names, in the configured order.
    fn authority_sources(&self) -> Vec<String>;

    /// Asks authority server `index` (from 0) alone; `None` when there is no such server.
    fn search_one_authority(
        &self,
        index: usize,
        query: &AuthorityQuery,
        page: u32,
    ) -> Option<SourceResult<AuthorityDraft>>;
}

#[cfg(test)]
mod tests {
    use elzevir_marc::{Field, Subfield, Tag};

    use super::*;

    fn record(leader06: u8, fields: Vec<Field>) -> Record {
        let mut leader = *b"00000nz  a2200000n  4500";
        if let Some(position) = leader.get_mut(6) {
            *position = leader06;
        }
        Record { leader, fields }
    }

    fn control(tag: [u8; 3], value: &str) -> Field {
        Field::Control {
            tag: Tag::new(tag),
            value: value.as_bytes().to_vec(),
        }
    }

    fn data(tag: [u8; 3], indicator: u8, subfields: &[(u8, &str)]) -> Field {
        Field::Data {
            tag: Tag::new(tag),
            indicators: [indicator, b' '],
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
    fn the_class_chooses_kohas_search_field() {
        let field = |kind| AuthorityQuery::new("x", kind).unwrap().field();
        assert_eq!(field(SubjectKind::Person), DraftField::PersonalName);
        assert_eq!(field(SubjectKind::Family), DraftField::Name);
        assert_eq!(field(SubjectKind::Place), DraftField::Subject);
        assert_eq!(field(SubjectKind::Title), DraftField::UniformTitle);
        assert_eq!(
            AuthorityQuery::new("  ", SubjectKind::Topic),
            Err(DraftQueryError::Empty)
        );
        assert_eq!(
            AuthorityQuery::new(&"a".repeat(257), SubjectKind::Topic),
            Err(DraftQueryError::TooLong)
        );
    }

    #[test]
    fn an_authority_record_gives_its_heading_class_dates_and_control_number() {
        let person = record(
            b'z',
            vec![
                control(*b"001", "n  00000001 "),
                control(*b"003", "DLC"),
                data(*b"100", b'1', &[(b'a', "Lemos, Teodora,"), (b'd', "1950-")]),
                data(*b"400", b'1', &[(b'a', "Lemos, T."), (b'd', "1950-")]),
                data(*b"400", b'1', &[(b'a', "Teodora Lemos")]),
            ],
        );
        let draft = AuthorityDraft::of(&person, "z3950:Exemplo").unwrap();
        assert_eq!(draft.origin, "z3950:Exemplo");
        assert_eq!(
            draft.heading,
            AuthorityHeading {
                kind: SubjectKind::Person,
                heading: "Lemos, Teodora, 1950-".to_owned(),
                dates: Some("1950-".to_owned()),
                identifier: Some("(DLC)n  00000001".to_owned()),
                variants: vec!["Lemos, T. 1950-".to_owned(), "Teodora Lemos".to_owned()],
            }
        );

        let family = record(
            b'z',
            vec![data(*b"100", b'3', &[(b'a', "Lemos (Família)")])],
        );
        let heading = AuthorityDraft::of(&family, "sru:A").unwrap().heading;
        assert_eq!(
            (heading.kind, heading.identifier),
            (SubjectKind::Family, None)
        );

        let topic = record(
            b'z',
            vec![
                control(*b"001", "sh00000002"),
                data(*b"150", b' ', &[(b'a', "Rios"), (b'z', "Paraná")]),
            ],
        );
        let heading = AuthorityDraft::of(&topic, "sru:A").unwrap().heading;
        assert_eq!(heading.kind, SubjectKind::Topic);
        assert_eq!(heading.heading, "Rios -- Paraná");
        assert_eq!(heading.identifier.as_deref(), Some("sh00000002"));
        assert_eq!(heading.dates, None);

        let bibliographic = record(b'a', vec![data(*b"100", b'1', &[(b'a', "Lemos")])]);
        assert_eq!(AuthorityDraft::of(&bibliographic, "sru:A"), None);
        assert_eq!(AuthorityDraft::of(&record(b'z', Vec::new()), "sru:A"), None);
    }
}
