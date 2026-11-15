// SPDX-License-Identifier: GPL-3.0-or-later
//! The number of a national bibliographic agency (`016`, docs/specs/catalogo/
//! bibframe-completo.md, item 29, part 5f), after the Library of Congress conversions
//! (`marc2bibframe2`: `ConvSpec-010-048.xsl`; `bibframe2marc`: `02-010-048.xml`).
//!
//! Each `016 $a` becomes a `bf:identifiedBy` of the Instance: a `bf:Local` with the number
//! and a `bf:source`, the agency of `$2` (its code), or Library and Archives Canada for a
//! blank first indicator without `$2`. The LC conversion takes a blank first indicator as
//! Library and Archives Canada even with `$2`; here `$2` wins, since it names the agency.
//! A number without an agency, and the cancelled ones of `$z`, stay in the source record.

use elzevir_domain::statement::{Resource, Statement, Value};

use super::titles::Owner;
use super::{DataField, Marc, text};

/// The fields this module reads.
pub(super) const TAGS: [&[u8; 3]; 1] = [b"016"];

/// The agency of a blank first indicator.
pub(super) const CANADA: &str = "Library and Archives Canada";

/// The statements of the Instance, in record order.
pub(super) fn of_instance(marc: &Marc<'_>) -> Vec<Statement> {
    marc.data_in(&TAGS)
        .filter_map(|(tag, field)| of_field(*tag, field))
        .map(|(_, statement)| statement)
        .collect()
}

/// The statement one `016` gives; `None` without a number or an agency.
pub(super) fn of_field(tag: [u8; 3], field: DataField<'_>) -> Option<(Owner, Statement)> {
    if tag != *b"016" {
        return None;
    }
    let number = field.values(b'a').find_map(text::trimmed)?;
    let source = match field.values(b'2').find_map(text::trimmed) {
        Some(code) => Resource {
            class: "Source".to_owned(),
            label: None,
            statements: vec![Statement {
                property: "code".to_owned(),
                value: Value::Literal {
                    text: code,
                    language: None,
                },
            }],
        },
        None if field.indicators[0] == b' ' => Resource {
            class: "Source".to_owned(),
            label: Some(CANADA.to_owned()),
            statements: Vec::new(),
        },
        None => return None,
    };
    Some((
        Owner::Instance,
        Statement {
            property: "identifiedBy".to_owned(),
            value: Value::Resource(Resource {
                class: "Local".to_owned(),
                label: Some(number),
                statements: vec![Statement {
                    property: "source".to_owned(),
                    value: Value::Resource(source),
                }],
            }),
        },
    ))
}
