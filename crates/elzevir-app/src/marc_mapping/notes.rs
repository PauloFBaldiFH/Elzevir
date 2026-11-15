// SPDX-License-Identifier: GPL-3.0-or-later
//! Notes with a type of their own, custodial history and frequency (docs/specs/catalogo/
//! bibframe-completo.md, item 29, part 5b), after the Library of Congress conversion
//! (`marc2bibframe2`: `ConvSpec-5XX.xsl` and `ConvSpec-3XX.xsl`).
//!
//! A note the conversion gives a `bf:noteType` from the `mnotetype` list becomes a `bf:note`
//! statement of the Work or of the Instance. General, contents and local notes (`500`,
//! `505`, `590`) keep their own field.

use elzevir_data::vocabulary;
use elzevir_domain::statement::{Resource, Statement, Value};

use super::titles::Owner;
use super::{DataField, Marc};

/// The fields this module reads, in tag order.
pub(super) const TAGS: [&[u8; 3]; 20] = [
    b"310", b"501", b"504", b"513", b"515", b"516", b"530", b"533", b"534", b"536", b"544", b"545",
    b"546", b"550", b"555", b"556", b"561", b"581", b"585", b"588",
];

/// Subfields that hold the text of a note: letters only; `$3` to `$8` are control data.
const TEXT_CODES: &[u8] = b"abcdefghijklmnopqrstuvwxyz";

/// The statements of `owner`, in record order.
pub(super) fn of_owner(marc: &Marc<'_>, owner: Owner) -> Vec<Statement> {
    marc.data_in(&TAGS)
        .filter_map(|(tag, field)| of_field(*tag, field))
        .filter(|(found, _)| *found == owner)
        .map(|(_, statement)| statement)
        .collect()
}

/// The owner and the statement of one field; `None` when it has no text.
pub(super) fn of_field(tag: [u8; 3], field: DataField<'_>) -> Option<(Owner, Statement)> {
    match &tag {
        b"310" => {
            let mut statements = Vec::new();
            if let Some(date) = field.text(b'b') {
                statements.push(literal("date", date));
            }
            let frequency = Resource {
                class: "Frequency".to_owned(),
                label: Some(field.text(b'a')?),
                statements,
            };
            Some((Owner::Instance, resource("frequency", frequency)))
        }
        b"561" => Some((
            Owner::Instance,
            literal("custodialHistory", field.raw(b'a')?),
        )),
        _ => {
            let (owner, code) = note_type(tag, field.indicators[0])?;
            // `546 $b` is the script (`bf:notation`), not part of the note.
            let codes = if tag == *b"546" { b"a" } else { TEXT_CODES };
            let text = field.raw_joined(codes)?;
            Some((owner, note(code, text)))
        }
    }
}

/// The entity and the `mnotetype` code of a note field.
fn note_type(tag: [u8; 3], first: u8) -> Option<(Owner, &'static str)> {
    let code = match &tag {
        b"501" => "with",
        b"504" => "biblio",
        b"513" => "report",
        b"515" => "issuance",
        b"516" => "computer",
        b"530" => "addphys",
        b"533" => "repro",
        b"534" => "orig",
        b"536" => "fundinfo",
        b"544" | b"581" => "related",
        b"545" if first == b'1' => "adminhist",
        b"545" => "biogdata",
        // The language of the content describes the Work.
        b"546" => return Some((Owner::Work, "lang")),
        b"550" => "issuing",
        b"555" if first == b'0' => "finding",
        b"555" => "index",
        b"556" => "doc",
        b"585" => "exhibit",
        b"588" => "descsource",
        _ => return None,
    };
    Some((Owner::Instance, code))
}

/// A `bf:Note` with its text and the label of its `mnotetype` concept.
pub(super) fn note(code: &str, text: String) -> Statement {
    let mut statements = Vec::new();
    if let Some(concept) = vocabulary::scheme("mnotetype").and_then(|s| s.concept(code)) {
        statements.push(literal("noteType", concept.label.to_owned()));
    }
    resource(
        "note",
        Resource {
            class: "Note".to_owned(),
            label: Some(text),
            statements,
        },
    )
}

fn resource(property: &str, value: Resource) -> Statement {
    Statement {
        property: property.to_owned(),
        value: Value::Resource(value),
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

/// The `mnotetype` code whose label is `label`.
pub(super) fn note_code(label: &str) -> Option<&'static str> {
    vocabulary::scheme("mnotetype")?
        .offered()
        .find(|concept| concept.label == label)
        .map(|concept| concept.code)
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
    fn typed_notes_custody_and_frequency_go_to_their_entity() {
        let record = Record {
            leader: *b"00000nam a2200000 i 4500",
            fields: vec![
                field(*b"310", *b"  ", &[(b'a', "Mensal,"), (b'b', "2001-")]),
                field(*b"500", *b"  ", &[(b'a', "Nota geral.")]),
                field(
                    *b"504",
                    *b"  ",
                    &[(b'a', "Inclui bibliografia."), (b'5', "BR")],
                ),
                field(*b"546", *b"  ", &[(b'a', "Texto em português e inglês.")]),
                field(*b"555", *b"0 ", &[(b'a', "Inventário"), (b'u', "http://x")]),
                field(*b"561", *b"  ", &[(b'a', "Doação de Ana.")]),
                field(*b"504", *b"  ", &[(b'5', "BR")]),
            ],
        };
        let marc = Marc(&record);
        let typed = |code: &str, text: &str| note(code, text.to_owned());
        assert_eq!(
            of_owner(&marc, Owner::Instance),
            [
                resource(
                    "frequency",
                    Resource {
                        class: "Frequency".to_owned(),
                        label: Some("Mensal".to_owned()),
                        statements: vec![literal("date", "2001-".to_owned())],
                    }
                ),
                typed("biblio", "Inclui bibliografia."),
                typed("finding", "Inventário http://x"),
                literal("custodialHistory", "Doação de Ana.".to_owned()),
            ]
        );
        assert_eq!(
            of_owner(&marc, Owner::Work),
            [typed("lang", "Texto em português e inglês.")]
        );
        assert_eq!(note_code("bibliography"), Some("biblio"));
    }
}
