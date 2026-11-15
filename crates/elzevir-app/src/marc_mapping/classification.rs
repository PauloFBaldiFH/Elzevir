// SPDX-License-Identifier: GPL-3.0-or-later
//! The classifications of the Work (docs/specs/catalogo/bibframe-completo.md, item 29, part
//! 5f), after the Library of Congress conversions (`marc2bibframe2`:
//! `ConvSpec-050-088.xsl`; `bibframe2marc`: `03-050-088.xml`).
//!
//! Each of `050`, `080`, `082` and `084` becomes one `bf:classification` of the Work: a
//! `bf:ClassificationLcc`, `bf:ClassificationUdc`, `bf:ClassificationDdc` or plain
//! `bf:Classification`, with each `$a` as `bf:classificationPortion`, the first `$b` as
//! `bf:itemPortion`, `$2` as `bf:source` (the edition of the DDC or UDC, the scheme of
//! `084`), the first indicator of `080` and `082` as `bf:edition` (`full`, `abridged`) and
//! the second indicator `0` of `050` and `082` as the Library of Congress for `bf:assigner`.
//! The Work keeps its main classification number in its own field, and the statement of
//! that classification holds the rest of its field; the way back puts the number back in.

use elzevir_domain::statement::{Resource, Statement, Value};

use super::{DataField, Marc, text};

/// The classification fields and the class each one gives, in tag order.
pub(super) const CLASSIFICATIONS: [(&[u8; 3], &str); 4] = [
    (b"050", "ClassificationLcc"),
    (b"080", "ClassificationUdc"),
    (b"082", "ClassificationDdc"),
    (b"084", "Classification"),
];

/// The code of the Library of Congress as an assigner (MARC organization code).
pub(super) const LIBRARY_OF_CONGRESS: &str = "dlc";

/// The classifications of the Work, in field order, without the main number, which the
/// Work keeps in its own field (see [`main`]). The statement of the main number keeps the
/// rest of its field and goes when nothing is left.
pub(super) fn of_work(marc: &Marc<'_>) -> Vec<Statement> {
    let mut statements = all(marc);
    if let Some(at) = main_index(&statements) {
        let empty = match statements.get_mut(at).map(|s| &mut s.value) {
            Some(Value::Resource(main)) => {
                if let Some(first) = main
                    .statements
                    .iter()
                    .position(|s| s.property == "classificationPortion")
                {
                    main.statements.remove(first);
                }
                main.statements.is_empty()
            }
            _ => false,
        };
        if empty {
            statements.remove(at);
        }
    }
    statements
}

/// The Work's main classification number and its scheme (`ddc`, `udc`, `lcc`): the first
/// number of the first DDC classification, or else of the UDC, or else of the LCC.
pub(super) fn main(marc: &Marc<'_>) -> (Option<String>, Option<String>) {
    let statements = all(marc);
    let Some(Value::Resource(main)) = main_index(&statements)
        .and_then(|at| statements.get(at))
        .map(|s| &s.value)
    else {
        return (None, None);
    };
    let number = main.statements.iter().find_map(|s| match &s.value {
        Value::Literal { text, .. } if s.property == "classificationPortion" => Some(text.clone()),
        _ => None,
    });
    let scheme = MAIN_SCHEMES
        .iter()
        .find(|(class, _)| *class == main.class)
        .map(|(_, scheme)| (*scheme).to_owned());
    (number, scheme)
}

/// The classes that can give the main number, in order of preference, with their scheme.
pub(super) const MAIN_SCHEMES: [(&str, &str); 3] = [
    ("ClassificationDdc", "ddc"),
    ("ClassificationUdc", "udc"),
    ("ClassificationLcc", "lcc"),
];

fn main_index(statements: &[Statement]) -> Option<usize> {
    MAIN_SCHEMES.iter().find_map(|(class, _)| {
        statements
            .iter()
            .position(|s| matches!(&s.value, Value::Resource(r) if r.class == *class))
    })
}

/// Every classification field, in field order.
fn all(marc: &Marc<'_>) -> Vec<Statement> {
    let tags: Vec<&[u8; 3]> = CLASSIFICATIONS.iter().map(|(tag, _)| *tag).collect();
    marc.data_in(&tags)
        .filter_map(|(tag, field)| of_field(*tag, field))
        .collect()
}

/// The classification one field describes; `None` for a field without a number.
pub(super) fn of_field(tag: [u8; 3], field: DataField<'_>) -> Option<Statement> {
    let class = CLASSIFICATIONS
        .iter()
        .find(|(t, _)| **t == tag)
        .map(|(_, class)| *class)?;
    let mut statements: Vec<Statement> = field
        .values(b'a')
        .filter_map(text::trimmed)
        .map(|number| literal("classificationPortion", number))
        .collect();
    if statements.is_empty() {
        return None;
    }
    statements.extend(
        field
            .values(b'b')
            .find_map(text::trimmed)
            .map(|item| literal("itemPortion", item)),
    );
    statements.extend(
        field
            .values(b'2')
            .filter_map(text::trimmed)
            .map(|source| resource("source", "Source", vec![literal("code", source)])),
    );
    if matches!(&tag, b"080" | b"082") {
        let edition = match field.indicators[0] {
            b'0' => Some("full"),
            b'1' => Some("abridged"),
            _ => None,
        };
        statements.extend(edition.map(|e| literal("edition", e.to_owned())));
    }
    if matches!(&tag, b"050" | b"082") && field.indicators[1] == b'0' {
        statements.push(resource(
            "assigner",
            "Agent",
            vec![literal("code", LIBRARY_OF_CONGRESS.to_owned())],
        ));
    }
    Some(resource("classification", class, statements))
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

fn resource(property: &str, class: &str, statements: Vec<Statement>) -> Statement {
    Statement {
        property: property.to_owned(),
        value: Value::Resource(Resource {
            class: class.to_owned(),
            label: None,
            statements,
        }),
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
    fn each_classification_field_becomes_a_classification_of_its_scheme() {
        let record = Record {
            leader: *b"00000nam a2200000 a 4500",
            fields: vec![
                field(*b"050", *b" 0", &[(b'a', "PC5345"), (b'b', ".A5")]),
                field(*b"080", *b"  ", &[(b'a', "316.7"), (b'2', "1987")]),
                field(*b"082", *b"14", &[(b'a', "320.5"), (b'2', "23")]),
                field(*b"084", *b"  ", &[(b'a', "340.08"), (b'2', "cddir")]),
                field(*b"082", *b"04", &[(b'2', "23")]),
            ],
        };
        let statements = all(&Marc(&record));
        let classes: Vec<&str> = statements
            .iter()
            .filter_map(|s| match &s.value {
                Value::Resource(r) => Some(r.class.as_str()),
                _ => None,
            })
            .collect();
        // A field without `$a` names no number and gives nothing.
        assert_eq!(
            classes,
            [
                "ClassificationLcc",
                "ClassificationUdc",
                "ClassificationDdc",
                "Classification"
            ]
        );
        let Some(Value::Resource(ddc)) = statements.get(2).map(|s| &s.value) else {
            panic!("a DDC classification");
        };
        let properties: Vec<&str> = ddc.statements.iter().map(|s| s.property.as_str()).collect();
        assert_eq!(properties, ["classificationPortion", "source", "edition"]);
        assert_eq!(ddc.statements[2], literal("edition", "abridged".to_owned()));
        let Some(Value::Resource(lcc)) = statements.first().map(|s| &s.value) else {
            panic!("an LCC classification");
        };
        let properties: Vec<&str> = lcc.statements.iter().map(|s| s.property.as_str()).collect();
        assert_eq!(
            properties,
            ["classificationPortion", "itemPortion", "assigner"]
        );
    }
}
