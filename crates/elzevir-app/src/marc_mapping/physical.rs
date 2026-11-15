// SPDX-License-Identifier: GPL-3.0-or-later
//! The physical description of the Instance (docs/specs/catalogo/bibframe-completo.md, item
//! 29, part 5e), after the Library of Congress conversions (`marc2bibframe2`:
//! `ConvSpec-3XX.xsl`; `bibframe2marc`: `06-3XX.xml`).
//!
//! `300 $a $f $g` is the extent, which keeps its own field. `$b` becomes a `bf:note` of type
//! physical details, `$c` `bf:dimensions` and `$e` a `bf:note` of type accompanying
//! material, all of the Instance. Only the first `300` is read, as for the extent.

use elzevir_domain::statement::{Statement, Value};

use super::notes::note;
use super::{DataField, Marc, text};

/// The `mnotetype` code of the physical details (`$b`).
pub(super) const PHYSICAL: &str = "physical";
/// The `mnotetype` code of the accompanying material (`$e`).
pub(super) const ACCOMPANYING: &str = "accmat";

/// The extent: `$a`, `$f` and `$g` in field order, without the punctuation that ends them.
pub(super) fn extent(field: DataField<'_>) -> Option<String> {
    let parts: Vec<String> = field
        .subfields
        .iter()
        .filter(|subfield| matches!(subfield.code, b'a' | b'f' | b'g'))
        .filter_map(|subfield| text::trimmed(&subfield.value))
        .collect();
    chop(&parts.join(" "), false)
}

/// The extent as the model kept it before part 5e: `$a`, `$b` and `$c` as written.
pub(super) fn former_extent(field: DataField<'_>) -> Option<String> {
    field.raw_joined(b"abc")
}

/// The statements of the Instance, in the order `$b`, `$c`, `$e`.
pub(super) fn of_instance(marc: &Marc<'_>) -> Vec<Statement> {
    let Some(field) = marc.first(b"300") else {
        return Vec::new();
    };
    let values = |code: u8, dimensions: bool| {
        field
            .values(code)
            .filter_map(text::trimmed)
            .filter_map(move |value| chop(&value, dimensions))
    };
    let mut statements: Vec<Statement> = values(b'b', false)
        .map(|text| note(PHYSICAL, text))
        .collect();
    statements.extend(values(b'c', true).map(dimensions));
    statements.extend(values(b'e', false).map(|text| note(ACCOMPANYING, text)));
    statements
}

/// A `bf:dimensions`.
pub(super) fn dimensions(text: String) -> Statement {
    Statement {
        property: "dimensions".to_owned(),
        value: Value::Literal {
            text,
            language: None,
        },
    }
}

/// `text` without the ISBD punctuation that closes it (` :`, ` ;`, ` +`, ` -` and the
/// like); a dimension also loses its final period, as in the LC conversion.
fn chop(text: &str, dimensions: bool) -> Option<String> {
    let mut text = text.trim();
    loop {
        let before = text.len();
        text = text.trim_end_matches([' ', ':', ';', ',', '/', '=', '+', '-']);
        if dimensions {
            text = text.trim_end_matches('.');
        }
        if text.len() == before {
            break;
        }
    }
    (!text.is_empty()).then(|| text.to_owned())
}

#[cfg(test)]
mod tests {
    use elzevir_marc::{Field, Record, Subfield, Tag};

    use super::*;

    fn record(subfields: &[(u8, &str)]) -> Record {
        Record {
            leader: *b"00000nam a2200000 i 4500",
            fields: vec![Field::Data {
                tag: Tag::new(*b"300"),
                indicators: *b"  ",
                subfields: subfields
                    .iter()
                    .map(|(code, value)| Subfield {
                        code: *code,
                        value: value.as_bytes().to_vec(),
                    })
                    .collect(),
            }],
        }
    }

    #[test]
    fn the_extent_keeps_its_field_and_the_rest_becomes_statements() {
        let record = record(&[
            (b'a', "82, [3]p. :"),
            (b'b', "bil. ;"),
            (b'c', "21cm. -"),
            (b'e', "1 CD-ROM."),
        ]);
        let marc = Marc(&record);
        let field = marc.first(b"300").unwrap();
        assert_eq!(extent(field).as_deref(), Some("82, [3]p."));
        assert_eq!(
            former_extent(field).as_deref(),
            Some("82, [3]p. : bil. ; 21cm. -")
        );
        assert_eq!(
            of_instance(&marc),
            [
                note(PHYSICAL, "bil.".to_owned()),
                dimensions("21cm".to_owned()),
                note(ACCOMPANYING, "1 CD-ROM.".to_owned()),
            ]
        );
    }
}
