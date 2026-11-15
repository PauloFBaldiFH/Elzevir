// SPDX-License-Identifier: GPL-3.0-or-later
//! The coded publication of `008` (docs/specs/catalogo/bibframe-completo.md, item 29, part
//! 5e): the type of date (`008/06`), the dates (`07-14`) and the country (`15-17`), after
//! the Library of Congress conversions (`marc2bibframe2`: `ConvSpec-Process8-ProvAct.xsl`
//! and `ConvSpec-006,008.xsl`; `bibframe2marc`: `01-001-008.xml`).
//!
//! The publication as transcribed (`260`/`264`) keeps its own fields. The coded one becomes
//! a `bf:Publication` statement of the Instance that holds only coded data: the date in EDTF,
//! the country (a concept of the `countries` list, or a `bf:Place` with `bf:code` for a code
//! outside it) and, for a continuing resource, its status. A reprint's original date becomes
//! `bf:originDate` of the Work and a copyright date `bf:copyrightDate` of the Instance. Type
//! `p` (distribution and production) and `b` (B.C.) stay only in the source record.

use elzevir_data::vocabulary;
use elzevir_domain::statement::{Resource, Statement, Value};

use super::Marc;

/// The parts of `008` this module reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Coded {
    /// `008/06`.
    pub(super) kind: u8,
    /// `008/07-10`.
    pub(super) first: [u8; 4],
    /// `008/11-14`.
    pub(super) second: [u8; 4],
    /// `008/15-17`.
    pub(super) country: [u8; 3],
}

impl Coded {
    fn of(fixed: &[u8]) -> Option<Self> {
        Some(Self {
            kind: *fixed.get(6)?,
            first: fixed.get(7..11)?.try_into().ok()?,
            second: fixed.get(11..15)?.try_into().ok()?,
            country: fixed.get(15..18)?.try_into().ok()?,
        })
    }
}

/// The statements of the Instance: the coded publication and a copyright date.
pub(super) fn of_instance(marc: &Marc<'_>) -> Vec<Statement> {
    marc.control(*b"008")
        .and_then(Coded::of)
        .map(|coded| instance_statements(&coded))
        .unwrap_or_default()
}

/// The statements of the Work: the original date of a reprint.
pub(super) fn of_work(marc: &Marc<'_>) -> Vec<Statement> {
    marc.control(*b"008")
        .and_then(Coded::of)
        .filter(|coded| coded.kind == b'r')
        .and_then(|coded| year(coded.second))
        .map(|date| vec![literal("originDate", date)])
        .unwrap_or_default()
}

fn instance_statements(coded: &Coded) -> Vec<Statement> {
    let mut publication = Vec::new();
    if let Some(date) = edtf(coded) {
        publication.push(literal("date", date));
    }
    match coded.kind {
        b'c' => publication.push(status("current")),
        b'd' => publication.push(status("ceased")),
        _ => {}
    }
    if let Some(place) = place(coded.country) {
        publication.push(place);
    }
    let mut statements = Vec::new();
    if !publication.is_empty() {
        statements.push(Statement {
            property: "provisionActivity".to_owned(),
            value: Value::Resource(Resource {
                class: "Publication".to_owned(),
                label: None,
                statements: publication,
            }),
        });
    }
    if coded.kind == b't'
        && let Some(date) = year(coded.second)
    {
        statements.push(literal("copyrightDate", date));
    }
    statements
}

/// The date of the publication in EDTF, as the LC conversion writes it; `u` becomes `X`.
fn edtf(coded: &Coded) -> Option<String> {
    let first = year(coded.first);
    let second = year(coded.second);
    match coded.kind {
        b'c' => Some(format!("{}/..", first?)),
        b'd' | b'i' | b'k' | b'm' | b'u' => Some(format!("{}/{}", first?, second?)),
        b'q' => Some(format!("{}~/{}~", first?, second?)),
        b'e' => {
            // `08/11-14` holds the month and the day of a detailed date.
            let month = std::str::from_utf8(coded.second.get(..2)?).ok()?;
            let day = std::str::from_utf8(coded.second.get(2..)?).ok()?.trim();
            let mut date = format!("{}-{}", first?, month.replace(['u', 'U'], "X"));
            if !day.is_empty() {
                date.push('-');
                date.push_str(&day.replace(['u', 'U'], "X"));
            }
            Some(date)
        }
        b'r' | b's' | b't' => first,
        b'n' => Some(match (first, second) {
            (Some(first), Some(second)) => format!("{first}/{second}"),
            (Some(first), None) => first,
            _ => "XXXX".to_owned(),
        }),
        _ => None,
    }
}

/// Four digits, with `u` for an unknown one; `None` for blanks and fill characters.
fn year(value: [u8; 4]) -> Option<String> {
    value
        .iter()
        .all(|b| b.is_ascii_digit() || matches!(b, b'u' | b'U'))
        .then(|| String::from_utf8_lossy(&value).replace(['u', 'U'], "X"))
}

/// The country: two letters and a blank, or three letters.
fn place(country: [u8; 3]) -> Option<Statement> {
    let text = String::from_utf8_lossy(&country);
    let code = text.trim_end();
    if code.is_empty() || code.contains('|') {
        return None;
    }
    let known = vocabulary::scheme("countries").is_some_and(|s| s.concept(code).is_some());
    let value = if known {
        Value::Concept {
            scheme: "countries".to_owned(),
            code: code.to_owned(),
        }
    } else {
        Value::Resource(Resource {
            class: "Place".to_owned(),
            label: None,
            statements: vec![literal("code", code.to_owned())],
        })
    };
    Some(Statement {
        property: "place".to_owned(),
        value,
    })
}

fn status(code: &str) -> Statement {
    Statement {
        property: "status".to_owned(),
        value: Value::Concept {
            scheme: "mstatus".to_owned(),
            code: code.to_owned(),
        },
    }
}

fn literal(property: &str, text: String) -> Statement {
    Statement {
        property: property.to_owned(),
        value: Value::Literal {
            text,
            language: None,
        },
    }
}

/// The `008/06-17` that `publication`, `copyright` and `origin` give: the way back.
pub(super) fn coded_of(
    publication: Option<&Resource>,
    copyright: Option<&str>,
    origin: Option<&str>,
) -> Coded {
    let date = publication.and_then(|p| text_of(&p.statements, "date"));
    let status = |wanted: &str| {
        publication.is_some_and(|p| {
            p.statements.iter().any(|s| {
                matches!(&s.value, Value::Concept { code, .. }
                    if s.property == "status" && code == wanted)
            })
        })
    };
    let ceased = status("ceased");

    let blank = *b"    ";
    let four = |text: &str| -> [u8; 4] {
        let digits = text.replace('X', "u");
        digits.as_bytes().try_into().unwrap_or(blank)
    };
    let (kind, first, second) = match date {
        // A continuing resource without dates keeps its status.
        None if status("current") => (b'c', blank, blank),
        None if ceased => (b'd', blank, blank),
        None => (b'|', *b"||||", *b"||||"),
        Some("XXXX") => (b'n', *b"uuuu", *b"uuuu"),
        Some(date) => {
            if let Some(first) = date.strip_suffix("/..") {
                (b'c', four(first), *b"9999")
            } else if let Some((first, second)) = date.split_once("~/") {
                (b'q', four(first), four(second.trim_end_matches('~')))
            } else if let Some((first, second)) = date.split_once('/') {
                (if ceased { b'd' } else { b'm' }, four(first), four(second))
            } else if let Some((first, rest)) = date.split_once('-') {
                let mut detail = rest.replace('-', "").replace('X', "u");
                detail.truncate(4);
                let detail = format!("{detail:<4}");
                (
                    b'e',
                    four(first),
                    detail.as_bytes().try_into().unwrap_or(blank),
                )
            } else if let Some(origin) = origin {
                (b'r', four(date), four(origin))
            } else if let Some(copyright) = copyright {
                (b't', four(date), four(copyright))
            } else {
                (b's', four(date), blank)
            }
        }
    };
    let country = publication
        .and_then(|p| {
            p.statements.iter().find_map(|s| match &s.value {
                Value::Concept { code, .. } if s.property == "place" => Some(code.as_str()),
                Value::Resource(place) if s.property == "place" => {
                    text_of(&place.statements, "code")
                }
                _ => None,
            })
        })
        // Without a country the positions stay blank; `xx` would claim an unknown place.
        .map_or(*b"   ", |code| {
            let padded = format!("{code:<3}");
            padded.as_bytes().try_into().unwrap_or(*b"xx ")
        });
    Coded {
        kind,
        first,
        second,
        country,
    }
}

/// Whether `text` is a date as `008` codes it: four digits or `X`.
pub(super) fn is_coded_year(text: &str) -> bool {
    text.len() == 4 && text.bytes().all(|b| b.is_ascii_digit() || b == b'X')
}

fn text_of<'a>(statements: &'a [Statement], property: &str) -> Option<&'a str> {
    statements.iter().find_map(|s| match &s.value {
        Value::Literal { text, .. } if s.property == property => Some(text.as_str()),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coded(fixed: &str) -> Coded {
        Coded::of(fixed.as_bytes()).unwrap()
    }

    fn date(fixed: &str) -> Option<String> {
        edtf(&coded(fixed))
    }

    #[test]
    fn dates_and_countries_follow_the_lc_conversion() {
        assert_eq!(date("250321s1995||||xx ").as_deref(), Some("1995"));
        assert_eq!(date("900503c1988    spb").as_deref(), Some("1988/.."));
        assert_eq!(date("900503d19881999bl ").as_deref(), Some("1988/1999"));
        assert_eq!(date("900503q19uu1999bl ").as_deref(), Some("19XX~/1999~"));
        assert_eq!(date("900503e19880312bl ").as_deref(), Some("1988-03-12"));
        assert_eq!(date("901222n00001990bsp").as_deref(), Some("0000/1990"));
        assert_eq!(date("901222nuuuuuuuubl ").as_deref(), Some("XXXX/XXXX"));
        let statements = instance_statements(&coded("031027t20011993rjb"));
        assert_eq!(statements.len(), 2);
        assert_eq!(statements[1], literal("copyrightDate", "1993".to_owned()));
        let Value::Resource(publication) = &statements[0].value else {
            panic!("a publication");
        };
        assert!(
            matches!(&publication.statements[1].value, Value::Resource(place) if place.class == "Place")
        );
        let bl = instance_statements(&coded("250321s1995    bl "));
        let Value::Resource(publication) = &bl[0].value else {
            panic!("a publication");
        };
        // The way back gives the same positions.
        assert_eq!(
            coded_of(Some(publication), None, None),
            coded("250321s1995    bl ")
        );
        assert_eq!(
            coded_of(Some(publication), Some("1993"), None),
            coded("250321t19951993bl ")
        );
        assert_eq!(
            coded_of(Some(publication), None, Some("1984")),
            coded("250321r19951984bl ")
        );
    }
}
