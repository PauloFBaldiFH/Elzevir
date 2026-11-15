// SPDX-License-Identifier: GPL-3.0-or-later
//! The time period of the content back to `045 $a` (bibframe-completo.md, item 29, part
//! 5f), after the way back of the Library of Congress (`bibframe2marc`: `02-010-048.xml`).
//!
//! Like the geographic coverage, the periods share their field with the formatted dates
//! (`$b`, `$c`), so only `$a` is replaced, and only when the periods differ from what the
//! record holds.

use elzevir_domain::statement::{Description, Statement, Value};
use elzevir_marc::{Field, Record, Subfield, Tag};

use super::super::Marc;
use super::super::temporal::{code, of_work};
use super::fields::insert_in_tag_order;

/// Rewrites `045 $a` when the time periods of the Work differ from those of `out`.
pub(super) fn write(out: &mut Record, work: &Description) {
    let wanted: Vec<&Statement> = work
        .statements
        .iter()
        .filter(|s| date(s).and_then(code).is_some())
        .collect();
    let current = of_work(&Marc(out));
    if wanted.len() == current.len() && wanted.iter().zip(&current).all(|(w, c)| *w == c) {
        return;
    }
    let mut codes: Vec<Subfield> = wanted
        .iter()
        .filter_map(|s| date(s).and_then(code))
        .map(|code| Subfield {
            code: b'a',
            value: code.into_bytes(),
        })
        .collect();
    for field in &mut out.fields {
        if let Field::Data { tag, subfields, .. } = field
            && tag.as_bytes() == b"045"
        {
            subfields.retain(|s| s.code != b'a');
            // The periods come first in the first field, before any formatted date.
            for (at, subfield) in std::mem::take(&mut codes).into_iter().enumerate() {
                subfields.insert(at, subfield);
            }
        }
    }
    out.fields.retain(|field| {
        !matches!(field, Field::Data { tag, subfields, .. }
            if tag.as_bytes() == b"045" && subfields.is_empty())
    });
    if !codes.is_empty() {
        insert_in_tag_order(
            out,
            vec![Field::Data {
                tag: Tag::new(*b"045"),
                indicators: *b"  ",
                subfields: codes,
            }],
        );
    }
}

fn date(statement: &Statement) -> Option<&str> {
    match &statement.value {
        Value::Literal { text, .. } if statement.property == "temporalCoverage" => Some(text),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::fields::data_field;
    use super::*;

    #[test]
    fn periods_come_back_and_keep_the_formatted_dates() {
        let source = Record {
            leader: *b"00000nam a2200000 i 4500",
            fields: vec![data_field(
                *b"045",
                *b"  ",
                &[(b'a', "x-x-"), (b'a', "x9x9"), (b'b', "d1995")],
            )],
        };
        let mut work = Description {
            classes: Vec::new(),
            statements: of_work(&Marc(&source)),
        };
        let mut out = source.clone();
        write(&mut out, &work);
        assert_eq!(out, source);
        work.statements.remove(0);
        write(&mut out, &work);
        assert_eq!(
            out.fields,
            [data_field(
                *b"045",
                *b"  ",
                &[(b'a', "x9x9"), (b'b', "d1995")]
            )]
        );
        // Written anew.
        let mut empty = Record {
            leader: source.leader,
            fields: Vec::new(),
        };
        write(&mut empty, &work);
        assert_eq!(
            empty.fields,
            [data_field(*b"045", *b"  ", &[(b'a', "x9x9")])]
        );
    }
}
