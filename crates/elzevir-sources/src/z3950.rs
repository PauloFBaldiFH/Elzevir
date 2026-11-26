// SPDX-License-Identifier: GPL-3.0-or-later
//! The Z39.50 messages a draft search needs: Init, Search, Present and Close
//! (docs/specs/catalogo/cliente-z3950.md, items 4 to 13), encoded and read with [`crate::ber`].

use elzevir_app::drafts::{DRAFTS_PER_PAGE, DraftField, DraftQuery};

use crate::ber::{
    self, BerError, Class, EXTERNAL, Element, INTEGER, OBJECT_IDENTIFIER, OCTET_STRING, context,
    context_integer, context_of, sequence,
};

/// The Bib-1 attribute set.
const BIB1: [u32; 6] = [1, 2, 840, 10003, 3, 1];
/// The USMARC (MARC21) record syntax.
pub const USMARC: [u32; 6] = [1, 2, 840, 10003, 5, 10];
/// Longest diagnostic text kept, as in the SRU client.
const MAX_DIAGNOSTIC_CHARS: usize = 300;
/// Message and record size offered in the Init.
const MESSAGE_SIZE: i64 = 8 * 1024 * 1024;

// Tags of the protocol data units.
const INIT_REQUEST: u32 = 20;
const INIT_RESPONSE: u32 = 21;
const SEARCH_REQUEST: u32 = 22;
const SEARCH_RESPONSE: u32 = 23;
const PRESENT_REQUEST: u32 = 24;
const PRESENT_RESPONSE: u32 = 25;
const CLOSE: u32 = 48;

/// A diagnostic from the server: a Bib-1 condition and its additional text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// The condition number.
    pub condition: i64,
    /// What the server added.
    pub addinfo: Option<String>,
}

impl Diagnostic {
    /// `<condition>: <addinfo>`, or the condition alone, at most 300 characters (item 13).
    #[must_use]
    pub fn message(&self) -> String {
        let text = match &self.addinfo {
            Some(addinfo) if !addinfo.trim().is_empty() => {
                format!("{}: {}", self.condition, addinfo.trim())
            }
            _ => self.condition.to_string(),
        };
        text.chars().take(MAX_DIAGNOSTIC_CHARS).collect()
    }
}

/// One record of a Present response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresentRecord {
    /// A MARC21 record, as the ISO 2709 bytes the server sent.
    Marc(Vec<u8>),
    /// A diagnostic in its place, or a record in another syntax (item 10).
    Unusable,
}

/// A message from the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pdu {
    /// The answer to the Init (item 5).
    InitResponse {
        /// Whether the server accepted.
        accepted: bool,
    },
    /// The answer to the Search (item 8).
    SearchResponse {
        /// Records found.
        total: u64,
        /// Whether the search succeeded.
        succeeded: bool,
        /// The server's diagnostic, if any.
        diagnostic: Option<Diagnostic>,
    },
    /// The answer to the Present (item 10).
    PresentResponse {
        /// The records, in order.
        records: Vec<PresentRecord>,
        /// A diagnostic in place of every record.
        diagnostic: Option<Diagnostic>,
    },
    /// The server closed the association, with its reason.
    Close {
        /// The close reason.
        reason: i64,
    },
    /// A message this client does not ask for, by its tag.
    Other(u32),
}

/// The Init (item 4), naming `version` as this implementation's version.
#[must_use]
pub fn init_request(version: &str) -> Vec<u8> {
    context_of(
        INIT_REQUEST,
        &[
            // Versions 1, 2 and 3 (bits 0 to 2); five unused bits.
            context(3, &[0x05, 0xE0]),
            // Options search and present (bits 0 and 1).
            context(4, &[0x06, 0xC0]),
            context_integer(5, MESSAGE_SIZE),
            context_integer(6, MESSAGE_SIZE),
            context(110, b"Elzevir"),
            context(111, b"Elzevir"),
            context(112, version.as_bytes()),
        ],
    )
}

/// The Search of `query` in `databases` (items 6 and 7).
#[must_use]
pub fn search_request(databases: &[String], query: &DraftQuery) -> Vec<u8> {
    search_request_of(databases, &query.fields().collect::<Vec<_>>())
}

/// The Search of the filled `fields` in `databases`, each with Koha's Bib-1 attributes,
/// in Koha's order (items 6 and 7; fontes-de-autoridade.md, items 6 and 7).
#[must_use]
pub fn search_request_of(databases: &[String], fields: &[(DraftField, &str)]) -> Vec<u8> {
    let names: Vec<Vec<u8>> = databases
        .iter()
        .map(|name| context(105, name.as_bytes()))
        .collect();
    context_of(
        SEARCH_REQUEST,
        &[
            context_integer(13, 0),
            context_integer(14, 1),
            context_integer(15, 0),
            context(16, &[0xFF]),
            context(17, b"default"),
            context_of(18, &names),
            context_of(21, &[context_of(1, &[ber::oid(&BIB1), rpn(fields)])]),
        ],
    )
}

/// The Type-1 terms of `fields`, in alphabetical order of Koha's names (item 7): for a
/// draft search `author`, `isbn`, `issn`, `srchany` (the Elzevir's `any`) and `title`,
/// each with its Bib-1 attributes.
///
/// Koha: `C4::Breeding::_bib_build_query` and `_auth_build_query`.
fn terms<'a>(fields: &[(DraftField, &'a str)]) -> Vec<(&'static [(i64, i64)], &'a str)> {
    let mut terms: Vec<_> = fields
        .iter()
        .map(|(field, value)| {
            let (name, attributes) = attributes(*field);
            (name, attributes, *value)
        })
        .collect();
    terms.sort_by_key(|(name, _, _)| *name);
    terms
        .into_iter()
        .map(|(_, attributes, value)| (attributes, value))
        .collect()
}

/// Koha's name of `field` and its Bib-1 attributes.
const fn attributes(field: DraftField) -> (&'static str, &'static [(i64, i64)]) {
    match field {
        DraftField::Author => ("author", &[(1, 1003)]),
        DraftField::Isbn => ("isbn", &[(1, 7), (5, 1)]),
        DraftField::Issn => ("issn", &[(1, 8), (5, 1)]),
        DraftField::Any => ("srchany", &[(1, 1016)]),
        DraftField::Title => ("title", &[(1, 4)]),
        DraftField::PersonalName => ("authorpersonal", &[(1, 1)]),
        DraftField::CorporateName => ("authorcorp", &[(1, 2)]),
        DraftField::MeetingName => ("authormeetingcon", &[(1, 3)]),
        DraftField::Name => ("nameany", &[(1, 1002)]),
        DraftField::Subject => ("subject", &[(1, 21)]),
        DraftField::UniformTitle => ("uniformtitle", &[(1, 6)]),
    }
}

/// The RPN structure: the terms joined by `and` from the left, like Koha's
/// `@and @and t1 t2 t3` (item 7.3).
fn rpn(fields: &[(DraftField, &str)]) -> Vec<u8> {
    let mut terms = terms(fields).into_iter().map(|(attributes, value)| {
        let attributes: Vec<Vec<u8>> = attributes
            .iter()
            .map(|(kind, value)| {
                sequence(&[context_integer(120, *kind), context_integer(121, *value)])
            })
            .collect();
        // op [0] EXPLICIT Operand, the operand an attrTerm [102].
        context_of(
            0,
            &[context_of(
                102,
                &[context_of(44, &attributes), context(45, value.as_bytes())],
            )],
        )
    });
    // A query has at least one filled field; an empty one asks for nothing.
    let Some(mut tree) = terms.next() else {
        return context_of(
            0,
            &[context_of(102, &[context_of(44, &[]), context(45, b"")])],
        );
    };
    for term in terms {
        // rpnRpnOp [1] { rpn1, rpn2, [46] { and [0] NULL } }.
        tree = context_of(1, &[tree, term, context_of(46, &[context(0, &[])])]);
    }
    tree
}

/// The Present of page `page` (from 1): up to 20 records from `(page - 1) × 20 + 1`,
/// never past `total` (item 9). `None` when the page starts after the last record.
#[must_use]
pub fn present_request(page: u32, total: u64) -> Option<Vec<u8>> {
    let start = u64::from(page.max(1) - 1) * u64::from(DRAFTS_PER_PAGE) + 1;
    if start > total {
        return None;
    }
    let count = (total - start + 1).min(u64::from(DRAFTS_PER_PAGE));
    Some(context_of(
        PRESENT_REQUEST,
        &[
            context(31, b"default"),
            context_integer(30, i64::try_from(start).unwrap_or(i64::MAX)),
            context_integer(29, i64::try_from(count).unwrap_or(0)),
            // simple [19] ElementSetNames, the generic name F (full).
            context_of(19, &[context(0, b"F")]),
            context(104, &ber::oid_content(&USMARC)),
        ],
    ))
}

/// The Close, with reason `finished` (item 11).
#[must_use]
pub fn close_request() -> Vec<u8> {
    context_of(CLOSE, &[context_integer(211, 0)])
}

/// Reads one message from the server.
///
/// # Errors
///
/// When the bytes are not one whole BER element, or a message this client reads is
/// malformed.
pub fn parse(bytes: &[u8]) -> Result<Pdu, BerError> {
    let (pdu, _) = ber::read(bytes)?;
    if !matches!(pdu.class, Class::Context) || !pdu.constructed {
        return Err(BerError::Malformed);
    }
    let fields = pdu.children()?;
    match pdu.number {
        INIT_RESPONSE => Ok(Pdu::InitResponse {
            accepted: field(&fields, 12).map_or(Ok(false), Element::boolean)?,
        }),
        SEARCH_RESPONSE => Ok(Pdu::SearchResponse {
            total: field(&fields, 23)
                .map_or(Ok(0), Element::integer)
                .map(|count| u64::try_from(count).unwrap_or(0))?,
            succeeded: field(&fields, 22).map_or(Ok(false), Element::boolean)?,
            diagnostic: records_diagnostic(&fields)?,
        }),
        PRESENT_RESPONSE => Ok(Pdu::PresentResponse {
            records: match field(&fields, 28) {
                Some(records) => records
                    .children()?
                    .iter()
                    .map(present_record)
                    .collect::<Result<_, _>>()?,
                None => Vec::new(),
            },
            diagnostic: records_diagnostic(&fields)?,
        }),
        CLOSE => Ok(Pdu::Close {
            reason: field(&fields, 211).map_or(Ok(0), Element::integer)?,
        }),
        other => Ok(Pdu::Other(other)),
    }
}

/// The first context-specific field with `number`; unknown fields are left alone (item 3).
fn field<'a, 'b>(fields: &'b [Element<'a>], number: u32) -> Option<&'b Element<'a>> {
    fields.iter().find(|field| field.is_context(number))
}

/// The diagnostic that replaces the records: `nonSurrogateDiagnostic [130]` or the first
/// of `multipleNonSurDiagnostics [205]`.
fn records_diagnostic(fields: &[Element<'_>]) -> Result<Option<Diagnostic>, BerError> {
    if let Some(single) = field(fields, 130) {
        return default_diagnostic(single).map(Some);
    }
    if let Some(many) = field(fields, 205) {
        return many
            .children()?
            .first()
            .map(diagnostic_record)
            .transpose()
            .map(Option::flatten);
    }
    Ok(None)
}

/// A `DiagRec`: the default format, or `None` for an externally defined one.
fn diagnostic_record(record: &Element<'_>) -> Result<Option<Diagnostic>, BerError> {
    if record.is_universal(ber::SEQUENCE) {
        default_diagnostic(record).map(Some)
    } else {
        Ok(None)
    }
}

/// `DefaultDiagFormat ::= SEQUENCE { diagnosticSetId OID, condition INTEGER, addinfo }`.
fn default_diagnostic(format: &Element<'_>) -> Result<Diagnostic, BerError> {
    let parts = format.children()?;
    let condition = parts
        .iter()
        .find(|part| part.is_universal(INTEGER))
        .ok_or(BerError::Malformed)?
        .integer()?;
    // VisibleString (26) in version 2, GeneralString (27) in version 3.
    let addinfo = parts
        .iter()
        .find(|part| part.is_universal(26) || part.is_universal(27))
        .map(Element::text)
        .transpose()?;
    Ok(Diagnostic { condition, addinfo })
}

/// One `NamePlusRecord`: `record [1] { retrievalRecord [1] EXTERNAL | surrogateDiagnostic
/// [2] ... }`. Only a USMARC record carried as octets is usable.
fn present_record(name_plus_record: &Element<'_>) -> Result<PresentRecord, BerError> {
    let parts = name_plus_record.children()?;
    let Some(record) = field(&parts, 1) else {
        return Ok(PresentRecord::Unusable);
    };
    let choice = record.inner()?;
    if !choice.is_context(1) {
        return Ok(PresentRecord::Unusable);
    }
    let external = choice.inner()?;
    if !external.is_universal(EXTERNAL) {
        return Ok(PresentRecord::Unusable);
    }
    let parts = external.children()?;
    let syntax = parts
        .iter()
        .find(|part| part.is_universal(OBJECT_IDENTIFIER))
        .map(Element::oid)
        .transpose()?;
    let octets = parts
        .iter()
        .find(|part| part.is_context(1) || part.is_universal(OCTET_STRING));
    match (syntax.as_deref(), octets) {
        (Some(syntax), Some(octets)) if syntax == USMARC && !octets.constructed => {
            Ok(PresentRecord::Marc(octets.content.to_vec()))
        }
        _ => Ok(PresentRecord::Unusable),
    }
}

#[cfg(test)]
mod tests {
    use elzevir_app::drafts::TypedQuery;

    use super::*;
    use crate::ber::element;

    fn query(isbn: &str, title: &str, author: &str) -> DraftQuery {
        DraftQuery::new(&TypedQuery {
            isbn,
            title,
            author,
            ..TypedQuery::default()
        })
        .unwrap()
    }

    /// A context-specific `[number] IMPLICIT BOOLEAN`.
    fn boolean(number: u32, value: bool) -> Vec<u8> {
        context(number, &[if value { 0xFF } else { 0x00 }])
    }

    /// The parts of a `DefaultDiagFormat`, to wrap as a `SEQUENCE` or an implicit tag.
    fn diagnostic_parts(condition: i64, addinfo: &str) -> Vec<u8> {
        [
            ber::oid(&[1, 2, 840, 10003, 4, 1]),
            element(
                Class::Universal,
                false,
                INTEGER,
                &ber::integer_content(condition),
            ),
            element(Class::Universal, false, 26, addinfo.as_bytes()),
        ]
        .concat()
    }

    fn diagnostic(condition: i64, addinfo: &str) -> Vec<u8> {
        element(
            Class::Universal,
            true,
            ber::SEQUENCE,
            &diagnostic_parts(condition, addinfo),
        )
    }

    fn marc_record(syntax: &[u32], marc: &[u8]) -> Vec<u8> {
        let external = element(
            Class::Universal,
            true,
            EXTERNAL,
            &[ber::oid(syntax), context(1, marc)].concat(),
        );
        sequence(&[
            context(0, b"catalogo"),
            context_of(1, &[context_of(1, &[external])]),
        ])
    }

    #[test]
    fn a_single_term_is_an_operand_with_kohas_attributes() {
        let encoded = search_request(
            &["catalogo".to_owned()],
            &query("978-0-00-000000-2", "", ""),
        );
        let (pdu, rest) = ber::read(&encoded).unwrap();
        assert!(rest.is_empty());
        assert!(pdu.is_context(SEARCH_REQUEST));
        let fields = pdu.children().unwrap();
        let names = field(&fields, 18).unwrap().children().unwrap();
        assert_eq!(names.first().unwrap().content, b"catalogo");
        let rpn_query = field(&fields, 21).unwrap().inner().unwrap();
        assert!(rpn_query.is_context(1));
        let parts = rpn_query.children().unwrap();
        assert_eq!(parts.first().unwrap().oid().unwrap(), BIB1);
        let operand = parts.get(1).unwrap();
        assert!(operand.is_context(0));
        let term = operand.inner().unwrap();
        assert!(term.is_context(102));
        let term_parts = term.children().unwrap();
        let attributes: Vec<(i64, i64)> = term_parts
            .first()
            .unwrap()
            .children()
            .unwrap()
            .iter()
            .map(|attribute| {
                let pair = attribute.children().unwrap();
                (
                    pair.first().unwrap().integer().unwrap(),
                    pair.get(1).unwrap().integer().unwrap(),
                )
            })
            .collect();
        assert_eq!(attributes, [(1, 7), (5, 1)]);
        assert_eq!(term_parts.get(1).unwrap().content, b"9780000000002");
    }

    #[test]
    fn several_terms_are_joined_by_and_from_the_left_in_kohas_order() {
        let encoded = search_request(&["db".to_owned()], &query("9780000000002", "rio", "Lemos"));
        let fields = ber::read(&encoded).unwrap().0.children().unwrap();
        let rpn_query = field(&fields, 21).unwrap().inner().unwrap();
        let top = rpn_query.children().unwrap().get(1).copied().unwrap();
        // ((author and isbn) and title).
        assert!(top.is_context(1));
        let top_parts = top.children().unwrap();
        let left = top_parts.first().unwrap();
        assert!(left.is_context(1));
        let value = |operand: &Element<'_>| -> Vec<u8> {
            operand
                .inner()
                .unwrap()
                .children()
                .unwrap()
                .get(1)
                .unwrap()
                .content
                .to_vec()
        };
        let left_parts = left.children().unwrap();
        assert_eq!(value(left_parts.first().unwrap()), b"Lemos");
        assert_eq!(value(left_parts.get(1).unwrap()), b"9780000000002");
        assert_eq!(value(top_parts.get(1).unwrap()), b"rio");
        let operator = top_parts.get(2).unwrap();
        assert!(operator.is_context(46));
        assert!(operator.inner().unwrap().is_context(0));
    }

    #[test]
    fn the_present_asks_for_the_page_within_the_total() {
        let fields = |encoded: Vec<u8>| -> (i64, i64) {
            let fields = ber::read(&encoded).unwrap().0.children().unwrap();
            (
                field(&fields, 30).unwrap().integer().unwrap(),
                field(&fields, 29).unwrap().integer().unwrap(),
            )
        };
        assert_eq!(fields(present_request(1, 37).unwrap()), (1, 20));
        assert_eq!(fields(present_request(2, 37).unwrap()), (21, 17));
        assert_eq!(present_request(3, 37), None);
        assert_eq!(present_request(1, 0), None);
        let encoded = present_request(1, 1).unwrap();
        let all = ber::read(&encoded).unwrap().0.children().unwrap();
        assert_eq!(field(&all, 19).unwrap().inner().unwrap().content, b"F");
        assert_eq!(field(&all, 104).unwrap().content, ber::oid_content(&USMARC));
    }

    #[test]
    fn server_messages_are_read_with_their_diagnostics_and_records() {
        let init = context_of(
            INIT_RESPONSE,
            &[context(3, &[0x05, 0xE0]), boolean(12, true)],
        );
        assert_eq!(parse(&init).unwrap(), Pdu::InitResponse { accepted: true });

        let found = context_of(
            SEARCH_RESPONSE,
            &[
                context_integer(23, 37),
                context_integer(24, 0),
                boolean(22, true),
            ],
        );
        assert_eq!(
            parse(&found).unwrap(),
            Pdu::SearchResponse {
                total: 37,
                succeeded: true,
                diagnostic: None
            }
        );
        let refused = context_of(
            SEARCH_RESPONSE,
            &[
                context_integer(23, 0),
                boolean(22, false),
                element(Class::Context, true, 130, &diagnostic_parts(114, "1=7")),
            ],
        );
        let Pdu::SearchResponse {
            diagnostic: refusal,
            ..
        } = parse(&refused).unwrap()
        else {
            panic!("not a search response");
        };
        assert_eq!(refusal.unwrap().message(), "114: 1=7");

        let surrogate = sequence(&[context_of(1, &[context_of(2, &[diagnostic(14, "")])])]);
        let present = context_of(
            PRESENT_RESPONSE,
            &[
                context_integer(24, 3),
                context_of(
                    28,
                    &[
                        marc_record(&USMARC, b"00026nam a2200025 a 4500\x1e\x1d"),
                        marc_record(&[1, 2, 840, 10003, 5, 109, 10], b"<record/>"),
                        surrogate,
                    ],
                ),
            ],
        );
        assert_eq!(
            parse(&present).unwrap(),
            Pdu::PresentResponse {
                records: vec![
                    PresentRecord::Marc(b"00026nam a2200025 a 4500\x1e\x1d".to_vec()),
                    PresentRecord::Unusable,
                    PresentRecord::Unusable,
                ],
                diagnostic: None,
            }
        );

        let close = context_of(CLOSE, &[context_integer(211, 2)]);
        assert_eq!(parse(&close).unwrap(), Pdu::Close { reason: 2 });
        let other = context_of(36, &[]);
        assert_eq!(parse(&other).unwrap(), Pdu::Other(36));
        assert_eq!(parse(&[0x02, 0x01, 0x00]), Err(BerError::Malformed));
    }

    #[test]
    fn a_long_diagnostic_is_cut() {
        let long = Diagnostic {
            condition: 2,
            addinfo: Some("x".repeat(400)),
        };
        assert_eq!(long.message().chars().count(), 300);
        let bare = Diagnostic {
            condition: 2,
            addinfo: Some("  ".to_owned()),
        };
        assert_eq!(bare.message(), "2");
    }

    #[test]
    fn the_init_and_close_carry_what_the_spec_names() {
        let encoded = init_request("0.1.0");
        let fields = ber::read(&encoded).unwrap().0.children().unwrap();
        assert_eq!(field(&fields, 3).unwrap().content, [0x05, 0xE0]);
        assert_eq!(field(&fields, 4).unwrap().content, [0x06, 0xC0]);
        assert_eq!(field(&fields, 5).unwrap().integer().unwrap(), MESSAGE_SIZE);
        assert_eq!(field(&fields, 111).unwrap().content, b"Elzevir");
        assert_eq!(field(&fields, 112).unwrap().content, b"0.1.0");
        let close = close_request();
        let fields = ber::read(&close).unwrap().0.children().unwrap();
        assert_eq!(field(&fields, 211).unwrap().integer().unwrap(), 0);
    }
}
