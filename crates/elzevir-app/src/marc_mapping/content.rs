// SPDX-License-Identifier: GPL-3.0-or-later
//! Geographic coverage, the languages of the parts and the script of the Work
//! (docs/specs/catalogo/bibframe-completo.md, item 29, part 5e), after the Library of
//! Congress conversions (`marc2bibframe2`: `ConvSpec-010-048.xsl` and `ConvSpec-5XX.xsl`;
//! `bibframe2marc`: `02-010-048.xml`).
//!
//! `043 $a` becomes `bf:geographicCoverage` (the IRI of the MARC geographic area); each
//! language of `041` other than `$a` becomes `bf:language` with a `bf:Language` that names
//! the part it applies to (`bf:part`); `546 $b` becomes `bf:notation`. The languages of
//! `041 $a` keep their own field.

use elzevir_domain::statement::{Resource, Statement, Value};

use super::{Marc, text, work};

/// The stem of the MARC geographic areas list.
pub(super) const GEOGRAPHIC_AREAS: &str = "http://id.loc.gov/vocabulary/geographicAreas/";

/// The `041` subfields that name the language of a part, with the `bf:part` that the
/// way back (`bibframe2marc`, `02-010-048.xml`) reads, one per subfield.
pub(super) const PARTS: [(u8, &str); 15] = [
    (b'b', "summary"),
    (b'd', "sung or spoken text"),
    (b'e', "libretto"),
    (b'f', "table of contents"),
    (b'g', "accompanying material"),
    (b'h', "original"),
    (b'i', "intertitles"),
    (b'j', "subtitles or captions"),
    (b'k', "intermediate translations"),
    (b'm', "original accompanying materials"),
    (b'n', "original libretto"),
    (b'p', "captions"),
    (b'q', "accessible audio"),
    (b'r', "accessible visual material"),
    (b't', "accompanying transcripts"),
];

/// The statements of the Work, in this order: coverage, languages of the parts, scripts.
pub(super) fn of_work(marc: &Marc<'_>) -> Vec<Statement> {
    let mut statements = geographic_coverage(marc);
    statements.extend(language_parts(marc));
    statements.extend(notations(marc));
    statements
}

/// `043 $a`: one IRI per area code; the trailing hyphens of the code are padding.
pub(super) fn geographic_coverage(marc: &Marc<'_>) -> Vec<Statement> {
    marc.data(b"043")
        .flat_map(|field| field.values(b'a'))
        .filter_map(area_code)
        .map(|code| Statement {
            property: "geographicCoverage".to_owned(),
            value: Value::Iri(format!("{GEOGRAPHIC_AREAS}{code}")),
        })
        .collect()
}

/// `S-BL` → `s-bl`; `None` for anything that is not a code.
fn area_code(value: &[u8]) -> Option<String> {
    let code = text::trimmed(value)?;
    let code = code.trim_end_matches('-').to_ascii_lowercase();
    (!code.is_empty() && code.bytes().all(|b| b.is_ascii_lowercase() || b == b'-')).then_some(code)
}

/// The languages of the parts, from the `041` fields with MARC codes (second indicator
/// blank), in field order.
pub(super) fn language_parts(marc: &Marc<'_>) -> Vec<Statement> {
    marc.data(b"041")
        .filter(|field| field.indicators[1] == b' ')
        .flat_map(|field| field.subfields.iter())
        .filter_map(|subfield| {
            let part = part_label(subfield.code)?;
            let joined = text::trimmed(&subfield.value)?;
            Some(
                work::split_codes(&joined)
                    .into_iter()
                    .map(move |code| language_part(&code, part)),
            )
        })
        .flatten()
        .collect()
}

/// The `bf:part` of a `041` subfield.
fn part_label(code: u8) -> Option<&'static str> {
    PARTS
        .iter()
        .find(|(subfield, _)| *subfield == code)
        .map(|(_, part)| *part)
}

/// The subfield of the part `part`.
pub(super) fn part_code(part: &str) -> Option<u8> {
    PARTS
        .iter()
        .find(|(_, known)| *known == part)
        .map(|(code, _)| *code)
}

/// A `bf:Language` with its code and the part it applies to.
pub(super) fn language_part(code: &str, part: &str) -> Statement {
    Statement {
        property: "language".to_owned(),
        value: Value::Resource(Resource {
            class: "Language".to_owned(),
            label: None,
            statements: vec![literal("code", code), literal("part", part)],
        }),
    }
}

/// `546 $b`: the script or notation of the content.
pub(super) fn notations(marc: &Marc<'_>) -> Vec<Statement> {
    marc.data(b"546")
        .flat_map(|field| field.values(b'b'))
        .filter_map(text::clean)
        .map(notation)
        .collect()
}

/// A `bf:Notation` with its label.
pub(super) fn notation(label: String) -> Statement {
    Statement {
        property: "notation".to_owned(),
        value: Value::Resource(Resource {
            class: "Notation".to_owned(),
            label: Some(label),
            statements: Vec::new(),
        }),
    }
}

fn literal(property: &str, text: &str) -> Statement {
    Statement {
        property: property.to_owned(),
        value: Value::Literal {
            text: text.to_owned(),
            language: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use elzevir_marc::{Field, Record, Subfield, Tag};

    use super::*;

    fn field(tag: [u8; 3], indicators: [u8; 2], subfields: &[(u8, &str)]) -> Field {
        Field::Data {
            tag: Tag::new(tag),
            indicators,
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
    fn coverage_languages_of_parts_and_scripts_become_statements() {
        let record = Record {
            leader: *b"00000nam a2200000 i 4500",
            fields: vec![
                field(*b"041", *b"1 ", &[(b'a', "por"), (b'h', "polita")]),
                field(*b"041", *b"07", &[(b'h', "pt"), (b'2', "iso639-1")]),
                field(*b"043", *b"  ", &[(b'a', "S-BL"), (b'a', "n-us---")]),
                field(
                    *b"546",
                    *b"  ",
                    &[(b'a', "Texto em polonês."), (b'b', "Latino.")],
                ),
            ],
        };
        let iri = |code: &str| Statement {
            property: "geographicCoverage".to_owned(),
            value: Value::Iri(format!("{GEOGRAPHIC_AREAS}{code}")),
        };
        assert_eq!(
            of_work(&Marc(&record)),
            [
                iri("s-bl"),
                iri("n-us"),
                language_part("pol", "original"),
                language_part("ita", "original"),
                notation("Latino".to_owned()),
            ]
        );
        assert_eq!(part_code("original"), Some(b'h'));
        assert_eq!(part_code("original accompanying materials"), Some(b'm'));
    }
}
