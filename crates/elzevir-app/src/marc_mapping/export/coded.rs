// SPDX-License-Identifier: GPL-3.0-or-later
//! Content, media, carrier and issuance back to `336`, `337`, `338` and `334`.
//!
//! A field is written only when the Leader alone would not give the same values, so a
//! record whose description only repeats its Leader gets no new field.

use elzevir_data::vocabulary;
use elzevir_domain::statement::{Description, Statement, Value};
use elzevir_marc::Record;

use super::super::{Marc, description};
use super::fields::{data_field, insert_in_tag_order};

/// One coded field: its tag, the property, and the source code for `$2`.
struct Coded {
    tag: [u8; 3],
    property: &'static str,
    source: &'static str,
    on_work: bool,
}

const CODED: [Coded; 4] = [
    Coded {
        tag: *b"334",
        property: "issuance",
        source: "rdami",
        on_work: false,
    },
    Coded {
        tag: *b"336",
        property: "content",
        source: "rdacontent",
        on_work: true,
    },
    Coded {
        tag: *b"337",
        property: "media",
        source: "rdamedia",
        on_work: false,
    },
    Coded {
        tag: *b"338",
        property: "carrier",
        source: "rdacarrier",
        on_work: false,
    },
];

/// Rewrites the coded fields whose values changed between `before` and the model.
pub(super) fn write(
    out: &mut Record,
    work: &Description,
    instance: &Description,
    before: Option<(&Description, &Description)>,
) {
    for coded in &CODED {
        let pick = |work: &Description, instance: &Description| {
            let description = if coded.on_work { work } else { instance };
            of(description, coded.property)
        };
        let wanted = pick(work, instance);
        if before.is_some_and(|(work, instance)| pick(work, instance) == wanted) {
            continue;
        }
        out.fields
            .retain(|field| field.tag().as_bytes() != &coded.tag);
        let marc = Marc(out);
        let implied = if coded.on_work {
            description::work(&marc)
        } else {
            description::instance(&marc)
        };
        if of(&implied, coded.property) == wanted {
            continue;
        }
        let fields = wanted
            .iter()
            .filter_map(|(scheme, code)| {
                let label = vocabulary::scheme(scheme)?.concept(code)?.label;
                Some(data_field(
                    coded.tag,
                    *b"  ",
                    &[(b'a', label), (b'b', code), (b'2', coded.source)],
                ))
            })
            .collect();
        insert_in_tag_order(out, fields);
    }
}

/// The concepts of `property`, in order.
fn of(description: &Description, property: &str) -> Vec<(String, String)> {
    description
        .statements
        .iter()
        .filter(|statement| statement.property == property)
        .filter_map(|Statement { value, .. }| match value {
            Value::Concept { scheme, code } => Some((scheme.clone(), code.clone())),
            _ => None,
        })
        .collect()
}
