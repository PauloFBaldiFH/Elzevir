// SPDX-License-Identifier: GPL-3.0-or-later
//! The requests a Z39.50 client sends to the server (docs/specs/opac/z3950.md, items 4
//! to 18), read with [`crate::ber`], and the RPN query turned into a query by field.
//!
//! Koha: `Koha::Z3950Responder::RPN` and `etc/z3950/attribute_mappings.yaml`.

use elzevir_app::field_search::{FieldQuery, FieldTerm, SearchField, Truncation, WordMatch};

use crate::ber::{self, BerError, Class, Element, OBJECT_IDENTIFIER};
use crate::z3950::Diagnostic;

/// The Bib-1 attribute set.
pub(super) const BIB1: [u32; 6] = [1, 2, 840, 10003, 3, 1];

/// Most terms in one query (item 7).
const MAX_TERMS: usize = 64;

/// A request from the client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Request {
    Init {
        reference: Option<Vec<u8>>,
        /// The content of the version bit string, its unused-bits byte first.
        versions: Vec<u8>,
        /// The content of the options bit string, its unused-bits byte first.
        options: Vec<u8>,
        message_size: Option<i64>,
        record_size: Option<i64>,
    },
    Search {
        reference: Option<Vec<u8>>,
        set: String,
        databases: Vec<String>,
        query: Result<FieldQuery, Diagnostic>,
    },
    Present {
        reference: Option<Vec<u8>>,
        set: String,
        start: i64,
        count: i64,
        elements: Option<String>,
        syntax: Option<Vec<u32>>,
    },
    Close {
        reference: Option<Vec<u8>>,
    },
    /// Another operation, by its tag.
    Other(u32),
}

pub(super) const INIT_REQUEST: u32 = 20;
pub(super) const INIT_RESPONSE: u32 = 21;
pub(super) const SEARCH_REQUEST: u32 = 22;
pub(super) const SEARCH_RESPONSE: u32 = 23;
pub(super) const PRESENT_REQUEST: u32 = 24;
pub(super) const PRESENT_RESPONSE: u32 = 25;
pub(super) const CLOSE: u32 = 48;

/// Reads one request.
///
/// # Errors
///
/// When the bytes are not one whole BER element, or a request is malformed outside its
/// query (a malformed query is a diagnostic, not an error).
pub(super) fn parse(bytes: &[u8]) -> Result<Request, BerError> {
    let (pdu, _) = ber::read(bytes)?;
    if !matches!(pdu.class, Class::Context) || !pdu.constructed {
        return Err(BerError::Malformed);
    }
    let fields = pdu.children()?;
    let reference = field(&fields, 2).map(|reference| reference.content.to_vec());
    let text = |number| field(&fields, number).map(Element::text).transpose();
    let integer = |number| field(&fields, number).map(Element::integer).transpose();
    match pdu.number {
        INIT_REQUEST => Ok(Request::Init {
            reference,
            versions: field(&fields, 3)
                .map(|bits| bits.content.to_vec())
                .unwrap_or_default(),
            options: field(&fields, 4)
                .map(|bits| bits.content.to_vec())
                .unwrap_or_default(),
            message_size: integer(5)?,
            record_size: integer(6)?,
        }),
        SEARCH_REQUEST => Ok(Request::Search {
            reference,
            set: text(17)?.unwrap_or_else(|| "default".to_owned()),
            databases: match field(&fields, 18) {
                Some(names) => names
                    .children()?
                    .iter()
                    .map(Element::text)
                    .collect::<Result<_, _>>()?,
                None => Vec::new(),
            },
            query: field(&fields, 21).map_or_else(
                || Err(Diagnostic::bare(108)),
                |query| {
                    query
                        .inner()
                        .map_or_else(|_| Err(Diagnostic::bare(108)), |choice| query_of(&choice))
                },
            ),
        }),
        PRESENT_REQUEST => Ok(Request::Present {
            reference,
            set: text(31)?.unwrap_or_else(|| "default".to_owned()),
            start: integer(30)?.unwrap_or(1),
            count: integer(29)?.unwrap_or(0),
            elements: match field(&fields, 19) {
                Some(simple) => {
                    let name = simple.inner()?;
                    name.is_context(0).then(|| name.text()).transpose()?
                }
                None => None,
            },
            syntax: field(&fields, 104).map(Element::oid).transpose()?,
        }),
        CLOSE => Ok(Request::Close { reference }),
        other => Ok(Request::Other(other)),
    }
}

/// The first context-specific field with `number`; unknown fields are left alone.
fn field<'a, 'b>(fields: &'b [Element<'a>], number: u32) -> Option<&'b Element<'a>> {
    fields.iter().find(|field| field.is_context(number))
}

/// The OID as dotted numbers, for a diagnostic.
pub(super) fn dotted(oid: &[u32]) -> String {
    oid.iter().map(u32::to_string).collect::<Vec<_>>().join(".")
}

impl Diagnostic {
    pub(super) const fn bare(condition: i64) -> Self {
        Self {
            condition,
            addinfo: None,
        }
    }

    pub(super) fn with(condition: i64, addinfo: &str) -> Self {
        Self {
            condition,
            addinfo: Some(addinfo.to_owned()),
        }
    }
}

/// The query of a `Query` choice (item 7): Type-1 or Type-101, with Bib-1.
fn query_of(choice: &Element<'_>) -> Result<FieldQuery, Diagnostic> {
    if !(choice.is_context(1) || choice.is_context(101)) {
        return Err(Diagnostic::bare(107));
    }
    let malformed = |_| Diagnostic::bare(108);
    let parts = choice.children().map_err(malformed)?;
    let set = parts
        .iter()
        .find(|part| part.is_universal(OBJECT_IDENTIFIER))
        .ok_or_else(|| Diagnostic::bare(108))?
        .oid()
        .map_err(malformed)?;
    if set != BIB1 {
        return Err(Diagnostic::with(121, &dotted(&set)));
    }
    let structure = parts
        .iter()
        .find(|part| part.is_context(0) || part.is_context(1))
        .ok_or_else(|| Diagnostic::bare(108))?;
    let mut terms = 0;
    rpn(structure, &mut terms)
}

/// An `RPNStructure`: an operand, or two structures and an operator.
fn rpn(structure: &Element<'_>, terms: &mut usize) -> Result<FieldQuery, Diagnostic> {
    let malformed = |_| Diagnostic::bare(108);
    if structure.is_context(0) {
        let operand = structure.inner().map_err(malformed)?;
        if !operand.is_context(102) {
            return Err(Diagnostic::bare(3));
        }
        *terms += 1;
        if *terms > MAX_TERMS {
            return Err(Diagnostic::bare(108));
        }
        return term(&operand).map(FieldQuery::Term);
    }
    if !structure.is_context(1) {
        return Err(Diagnostic::bare(108));
    }
    let parts = structure.children().map_err(malformed)?;
    let (Some(left), Some(right), Some(operator)) = (parts.first(), parts.get(1), parts.get(2))
    else {
        return Err(Diagnostic::bare(108));
    };
    let operator = operator.inner().map_err(malformed)?;
    let (left, right) = (Box::new(rpn(left, terms)?), Box::new(rpn(right, terms)?));
    match operator.number {
        0 => Ok(FieldQuery::And(left, right)),
        1 => Ok(FieldQuery::Or(left, right)),
        2 => Ok(FieldQuery::Not(left, right)),
        3 => Err(Diagnostic::with(110, "prox")),
        _ => Err(Diagnostic::bare(108)),
    }
}

/// An `AttributesPlusTerm`: the use and truncation attributes and the term (items 8 to
/// 10).
fn term(operand: &Element<'_>) -> Result<FieldTerm, Diagnostic> {
    let malformed = |_| Diagnostic::bare(108);
    let parts = operand.children().map_err(malformed)?;
    let mut field = SearchField::Any;
    let mut truncation = Truncation::None;
    if let Some(list) = parts.iter().find(|part| part.is_context(44)) {
        for attribute in list.children().map_err(malformed)? {
            let pair = attribute.children().map_err(malformed)?;
            let kind = pair.iter().find(|part| part.is_context(120));
            // A complex value [224] is accepted and ignored, as every attribute but
            // use and truncation.
            let value = pair.iter().find(|part| part.is_context(121));
            let (Some(kind), Some(value)) = (kind, value) else {
                continue;
            };
            let (kind, value) = (
                kind.integer().map_err(malformed)?,
                value.integer().map_err(malformed)?,
            );
            match kind {
                1 => field = use_field(value),
                5 => {
                    truncation = match value {
                        1 => Truncation::Right,
                        2 => Truncation::Left,
                        3 => Truncation::Both,
                        _ => Truncation::None,
                    };
                }
                _ => {}
            }
        }
    }
    let value = parts
        .iter()
        .find(|part| !part.is_context(44))
        .ok_or_else(|| Diagnostic::bare(108))?;
    let text = match value.number {
        45 | 216 if !value.constructed => String::from_utf8_lossy(value.content).into_owned(),
        215 => value.integer().map_err(malformed)?.to_string(),
        _ => return Err(Diagnostic::bare(3)),
    };
    Ok(FieldTerm {
        field,
        matching: WordMatch::All,
        text,
        truncation,
    })
}

/// The field of a Bib-1 use attribute (item 9): Koha's mapping where the index has the
/// field, and everything otherwise, as Koha does with a use it does not map.
const fn use_field(value: i64) -> SearchField {
    match value {
        1 | 2 | 3 | 1003 => SearchField::Contributor,
        4 => SearchField::Title,
        21 => SearchField::Subject,
        1018 => SearchField::Publisher,
        7 | 8 | 9 | 1007 => SearchField::Identifier,
        12 => SearchField::Record,
        20 => SearchField::CallNumber,
        30 | 31 => SearchField::Year,
        1028 => SearchField::Barcode,
        _ => SearchField::Any,
    }
}

#[cfg(test)]
mod tests {
    use elzevir_app::drafts::{DraftQuery, TypedQuery};

    use super::*;
    use crate::ber::{context, context_integer, context_of, sequence};
    use crate::z3950;

    fn attributes_plus_term(attributes: &[(i64, i64)], value: &[u8]) -> Vec<u8> {
        let attributes: Vec<Vec<u8>> = attributes
            .iter()
            .map(|(kind, value)| {
                sequence(&[context_integer(120, *kind), context_integer(121, *value)])
            })
            .collect();
        context_of(
            0,
            &[context_of(
                102,
                &[context_of(44, &attributes), context(45, value)],
            )],
        )
    }

    fn search(query_choice: Vec<u8>) -> Result<FieldQuery, Diagnostic> {
        let encoded = context_of(
            SEARCH_REQUEST,
            &[
                context(17, b"one"),
                context_of(18, &[context(105, b"biblios")]),
                context_of(21, &[query_choice]),
            ],
        );
        let Request::Search {
            set,
            databases,
            query,
            ..
        } = parse(&encoded).unwrap()
        else {
            panic!("not a search");
        };
        assert_eq!(
            (set.as_str(), databases.as_slice()),
            ("one", ["biblios".to_owned()].as_slice())
        );
        query
    }

    fn type1(set: &[u32], structure: Vec<u8>) -> Vec<u8> {
        context_of(1, &[ber::oid(set), structure])
    }

    fn term_of(field: SearchField, text: &str, truncation: Truncation) -> FieldQuery {
        FieldQuery::Term(FieldTerm {
            field,
            matching: WordMatch::All,
            text: text.to_owned(),
            truncation,
        })
    }

    #[test]
    fn the_elzevir_client_query_reads_back() {
        let query = DraftQuery::new(&TypedQuery {
            isbn: "978-0-00-000000-2",
            title: "rio",
            author: "Lemos",
            ..TypedQuery::default()
        })
        .unwrap();
        let encoded = z3950::search_request(&["biblios".to_owned()], &query);
        let Request::Search { set, query, .. } = parse(&encoded).unwrap() else {
            panic!("not a search");
        };
        assert_eq!(set, "default");
        assert_eq!(
            query.unwrap(),
            FieldQuery::And(
                Box::new(FieldQuery::And(
                    Box::new(term_of(SearchField::Contributor, "Lemos", Truncation::None)),
                    Box::new(term_of(
                        SearchField::Identifier,
                        "9780000000002",
                        Truncation::Right
                    )),
                )),
                Box::new(term_of(SearchField::Title, "rio", Truncation::None)),
            )
        );
    }

    #[test]
    fn operators_attributes_and_their_diagnostics() {
        let or_not = context_of(
            1,
            &[
                attributes_plus_term(&[(1, 20), (5, 3), (2, 3)], b"310"),
                context_of(
                    1,
                    &[
                        attributes_plus_term(&[(1, 5)], b"serie"),
                        attributes_plus_term(&[], &[]),
                        context_of(46, &[context(2, &[])]),
                    ],
                ),
                context_of(46, &[context(1, &[])]),
            ],
        );
        assert_eq!(
            search(type1(&BIB1, or_not)).unwrap(),
            FieldQuery::Or(
                Box::new(term_of(SearchField::CallNumber, "310", Truncation::Both)),
                Box::new(FieldQuery::Not(
                    Box::new(term_of(SearchField::Any, "serie", Truncation::None)),
                    Box::new(term_of(SearchField::Any, "", Truncation::None)),
                )),
            )
        );
        let code = |choice| {
            let refused = search(choice).unwrap_err();
            (refused.condition, refused.addinfo)
        };
        assert_eq!(code(context(0, b"any")), (107, None));
        assert_eq!(
            code(type1(
                &[1, 2, 840, 10003, 3, 2],
                attributes_plus_term(&[], b"x")
            )),
            (121, Some("1.2.840.10003.3.2".to_owned()))
        );
        let prox = context_of(
            1,
            &[
                attributes_plus_term(&[], b"a"),
                attributes_plus_term(&[], b"b"),
                context_of(46, &[context_of(3, &[])]),
            ],
        );
        assert_eq!(code(type1(&BIB1, prox)), (110, Some("prox".to_owned())));
        let result_set = context_of(0, &[context(31, b"one")]);
        assert_eq!(code(type1(&BIB1, result_set)), (3, None));
        let date_term = context_of(
            0,
            &[context_of(102, &[context_of(44, &[]), context(218, b"x")])],
        );
        assert_eq!(code(type1(&BIB1, date_term)), (3, None));
        let mut many = attributes_plus_term(&[], b"a");
        for _ in 0..MAX_TERMS {
            many = context_of(
                1,
                &[
                    many,
                    attributes_plus_term(&[], b"a"),
                    context_of(46, &[context(0, &[])]),
                ],
            );
        }
        assert_eq!(code(type1(&BIB1, many)), (108, None));
        let numeric = context_of(
            0,
            &[context_of(
                102,
                &[context_of(44, &[]), context(215, &[0x07, 0xCB])],
            )],
        );
        assert_eq!(
            search(type1(&BIB1, numeric)).unwrap(),
            term_of(SearchField::Any, "1995", Truncation::None)
        );
    }

    #[test]
    fn init_present_and_close_are_read() {
        let init = context_of(
            INIT_REQUEST,
            &[
                context(2, b"ref"),
                context(3, &[0x05, 0xE0]),
                context(4, &[0x06, 0xC0]),
                context_integer(5, 1024),
                context_integer(6, 2048),
            ],
        );
        assert_eq!(
            parse(&init).unwrap(),
            Request::Init {
                reference: Some(b"ref".to_vec()),
                versions: vec![0x05, 0xE0],
                options: vec![0x06, 0xC0],
                message_size: Some(1024),
                record_size: Some(2048),
            }
        );
        let present = z3950::present_request(2, 37).unwrap();
        assert_eq!(
            parse(&present).unwrap(),
            Request::Present {
                reference: None,
                set: "default".to_owned(),
                start: 21,
                count: 17,
                elements: Some("F".to_owned()),
                syntax: Some(z3950::USMARC.to_vec()),
            }
        );
        assert_eq!(
            parse(&z3950::close_request()).unwrap(),
            Request::Close { reference: None }
        );
        assert_eq!(parse(&context_of(35, &[])).unwrap(), Request::Other(35));
        assert_eq!(parse(&[0x02, 0x01, 0x00]), Err(BerError::Malformed));
    }
}
