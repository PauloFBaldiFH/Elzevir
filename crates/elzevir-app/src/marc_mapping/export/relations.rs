// SPDX-License-Identifier: GPL-3.0-or-later
//! Series and related works back to `440`, `740`, `775` and `830` (bibframe-completo.md,
//! item 29, part 5d).

use elzevir_domain::statement::{Resource, Statement, Value};
use elzevir_marc::Field;

use super::super::relations::{TAGS, of_field};
use super::super::titles::Owner;
use super::described::StatementFields;
use super::fields::data_field;

/// The fields of series and related works.
pub(super) const RELATIONS: StatementFields = StatementFields {
    tags: &TAGS,
    read: of_field,
    write: field_of,
};

/// The field one relation of the Work is written as; `None` for any other statement.
fn field_of(owner: Owner, statement: &Statement) -> Option<Field> {
    if owner != Owner::Work || statement.property != "relation" {
        return None;
    }
    let Value::Resource(relation) = &statement.value else {
        return None;
    };
    let relationship = relation.statements.iter().find_map(|s| match &s.value {
        Value::Concept { scheme, code }
            if s.property == "relationship" && scheme == "relationship" =>
        {
            Some(code.as_str())
        }
        _ => None,
    })?;
    let related = relation.statements.iter().find_map(|s| match &s.value {
        Value::Resource(r) if s.property == "associatedResource" => Some(r),
        _ => None,
    })?;
    let title = resource(related, "title")?;
    let mut subfields: Vec<(u8, &str)> = Vec::new();
    let (tag, indicators) = match (relationship, related.class.as_str()) {
        ("series", class @ ("Series" | "Hub")) => {
            subfields.extend(title_parts(title, b'a'));
            subfields.extend(literal(relation, "seriesEnumeration").map(|v| (b'v', v)));
            subfields.extend(
                resource(related, "identifiedBy")
                    .filter(|id| id.class == "Issn")
                    .and_then(|id| id.label.as_deref())
                    .map(|issn| (b'x', issn)),
            );
            if class == "Series" {
                (*b"440", *b" 0")
            } else {
                (*b"830", *b" 0")
            }
        }
        ("part", "Work") => {
            subfields.extend(title_parts(title, b'a'));
            (*b"740", *b"02")
        }
        ("relatedwork", "Work") => {
            subfields.extend(title_parts(title, b'a'));
            (*b"740", *b"0 ")
        }
        ("otheredition", "Work") => {
            let agent = resource(related, "contribution")
                .and_then(|c| resource(c, "agent"))
                .and_then(|a| a.label.as_deref());
            subfields.extend(agent.map(|name| (b'a', name)));
            subfields.extend(title_parts(title, b't'));
            let instance = resource(related, "hasInstance");
            subfields.extend(
                instance
                    .and_then(|i| literal(i, "editionStatement"))
                    .map(|edition| (b'b', edition)),
            );
            subfields.extend(
                instance
                    .and_then(|i| resource(i, "identifiedBy"))
                    .and_then(|id| id.label.as_deref())
                    .map(|number| (b'w', number)),
            );
            (*b"775", *b"0 ")
        }
        _ => return None,
    };
    Some(data_field(tag, indicators, &subfields))
}

/// The main title under `main`, then the part numbers and names.
fn title_parts(title: &Resource, main: u8) -> Vec<(u8, &str)> {
    let mut parts: Vec<(u8, &str)> = literal(title, "mainTitle")
        .map(|t| (main, t))
        .into_iter()
        .collect();
    for (code, property) in [(b'n', "partNumber"), (b'p', "partName")] {
        parts.extend(
            title
                .statements
                .iter()
                .filter(|s| s.property == property)
                .filter_map(|s| match &s.value {
                    Value::Literal { text, .. } => Some((code, text.as_str())),
                    _ => None,
                }),
        );
    }
    parts
}

fn resource<'a>(of: &'a Resource, property: &str) -> Option<&'a Resource> {
    of.statements.iter().find_map(|s| match &s.value {
        Value::Resource(r) if s.property == property => Some(r),
        _ => None,
    })
}

fn literal<'a>(of: &'a Resource, property: &str) -> Option<&'a str> {
    of.statements.iter().find_map(|s| match &s.value {
        Value::Literal { text, .. } if s.property == property => Some(text.as_str()),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use elzevir_domain::statement::Description;
    use elzevir_marc::Record;

    use super::super::super::Marc;
    use super::super::super::relations::of_work;
    use super::super::described::write;
    use super::*;

    #[test]
    fn every_relation_comes_back_from_the_field_it_is_written_to() {
        let fields = vec![
            data_field(*b"440", *b" 0", &[(b'a', "Coleção"), (b'v', "2")]),
            data_field(
                *b"740",
                *b"02",
                &[(b'a', "A Democracia"), (b'n', "Parte 1")],
            ),
            data_field(*b"740", *b"0 ", &[(b'a', "O Caso")]),
            data_field(
                *b"775",
                *b"0 ",
                &[
                    (b'a', "Ramalho"),
                    (b't', "Os fundamentos"),
                    (b'b', "7. ed"),
                    (b'w', "153256"),
                ],
            ),
            data_field(
                *b"830",
                *b" 0",
                &[(b'a', "Grandes vultos"), (b'v', "3"), (b'x', "1234-5678")],
            ),
        ];
        let source = Record {
            leader: *b"00000nam a2200000 i 4500",
            fields: fields.clone(),
        };
        let work = Description {
            classes: Vec::new(),
            statements: of_work(&Marc(&source)),
        };
        assert_eq!(work.statements.len(), 5);
        let mut out = Record {
            leader: source.leader,
            fields: Vec::new(),
        };
        write(&mut out, &RELATIONS, &work, &Description::default(), None);
        assert_eq!(out.fields, fields);
    }
}
