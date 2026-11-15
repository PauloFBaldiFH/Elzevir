// SPDX-License-Identifier: GPL-3.0-or-later
//! Production, distribution, manufacture and copyright date (docs/specs/catalogo/
//! bibframe-completo.md, item 29, part 5c), after the Library of Congress conversion
//! (`marc2bibframe2`: `ConvSpec-250-270.xsl` and `ConvSpec-Process8-ProvAct.xsl`).
//!
//! The publication keeps its own fields (place, publisher, date). The manufacture in
//! `260 $e $f $g` and each `264` with second indicator `0`, `2`, `3` become
//! `bf:provisionActivity` statements of the Instance; `264` with `4` gives `bf:copyrightDate`.

use elzevir_domain::statement::{Resource, Statement, Value};

use super::{DataField, Marc, text};

/// The statements of the Instance, in record order.
pub(super) fn of_instance(marc: &Marc<'_>) -> Vec<Statement> {
    let mut statements: Vec<Statement> = marc
        .first(b"260")
        .and_then(manufacture_of_260)
        .into_iter()
        .collect();
    statements.extend(marc.data(b"264").flat_map(of_264));
    statements
}

/// `260 $e $f $g`: where, by whom and when the resource was manufactured.
fn manufacture_of_260(field: DataField<'_>) -> Option<Statement> {
    activity("Manufacture", field, *b"efg")
}

/// The statements one `264` gives: an activity, or copyright dates.
pub(super) fn of_264(field: DataField<'_>) -> Vec<Statement> {
    let class = match field.indicators[1] {
        b'0' => "Production",
        b'2' => "Distribution",
        b'3' => "Manufacture",
        b'4' => {
            return field
                .values(b'c')
                .filter_map(part_text)
                .map(|date| literal("copyrightDate", date))
                .collect();
        }
        _ => return Vec::new(),
    };
    activity(class, field, *b"abc").into_iter().collect()
}

/// An activity from the subfields for place, agent and date, in field order.
fn activity(class: &str, field: DataField<'_>, [place, agent, date]: [u8; 3]) -> Option<Statement> {
    let statements: Vec<Statement> = field
        .subfields
        .iter()
        .filter_map(|subfield| {
            let text = part_text(&subfield.value)?;
            Some(match subfield.code {
                code if code == place => named("place", "Place", text),
                code if code == agent => named("agent", "Agent", text),
                code if code == date => literal("date", text),
                _ => return None,
            })
        })
        .collect();
    (!statements.is_empty()).then(|| Statement {
        property: "provisionActivity".to_owned(),
        value: Value::Resource(Resource {
            class: class.to_owned(),
            label: None,
            statements,
        }),
    })
}

/// A value without ISBD punctuation and without the parentheses that enclose a
/// manufacture statement.
fn part_text(value: &[u8]) -> Option<String> {
    let cleaned = text::clean(value)?;
    let unwrapped = cleaned.trim_start_matches('(').trim_end_matches(')');
    text::clean(unwrapped.as_bytes())
}

fn named(property: &str, class: &str, label: String) -> Statement {
    Statement {
        property: property.to_owned(),
        value: Value::Resource(Resource {
            class: class.to_owned(),
            label: Some(label),
            statements: Vec::new(),
        }),
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

    fn manufacture(class: &str, parts: Vec<Statement>) -> Statement {
        Statement {
            property: "provisionActivity".to_owned(),
            value: Value::Resource(Resource {
                class: class.to_owned(),
                label: None,
                statements: parts,
            }),
        }
    }

    #[test]
    fn manufacture_other_activities_and_copyright_become_statements() {
        let record = Record {
            leader: *b"00000nam a2200000 i 4500",
            fields: vec![
                field(
                    *b"260",
                    *b"  ",
                    &[
                        (b'a', "Campo Erê, SC :"),
                        (b'b', "[s.n.],"),
                        (b'c', "2014"),
                        (b'e', "(Chapecó, SC :"),
                        (b'f', "Gráfica Schaefer)"),
                    ],
                ),
                field(*b"264", *b" 1", &[(b'a', "Curitiba :"), (b'b', "Editora,")]),
                field(
                    *b"264",
                    *b" 2",
                    &[(b'a', "São Paulo :"), (b'b', "Distribuidora")],
                ),
                field(*b"264", *b" 4", &[(b'c', "©2014")]),
            ],
        };
        assert_eq!(
            of_instance(&Marc(&record)),
            [
                manufacture(
                    "Manufacture",
                    vec![
                        named("place", "Place", "Chapecó, SC".to_owned()),
                        named("agent", "Agent", "Gráfica Schaefer".to_owned()),
                    ]
                ),
                manufacture(
                    "Distribution",
                    vec![
                        named("place", "Place", "São Paulo".to_owned()),
                        named("agent", "Agent", "Distribuidora".to_owned()),
                    ]
                ),
                literal("copyrightDate", "©2014".to_owned()),
            ]
        );
    }
}
