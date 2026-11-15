// SPDX-License-Identifier: GPL-3.0-or-later
//! Electronic locations of the Instance (docs/specs/catalogo/bibframe-completo.md, item 29,
//! part 5e), after the Library of Congress conversions (`marc2bibframe2`:
//! `ConvSpec-841-887.xsl`; `bibframe2marc`: `10-8XX.xml`).
//!
//! An `856` with second indicator `2` (a related resource) becomes `bf:supplementaryContent`:
//! a `bf:SupplementaryContent` with the `bf:electronicLocator` and the notes of `$3`, `$z`
//! and `$y`. Any other `856` becomes a `bf:electronicLocator` of the Instance; the way back
//! of the LC writes no note for it, so `$z` and `$y` stay only in the source record.

use elzevir_domain::statement::{Resource, Statement, Value};

use super::titles::Owner;
use super::{DataField, Marc, text};

/// The fields this module reads.
pub(super) const TAGS: [&[u8; 3]; 1] = [b"856"];

/// The statements of the Instance, in record order.
pub(super) fn of_instance(marc: &Marc<'_>) -> Vec<Statement> {
    marc.data_in(&TAGS)
        .filter_map(|(tag, field)| of_field(*tag, field))
        .map(|(_, statement)| statement)
        .collect()
}

/// The statement one `856` gives, from its first web address; `None` without one. A
/// repeated `$u` names a mirror of the same resource, which the model does not keep.
pub(super) fn of_field(tag: [u8; 3], field: DataField<'_>) -> Option<(Owner, Statement)> {
    if tag != *b"856" {
        return None;
    }
    let address = field
        .values(b'u')
        .filter_map(text::trimmed)
        .find(|u| is_web(u))?;
    let statement = if field.indicators[1] == b'2' {
        let mut statements = vec![locator(address)];
        statements.extend(
            field
                .subfields
                .iter()
                .filter(|subfield| matches!(subfield.code, b'3' | b'z' | b'y'))
                .filter_map(|subfield| text::clean(&subfield.value))
                .map(note),
        );
        Statement {
            property: "supplementaryContent".to_owned(),
            value: Value::Resource(Resource {
                class: "SupplementaryContent".to_owned(),
                label: None,
                statements,
            }),
        }
    } else {
        locator(address)
    };
    Some((Owner::Instance, statement))
}

/// Whether the model can keep `address`: it holds web addresses only.
fn is_web(address: &str) -> bool {
    address.starts_with("http://") || address.starts_with("https://")
}

/// A `bf:electronicLocator`.
pub(super) fn locator(address: String) -> Statement {
    Statement {
        property: "electronicLocator".to_owned(),
        value: Value::Iri(address),
    }
}

/// A `bf:Note` without a type.
pub(super) fn note(label: String) -> Statement {
    Statement {
        property: "note".to_owned(),
        value: Value::Resource(Resource {
            class: "Note".to_owned(),
            label: Some(label),
            statements: Vec::new(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use elzevir_marc::{Field, Record, Subfield, Tag};

    use super::*;

    fn field(indicators: [u8; 2], subfields: &[(u8, &str)]) -> Field {
        Field::Data {
            tag: Tag::new(*b"856"),
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
    fn locations_and_related_resources_become_statements() {
        let record = Record {
            leader: *b"00000nam a2200000 i 4500",
            fields: vec![
                field(
                    *b"4 ",
                    &[
                        (b'a', "/usr/local/x/"),
                        (b'z', "Imagem"),
                        (b'u', "https://x.org/a.png"),
                    ],
                ),
                field(
                    *b"42",
                    &[(b'u', "http://x.org/sumario"), (b'y', "Sumário.")],
                ),
                field(*b"4 ", &[(b'u', "/local/path")]),
            ],
        };
        assert_eq!(
            of_instance(&Marc(&record)),
            [
                locator("https://x.org/a.png".to_owned()),
                Statement {
                    property: "supplementaryContent".to_owned(),
                    value: Value::Resource(Resource {
                        class: "SupplementaryContent".to_owned(),
                        label: None,
                        statements: vec![
                            locator("http://x.org/sumario".to_owned()),
                            note("Sumário".to_owned()),
                        ],
                    }),
                },
            ]
        );
    }
}
