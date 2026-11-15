// SPDX-License-Identifier: GPL-3.0-or-later
//! Series and related works (docs/specs/catalogo/bibframe-completo.md, item 29, part 5d),
//! after the Library of Congress conversion (`marc2bibframe2`: `ConvSpec-Process6-Series.xsl`,
//! `ConvSpec-720+740to755.xsl` and `ConvSpec-760-788-Links.xsl`).
//!
//! `440` and `830` (a series), `740` (a related or contained work) and `775` (another
//! edition) become `bf:relation` statements of the Work: a `bf:Relation` with its
//! `bf:relationship` from the `relationship` list and the `bf:associatedResource` described
//! in place. The series statement as transcribed (`490`) keeps its own field.

use elzevir_domain::statement::{Resource, Statement, Value};

use super::titles::Owner;
use super::{DataField, Marc, text};

/// The fields this module reads, in tag order.
pub(super) const TAGS: [&[u8; 3]; 4] = [b"440", b"740", b"775", b"830"];

/// The relations of the Work, in record order.
pub(super) fn of_work(marc: &Marc<'_>) -> Vec<Statement> {
    marc.data_in(&TAGS)
        .filter_map(|(tag, field)| of_field(*tag, field))
        .map(|(_, statement)| statement)
        .collect()
}

/// The relation one field gives; `None` when it names nothing.
pub(super) fn of_field(tag: [u8; 3], field: DataField<'_>) -> Option<(Owner, Statement)> {
    let (relationship, resource, enumeration) = match &tag {
        b"440" => ("series", series("Series", field)?, field.text(b'v')),
        b"830" => ("series", series("Hub", field)?, field.text(b'v')),
        b"740" => {
            let relationship = if field.indicators[1] == b'2' {
                "part"
            } else {
                "relatedwork"
            };
            let work = Resource {
                class: "Work".to_owned(),
                label: None,
                statements: vec![title(field, b'a')?],
            };
            (relationship, work, None)
        }
        b"775" => ("otheredition", other_edition(field)?, None),
        _ => return None,
    };
    let mut statements = vec![Statement {
        property: "relationship".to_owned(),
        value: Value::Concept {
            scheme: "relationship".to_owned(),
            code: relationship.to_owned(),
        },
    }];
    statements.push(resource_statement("associatedResource", resource));
    if let Some(enumeration) = enumeration {
        statements.push(literal("seriesEnumeration", enumeration));
    }
    let relation = Resource {
        class: "Relation".to_owned(),
        label: None,
        statements,
    };
    Some((Owner::Work, resource_statement("relation", relation)))
}

/// A series: its title and its ISSN.
fn series(class: &str, field: DataField<'_>) -> Option<Resource> {
    let mut statements = vec![title(field, b'a')?];
    if let Some(issn) = field.text(b'x') {
        statements.push(identifier("Issn", issn));
    }
    Some(Resource {
        class: class.to_owned(),
        label: None,
        statements,
    })
}

/// `775`: the other edition's Work, with its author, and its Instance with the edition
/// statement and the control number of the record that describes it.
fn other_edition(field: DataField<'_>) -> Option<Resource> {
    let mut statements = vec![title(field, b't')?];
    if let Some(name) = field.text(b'a') {
        let agent = Resource {
            class: "Agent".to_owned(),
            label: Some(name),
            statements: Vec::new(),
        };
        statements.push(resource_statement(
            "contribution",
            Resource {
                class: "Contribution".to_owned(),
                label: None,
                statements: vec![resource_statement("agent", agent)],
            },
        ));
    }
    let mut instance = Vec::new();
    if let Some(edition) = field.text(b'b') {
        instance.push(literal("editionStatement", edition));
    }
    if let Some(number) = field.raw(b'w') {
        instance.push(identifier("Local", number));
    }
    if !instance.is_empty() {
        statements.push(resource_statement(
            "hasInstance",
            Resource {
                class: "Instance".to_owned(),
                label: None,
                statements: instance,
            },
        ));
    }
    Some(Resource {
        class: "Work".to_owned(),
        label: None,
        statements,
    })
}

/// The title of a related resource: `$main` and the part number and name.
fn title(field: DataField<'_>, main: u8) -> Option<Statement> {
    let mut statements = vec![literal("mainTitle", field.text(main)?)];
    for (code, property) in [(b'n', "partNumber"), (b'p', "partName")] {
        statements.extend(
            field
                .values(code)
                .filter_map(text::clean)
                .map(|part| literal(property, part)),
        );
    }
    Some(resource_statement(
        "title",
        Resource {
            class: "Title".to_owned(),
            label: None,
            statements,
        },
    ))
}

fn identifier(class: &str, value: String) -> Statement {
    resource_statement(
        "identifiedBy",
        Resource {
            class: class.to_owned(),
            label: Some(value),
            statements: Vec::new(),
        },
    )
}

fn resource_statement(property: &str, value: Resource) -> Statement {
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

    fn relationship(statement: &Statement) -> Option<&str> {
        let Value::Resource(relation) = &statement.value else {
            return None;
        };
        relation.statements.iter().find_map(|s| match &s.value {
            Value::Concept { code, .. } if s.property == "relationship" => Some(code.as_str()),
            _ => None,
        })
    }

    #[test]
    fn series_related_works_and_other_editions_become_relations() {
        let record = Record {
            leader: *b"00000nam a2200000 i 4500",
            fields: vec![
                field(*b"490", *b"1 ", &[(b'a', "Grandes vultos ;"), (b'v', "3")]),
                field(*b"740", *b"22", &[(b'a', "A Democracia.")]),
                field(*b"740", *b"0 ", &[(b'a', "O Caso.")]),
                field(
                    *b"775",
                    *b"0 ",
                    &[
                        (b'a', "Ramalho"),
                        (b't', "Os fundamentos /"),
                        (b'b', "7. ed."),
                        (b'w', "153256"),
                    ],
                ),
                field(
                    *b"830",
                    *b" 0",
                    &[(b'a', "Grandes vultos ;"), (b'v', "3"), (b'x', "1234-5678")],
                ),
            ],
        };
        let relations = of_work(&Marc(&record));
        let kinds: Vec<Option<&str>> = relations.iter().map(relationship).collect();
        assert_eq!(
            kinds,
            [
                Some("part"),
                Some("relatedwork"),
                Some("otheredition"),
                Some("series")
            ]
        );
        let enumeration = match relations.get(3).map(|s| &s.value) {
            Some(Value::Resource(series)) => series.statements.last().cloned(),
            _ => None,
        };
        assert_eq!(
            enumeration,
            Some(literal("seriesEnumeration", "3".to_owned()))
        );
    }
}
