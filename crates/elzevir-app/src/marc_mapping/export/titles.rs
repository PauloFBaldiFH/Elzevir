// SPDX-License-Identifier: GPL-3.0-or-later
//! Titles other than the preferred one back to `210`, `222`, `242`, `243`, `246` and `247`
//! (bibframe-completo.md, item 29, part 5a).

use elzevir_data::vocabulary;
use elzevir_domain::statement::{Resource, Statement, Value, is_other_title};
use elzevir_marc::Field;

use super::super::titles::{Owner, TAGS, of_field, part_property};
use super::described::StatementFields;
use super::fields::data_field;

/// The title fields.
pub(super) const TITLES: StatementFields = StatementFields {
    tags: &TAGS,
    read: of_field,
    write: field_of,
};

/// The field one title is written as; `None` for a statement that is not another title.
fn field_of(owner: Owner, title: &Statement) -> Option<Field> {
    if !is_other_title(title) {
        return None;
    }
    let Value::Resource(resource) = &title.value else {
        return None;
    };
    let variant = literal(resource, "variantType").and_then(variant_code);
    let (tag, indicators) = match (resource.class.as_str(), variant) {
        ("AbbreviatedTitle", _) => (
            *b"210",
            [b'0', if owner == Owner::Instance { b'0' } else { b' ' }],
        ),
        ("KeyTitle", _) => (*b"222", *b" 0"),
        ("CollectiveTitle", _) => (*b"243", *b"10"),
        (_, Some("tra")) => (*b"242", *b"00"),
        (_, Some("for")) => (*b"247", *b"10"),
        ("ParallelTitle", _) => (*b"246", *b"11"),
        (_, code) => (*b"246", [b'1', second_of_246(code)]),
    };
    let note = resource.statements.iter().find_map(|statement| {
        match (&*statement.property, &statement.value) {
            ("note", Value::Resource(note)) => note.label.as_deref(),
            _ => None,
        }
    });
    let mut subfields: Vec<(u8, &str)> = Vec::new();
    if tag == *b"246"
        && let Some(note) = note
    {
        subfields.push((b'i', note));
    }
    for code in *b"abnpf" {
        let Some(property) = part_property(tag, code) else {
            continue;
        };
        subfields.extend(
            resource
                .statements
                .iter()
                .filter(|statement| statement.property == property)
                .filter_map(|statement| match &statement.value {
                    Value::Literal { text, .. } => Some((code, text.as_str())),
                    _ => None,
                }),
        );
    }
    Some(data_field(tag, indicators, &subfields))
}

fn literal<'a>(resource: &'a Resource, property: &str) -> Option<&'a str> {
    resource
        .statements
        .iter()
        .find_map(|statement| match &statement.value {
            Value::Literal { text, .. } if statement.property == property => Some(text.as_str()),
            _ => None,
        })
}

/// The `vartitletype` code whose label is `label`.
fn variant_code(label: &str) -> Option<&'static str> {
    vocabulary::scheme("vartitletype")?
        .offered()
        .find(|concept| concept.label == label)
        .map(|concept| concept.code)
}

/// `246` second indicator for a type of title; blank when the list has no indicator for it.
fn second_of_246(code: Option<&str>) -> u8 {
    match code {
        Some("por") => b'0',
        Some("dis") => b'2',
        Some("cov") => b'4',
        Some("atp") => b'5',
        Some("cap") => b'6',
        Some("run") => b'7',
        Some("spi") => b'8',
        _ => b' ',
    }
}

#[cfg(test)]
mod tests {
    use elzevir_domain::statement::Description;
    use elzevir_marc::Record;

    use super::super::super::Marc;
    use super::super::super::titles::of_owner;
    use super::*;

    fn part(property: &str, text: &str) -> Statement {
        Statement {
            property: property.to_owned(),
            value: Value::Literal {
                text: text.to_owned(),
                language: None,
            },
        }
    }

    fn title(class: &str, parts: Vec<Statement>) -> Statement {
        Statement {
            property: "title".to_owned(),
            value: Value::Resource(Resource {
                class: class.to_owned(),
                label: None,
                statements: parts,
            }),
        }
    }

    #[test]
    fn every_kind_of_title_comes_back_from_the_field_it_is_written_to() {
        let work = Description {
            classes: Vec::new(),
            statements: vec![
                title(
                    "ParallelTitle",
                    vec![part("mainTitle", "Capitu"), part("subtitle", "romance")],
                ),
                title("CollectiveTitle", vec![part("mainTitle", "Obras")]),
                title(
                    "VariantTitle",
                    vec![
                        part("mainTitle", "Revista antiga"),
                        part("date", "1900-1910"),
                        part("variantType", "former"),
                    ],
                ),
                title(
                    "AbbreviatedTitle",
                    vec![
                        part("mainTitle", "Rev. bras"),
                        part("qualifier", "(Rio J.)"),
                    ],
                ),
            ],
        };
        let instance = Description {
            classes: Vec::new(),
            statements: vec![
                title("KeyTitle", vec![part("mainTitle", "Revista brasileira")]),
                title(
                    "VariantTitle",
                    vec![
                        part("mainTitle", "The skin"),
                        part("variantType", "translated"),
                    ],
                ),
                title(
                    "VariantTitle",
                    vec![
                        part("mainTitle", "Dom Casmurro"),
                        part("partNumber", "v. 1"),
                        part("variantType", "spine"),
                    ],
                ),
            ],
        };
        let mut out = Record {
            leader: *b"00000nam a2200000 i 4500",
            fields: Vec::new(),
        };
        super::super::described::write(&mut out, &TITLES, &work, &instance, None);
        let tags: Vec<[u8; 3]> = out.fields.iter().map(|f| *f.tag().as_bytes()).collect();
        assert_eq!(
            tags,
            [
                *b"210", *b"222", *b"242", *b"243", *b"246", *b"246", *b"247"
            ]
        );
        let marc = Marc(&out);
        assert_eq!(of_owner(&marc, Owner::Instance), instance.statements);
        let mut back = of_owner(&marc, Owner::Work);
        back.sort_by_key(|statement| format!("{statement:?}"));
        let mut expected = work.statements.clone();
        expected.sort_by_key(|statement| format!("{statement:?}"));
        assert_eq!(back, expected);
        // Unchanged titles leave the record alone.
        let before = out.clone();
        super::super::described::write(
            &mut out,
            &TITLES,
            &work,
            &instance,
            Some((&work, &instance)),
        );
        assert_eq!(out, before);
    }
}
