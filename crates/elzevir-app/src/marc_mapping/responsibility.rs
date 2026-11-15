// SPDX-License-Identifier: GPL-3.0-or-later
//! The statement of responsibility (docs/specs/catalogo/bibframe-completo.md, item 29, part
//! 5f), after the Library of Congress conversions (`marc2bibframe2`:
//! `ConvSpec-200-247not240-Titles.xsl`; `bibframe2marc`: `05-2XX.xml`).
//!
//! `245 $c` becomes `bf:responsibilityStatement` of the Instance, without the ISBD
//! punctuation that closes it; the way back puts the punctuation again.

use elzevir_domain::statement::{Description, Statement, Value};

use super::{Marc, text};

/// The property of the statement of responsibility.
const PROPERTY: &str = "responsibilityStatement";

/// The statements of the Instance: one for each `$c` of the first `245`.
pub(super) fn of_instance(marc: &Marc<'_>) -> Vec<Statement> {
    marc.first(b"245")
        .map(|field| {
            field
                .values(b'c')
                .filter_map(|value| text::clean(chop_area_separator(value)))
                .map(|text| Statement {
                    property: PROPERTY.to_owned(),
                    value: Value::Literal {
                        text,
                        language: None,
                    },
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `value` without the ` -` that older records write before the next ISBD area
/// (`Maria L. Marcílio. -`).
fn chop_area_separator(value: &[u8]) -> &[u8] {
    let value = value.trim_ascii_end();
    value.strip_suffix(b" -").unwrap_or(value)
}

/// The statement of responsibility of an Instance, for the way back.
pub(super) fn of_description(description: &Description) -> Option<&str> {
    description.statements.iter().find_map(|s| match &s.value {
        Value::Literal { text, .. } if s.property == PROPERTY => Some(text.as_str()),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use elzevir_marc::{Field, Record, Subfield, Tag};

    use super::*;

    #[test]
    fn the_statement_of_responsibility_loses_its_closing_period() {
        let record = Record {
            leader: *b"00000nam a2200000 i 4500",
            fields: vec![Field::Data {
                tag: Tag::new(*b"245"),
                indicators: *b"10",
                subfields: [
                    (b'a', "População e Sociedade /"),
                    (b'c', "Maria L. Marcílio. -"),
                ]
                .iter()
                .map(|(code, value)| Subfield {
                    code: *code,
                    value: value.as_bytes().to_vec(),
                })
                .collect(),
            }],
        };
        let statements = of_instance(&Marc(&record));
        let description = Description {
            classes: Vec::new(),
            statements,
        };
        assert_eq!(of_description(&description), Some("Maria L. Marcílio"));
    }
}
