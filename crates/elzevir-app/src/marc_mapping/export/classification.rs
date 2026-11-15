// SPDX-License-Identifier: GPL-3.0-or-later
//! The classifications of the Work back to `050`, `080`, `082` and `084`
//! (bibframe-completo.md, item 29, part 5f), after the way back of the Library of Congress
//! (`bibframe2marc`: `03-050-088.xml`).
//!
//! The Work's main classification number, which the form edits, goes back into the
//! statement that holds the rest of its field (the first of its scheme without a number),
//! or into a new one; without the number, that statement goes too.

use elzevir_domain::statement::{Resource, Statement, Value};
use elzevir_marc::{Field, Record};

use super::super::classification::{CLASSIFICATIONS, LIBRARY_OF_CONGRESS, of_field};
use super::fields::data_field;
use super::{mapped, rebuild};
use crate::catalog::NewRecord;

/// Rewrites the classification fields when the classifications differ from those of
/// `before`; a classification that did not change keeps its original field.
pub(super) fn write(out: &mut Record, model: &NewRecord, before: Option<&NewRecord>) {
    let wanted = wanted(model);
    if before.is_some_and(|before| self::wanted(before) == wanted) {
        return;
    }
    let tags: Vec<&[u8; 3]> = CLASSIFICATIONS.iter().map(|(tag, _)| *tag).collect();
    let current = mapped(out, &tags, of_field);
    rebuild(out, &current, &wanted, field_of, |_, _| None);
}

/// The classifications to write, in tag order, with the main number in place.
fn wanted(model: &NewRecord) -> Vec<Statement> {
    let mut wanted: Vec<Statement> = model
        .work_description
        .statements
        .iter()
        .filter(|s| s.property == "classification" && tag_of(s).is_some())
        .cloned()
        .collect();
    let class = match model.classification_scheme.as_deref() {
        Some("udc") => "ClassificationUdc",
        Some("lcc") => "ClassificationLcc",
        _ => "ClassificationDdc",
    };
    // The rest of the main classification is the statement of its scheme without a number
    // of its own; when the field had only the number, there is none.
    let found = wanted
        .iter()
        .position(|s| class_of(s) == Some(class) && !has_number(s));
    match (found, model.classification.as_deref()) {
        (Some(at), Some(number)) => {
            if let Some(Statement {
                value: Value::Resource(resource),
                ..
            }) = wanted.get_mut(at)
            {
                set_number(resource, number);
            }
        }
        (Some(at), None) => {
            wanted.remove(at);
        }
        (None, Some(number)) => {
            let mut resource = Resource {
                class: class.to_owned(),
                label: None,
                statements: Vec::new(),
            };
            set_number(&mut resource, number);
            // First of all, so that it stays the first of its tag after sorting.
            wanted.insert(
                0,
                Statement {
                    property: "classification".to_owned(),
                    value: Value::Resource(resource),
                },
            );
        }
        (None, None) => {}
    }
    // Fields go in tag order; `sort_by_key` keeps the order within a tag.
    wanted.sort_by_key(tag_of);
    wanted
}

/// Puts `number` before the rest of the main classification.
fn set_number(resource: &mut Resource, number: &str) {
    resource.statements.insert(
        0,
        Statement {
            property: "classificationPortion".to_owned(),
            value: Value::Literal {
                text: number.to_owned(),
                language: None,
            },
        },
    );
}

fn has_number(statement: &Statement) -> bool {
    matches!(&statement.value, Value::Resource(resource)
        if resource.statements.iter().any(|s| s.property == "classificationPortion"))
}

fn class_of(statement: &Statement) -> Option<&str> {
    match &statement.value {
        Value::Resource(resource) => Some(&resource.class),
        _ => None,
    }
}

fn tag_of(statement: &Statement) -> Option<[u8; 3]> {
    let class = class_of(statement)?;
    CLASSIFICATIONS
        .iter()
        .find(|(_, c)| *c == class)
        .map(|(tag, _)| **tag)
}

/// The field of one classification: its numbers, item part, assigner, then source. The
/// first indicator of `080` and `082` is the edition, blank without one: the LC way back
/// writes `7` (an edition named in `$2`), which would say what the description does not,
/// and a blank comes back as no edition, as the sources that leave it blank. The second indicator of `050` and `082` says
/// whether the Library of Congress assigned it.
fn field_of(statement: &Statement) -> Field {
    let tag = tag_of(statement).unwrap_or(*b"082");
    let Value::Resource(resource) = &statement.value else {
        return data_field(tag, *b"  ", &[]);
    };
    let texts = |property: &str| -> Vec<&str> {
        resource
            .statements
            .iter()
            .filter(|s| s.property == property)
            .filter_map(|s| match &s.value {
                Value::Literal { text, .. } => Some(text.as_str()),
                Value::Resource(inner) => inner.statements.iter().find_map(|s| match &s.value {
                    Value::Literal { text, .. } if s.property == "code" => Some(text.as_str()),
                    _ => None,
                }),
                _ => None,
            })
            .collect()
    };
    let edition = texts("edition").first().copied();
    let by_lc = texts("assigner").contains(&LIBRARY_OF_CONGRESS);
    let first = match (&tag, edition) {
        (b"080" | b"082", Some("full")) => b'0',
        (b"080" | b"082", Some("abridged")) => b'1',
        _ => b' ',
    };
    let second = match &tag {
        b"050" | b"082" if by_lc => b'0',
        b"050" | b"082" => b'4',
        _ => b' ',
    };
    let mut subfields: Vec<(u8, &str)> = texts("classificationPortion")
        .into_iter()
        .map(|number| (b'a', number))
        .collect();
    subfields.extend(texts("itemPortion").first().map(|item| (b'b', *item)));
    subfields.extend(texts("source").into_iter().map(|source| (b'2', source)));
    data_field(tag, [first, second], &subfields)
}

#[cfg(test)]
mod tests {
    use elzevir_marc::Record;

    use super::super::super::marc_to_bibframe;
    use super::*;

    fn record(fields: Vec<Field>) -> Record {
        Record {
            leader: *b"00000nam a2200000 a 4500",
            fields,
        }
    }

    #[test]
    fn classifications_come_back_and_the_main_number_wins() {
        let title = data_field(*b"245", *b"00", &[(b'a', "Política")]);
        let source = record(vec![
            data_field(*b"080", *b"  ", &[(b'a', "316.7"), (b'2', "1987")]),
            data_field(*b"082", *b"14", &[(b'a', "320.5"), (b'2', "23")]),
        ]);
        let mut with_title = source.clone();
        with_title.fields.push(title);
        let model = marc_to_bibframe(&with_title, None).unwrap();
        assert_eq!(model.classification.as_deref(), Some("320.5"));
        let mut out = record(Vec::new());
        write(&mut out, &model, None);
        assert_eq!(out, source);
        // The form changes the main number: only the 082 changes.
        let mut edited = model.clone();
        edited.classification = Some("320.9".to_owned());
        write(&mut out, &edited, Some(&model));
        assert_eq!(
            out,
            record(vec![
                data_field(*b"080", *b"  ", &[(b'a', "316.7"), (b'2', "1987")]),
                data_field(*b"082", *b"14", &[(b'a', "320.9"), (b'2', "23")]),
            ])
        );
        // Without a main number, the 082 goes and the 080 stays.
        let mut cleared = edited.clone();
        cleared.classification = None;
        write(&mut out, &cleared, Some(&edited));
        assert_eq!(
            out,
            record(vec![data_field(
                *b"080",
                *b"  ",
                &[(b'a', "316.7"), (b'2', "1987")]
            )])
        );
    }

    #[test]
    fn a_main_number_alone_in_its_field_does_not_take_over_a_second_one() {
        let fields = |main: &str| {
            vec![
                data_field(*b"082", *b"  ", &[(b'a', main)]),
                data_field(*b"082", *b"  ", &[(b'a', "382.70981")]),
            ]
        };
        let mut source = record(fields("354.74"));
        source
            .fields
            .push(data_field(*b"245", *b"00", &[(b'a', "Comércio")]));
        let model = marc_to_bibframe(&source, None).unwrap();
        let mut edited = model.clone();
        edited.classification = Some("354.8".to_owned());
        let mut out = record(fields("354.74"));
        write(&mut out, &edited, Some(&model));
        // The edited field is written anew, with the second indicator of the LC way back.
        let mut wanted = fields("354.8");
        wanted[0] = data_field(*b"082", *b" 4", &[(b'a', "354.8")]);
        assert_eq!(out, record(wanted));
    }
}
