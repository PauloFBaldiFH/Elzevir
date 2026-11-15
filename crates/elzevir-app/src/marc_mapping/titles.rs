// SPDX-License-Identifier: GPL-3.0-or-later
//! Titles other than the preferred one (docs/specs/catalogo/bibframe-completo.md, item 29,
//! part 5a), after the Library of Congress conversion (`marc2bibframe2`:
//! `ConvSpec-200-247not240-Titles.xsl`).
//!
//! Each `210`, `222`, `242`, `243`, `246` and `247` becomes a `bf:title` statement of the
//! Work or of the Instance whose value is a `bf:VariantTitle` or one of its subclasses.

use elzevir_data::vocabulary;
use elzevir_domain::statement::{Resource, Statement, Value};

use super::{DataField, Marc, text};

/// The title fields, in tag order.
pub(super) const TAGS: [&[u8; 3]; 6] = [b"210", b"222", b"242", b"243", b"246", b"247"];

/// The entity a title belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Owner {
    /// The Work.
    Work,
    /// The Instance.
    Instance,
}

/// The title statements of the Work and of the Instance, in record order.
pub(super) fn of_owner(marc: &Marc<'_>, owner: Owner) -> Vec<Statement> {
    marc.data_in(&TAGS)
        .filter_map(|(tag, field)| of_field(*tag, field))
        .filter(|(found, _)| *found == owner)
        .map(|(_, statement)| statement)
        .collect()
}

/// The owner and the title statement of one field; `None` when the field has no title or
/// the conversion gives it none.
pub(super) fn of_field(tag: [u8; 3], field: DataField<'_>) -> Option<(Owner, Statement)> {
    let second = field.indicators[1];
    let (owner, class, variant) = match &tag {
        b"210" => match second {
            b'0' => (Owner::Instance, "AbbreviatedTitle", None),
            b' ' => (Owner::Work, "AbbreviatedTitle", None),
            _ => return None,
        },
        b"222" => (Owner::Instance, "KeyTitle", None),
        // The conversion gives `242` no type; "translated" is the list's word for it.
        b"242" => (Owner::Instance, "VariantTitle", Some("tra")),
        b"243" => (Owner::Work, "CollectiveTitle", None),
        b"246" => {
            let owner = if matches!(second, b'4' | b'8') {
                Owner::Instance
            } else {
                Owner::Work
            };
            let class = if second == b'1' {
                "ParallelTitle"
            } else {
                "VariantTitle"
            };
            (owner, class, variant_of_246(second))
        }
        b"247" => (Owner::Work, "VariantTitle", Some("for")),
        _ => return None,
    };
    let mut statements: Vec<Statement> = field
        .subfields
        .iter()
        .filter_map(|subfield| {
            let property = part_property(tag, subfield.code)?;
            let text = text::clean(&subfield.value)?;
            Some(part(property, text))
        })
        .collect();
    if !statements.iter().any(|s| s.property == "mainTitle") {
        return None;
    }
    if let Some(label) = variant
        .and_then(|code| vocabulary::scheme("vartitletype")?.concept(code))
        .map(|concept| concept.label)
    {
        statements.push(part("variantType", label.to_owned()));
    }
    if tag == *b"246"
        && let Some(note) = field.text(b'i')
    {
        statements.push(Statement {
            property: "note".to_owned(),
            value: Value::Resource(Resource {
                class: "Note".to_owned(),
                label: Some(note),
                statements: Vec::new(),
            }),
        });
    }
    let title = Statement {
        property: "title".to_owned(),
        value: Value::Resource(Resource {
            class: class.to_owned(),
            label: None,
            statements,
        }),
    };
    Some((owner, title))
}

/// `246` second indicator: the type of title it is.
const fn variant_of_246(second: u8) -> Option<&'static str> {
    match second {
        b'0' => Some("por"),
        b'2' => Some("dis"),
        b'4' => Some("cov"),
        b'5' => Some("atp"),
        b'6' => Some("cap"),
        b'7' => Some("run"),
        b'8' => Some("spi"),
        _ => None,
    }
}

/// The property a subfield of a title field gives, in the order they are written back.
pub(super) fn part_property(tag: [u8; 3], code: u8) -> Option<&'static str> {
    let qualified = matches!(&tag, b"210" | b"222");
    match code {
        b'a' => Some("mainTitle"),
        b'b' if qualified => Some("qualifier"),
        b'b' => Some("subtitle"),
        b'n' if !qualified => Some("partNumber"),
        b'p' if !qualified => Some("partName"),
        b'f' if matches!(&tag, b"243" | b"246" | b"247") => Some("date"),
        _ => None,
    }
}

fn part(property: &str, text: String) -> Statement {
    Statement {
        property: property.to_owned(),
        value: Value::Literal {
            text,
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

    fn title(class: &str, parts: &[(&str, &str)]) -> Statement {
        Statement {
            property: "title".to_owned(),
            value: Value::Resource(Resource {
                class: class.to_owned(),
                label: None,
                statements: parts
                    .iter()
                    .map(|(property, text)| part(property, (*text).to_owned()))
                    .collect(),
            }),
        }
    }

    #[test]
    fn each_title_field_goes_to_its_entity_with_its_class_and_type() {
        let record = Record {
            leader: *b"00000nam a2200000 i 4500",
            fields: vec![
                field(*b"210", *b"00", &[(b'a', "Rev. bras."), (b'b', "(Rio J.)")]),
                field(*b"242", *b"00", &[(b'a', "The wild ass's skin /")]),
                field(*b"246", *b"31", &[(b'a', "Capitu :"), (b'b', "romance")]),
                field(*b"246", *b"18", &[(b'a', "Dom Casmurro"), (b'n', "v. 1")]),
                field(
                    *b"246",
                    *b"3 ",
                    &[(b'i', "Título da capa:"), (b'a', "Bentinho")],
                ),
                field(
                    *b"247",
                    *b"10",
                    &[(b'a', "Revista antiga"), (b'f', "1900-1910")],
                ),
                field(*b"246", *b"13", &[(b'b', "sem título principal")]),
                field(*b"210", *b"09", &[(b'a', "Ignorado")]),
            ],
        };
        let marc = Marc(&record);
        assert_eq!(
            of_owner(&marc, Owner::Instance),
            [
                title(
                    "AbbreviatedTitle",
                    &[("mainTitle", "Rev. bras"), ("qualifier", "(Rio J.)")]
                ),
                title(
                    "VariantTitle",
                    &[
                        ("mainTitle", "The wild ass's skin"),
                        ("variantType", "translated")
                    ]
                ),
                title(
                    "VariantTitle",
                    &[
                        ("mainTitle", "Dom Casmurro"),
                        ("partNumber", "v. 1"),
                        ("variantType", "spine")
                    ]
                ),
            ]
        );
        let work = of_owner(&marc, Owner::Work);
        assert_eq!(
            work.first(),
            Some(&title(
                "ParallelTitle",
                &[("mainTitle", "Capitu"), ("subtitle", "romance")]
            ))
        );
        let note = match work.get(1).map(|s| &s.value) {
            Some(Value::Resource(with_note)) => with_note.statements.last().cloned(),
            _ => None,
        };
        assert_eq!(
            note.map(|s| s.value),
            Some(Value::Resource(Resource {
                class: "Note".to_owned(),
                label: Some("Título da capa".to_owned()),
                statements: Vec::new(),
            }))
        );
        assert_eq!(
            work.get(2),
            Some(&title(
                "VariantTitle",
                &[
                    ("mainTitle", "Revista antiga"),
                    ("date", "1900-1910"),
                    ("variantType", "former")
                ]
            ))
        );
        assert_eq!(work.len(), 3);
    }
}
