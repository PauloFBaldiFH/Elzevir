// SPDX-License-Identifier: GPL-3.0-or-later
//! Physical details, dimensions and accompanying material back to `300 $b $c $e`
//! (bibframe-completo.md, item 29, part 5e), after the way back of the Library of Congress
//! (`bibframe2marc`: `06-3XX.xml`).
//!
//! They share the field with the extent, so they are compared with what the record being
//! written holds and only their subfields are replaced when they differ.

use elzevir_domain::statement::{Description, Statement, Value};
use elzevir_marc::{Field, Record, Subfield, Tag};

use super::super::Marc;
use super::super::notes::note_code;
use super::super::physical::{ACCOMPANYING, PHYSICAL, of_instance};
use super::fields::{first_index, insert_in_tag_order};

/// Writes `$b`, `$c` and `$e` of the first `300` when they differ from the Instance's.
pub(super) fn write(out: &mut Record, instance: &Description) {
    let wanted: Vec<(u8, &Statement)> = instance
        .statements
        .iter()
        .filter_map(|statement| code_of(statement).map(|code| (code, statement)))
        .collect();
    let current = of_instance(&Marc(out));
    let same =
        wanted.len() == current.len() && wanted.iter().zip(&current).all(|((_, w), c)| *w == c);
    if same {
        return;
    }
    // `$b`, `$c`, `$e` in that order, as MARC lists them.
    let mut subfields: Vec<Subfield> = wanted
        .iter()
        .filter_map(|(code, statement)| {
            text_of(statement).map(|text| Subfield {
                code: *code,
                value: text.as_bytes().to_vec(),
            })
        })
        .collect();
    subfields.sort_by_key(|subfield| subfield.code);
    let Some(index) = first_index(out, *b"300") else {
        if !subfields.is_empty() {
            insert_in_tag_order(
                out,
                vec![Field::Data {
                    tag: Tag::new(*b"300"),
                    indicators: *b"  ",
                    subfields,
                }],
            );
        }
        return;
    };
    let Some(Field::Data {
        subfields: present, ..
    }) = out.fields.get_mut(index)
    else {
        return;
    };
    present.retain(|subfield| !matches!(subfield.code, b'b' | b'c' | b'e'));
    // After the extent, before the materials specified and the links.
    let at = present
        .iter()
        .rposition(|subfield| matches!(subfield.code, b'a' | b'f' | b'g'))
        .map_or(0, |last| last + 1);
    present.splice(at..at, subfields);
    if present.is_empty() {
        out.fields.remove(index);
    }
}

/// The `300` subfield of a statement of the physical description.
fn code_of(statement: &Statement) -> Option<u8> {
    match (&*statement.property, &statement.value) {
        ("dimensions", Value::Literal { .. }) => Some(b'c'),
        ("note", Value::Resource(note)) => {
            let label = note.statements.iter().find_map(|s| match &s.value {
                Value::Literal { text, .. } if s.property == "noteType" => Some(text.as_str()),
                _ => None,
            })?;
            match note_code(label)? {
                PHYSICAL => Some(b'b'),
                ACCOMPANYING => Some(b'e'),
                _ => None,
            }
        }
        _ => None,
    }
}

fn text_of(statement: &Statement) -> Option<&str> {
    match &statement.value {
        Value::Literal { text, .. } => Some(text),
        Value::Resource(note) => note.label.as_deref(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::notes::note;
    use super::super::super::physical::dimensions;
    use super::super::fields::data_field;
    use super::*;

    #[test]
    fn details_dimensions_and_material_come_back_next_to_the_extent() {
        let source = Record {
            leader: *b"00000nam a2200000 i 4500",
            fields: vec![data_field(
                *b"300",
                *b"  ",
                &[(b'a', "195 p. :"), (b'b', "il. ;"), (b'c', "19 cm.")],
            )],
        };
        let mut instance = Description {
            classes: Vec::new(),
            statements: of_instance(&Marc(&source)),
        };
        let mut out = source.clone();
        write(&mut out, &instance);
        assert_eq!(out, source);
        instance.statements = vec![
            note(PHYSICAL, "color.".to_owned()),
            dimensions("23 cm".to_owned()),
            note(ACCOMPANYING, "1 mapa".to_owned()),
        ];
        write(&mut out, &instance);
        assert_eq!(
            out.fields,
            [data_field(
                *b"300",
                *b"  ",
                &[
                    (b'a', "195 p. :"),
                    (b'b', "color."),
                    (b'c', "23 cm"),
                    (b'e', "1 mapa")
                ]
            )]
        );
        assert_eq!(of_instance(&Marc(&out)), instance.statements);
        let mut empty = Record {
            leader: source.leader,
            fields: Vec::new(),
        };
        write(&mut empty, &instance);
        assert_eq!(of_instance(&Marc(&empty)), instance.statements);
    }
}
