// SPDX-License-Identifier: GPL-3.0-or-later
//! The administrative metadata of the Instance back to Leader/05 and 18, `001`, `003`, `005`,
//! `008/00-05` and `040` (bibframe-completo.md, item 29, part 5e), after the way back of
//! the Library of Congress (`bibframe2marc`: `01-001-008.xml` and `02-010-048.xml`).
//!
//! It is compared with what the record being written holds and written only when it
//! differs; the rest of `008` and the other subfields of `040` stay as they are.

use elzevir_domain::statement::{Description, Resource, Statement, Value};
use elzevir_marc::{Field, Record, Subfield, Tag};

use super::super::Marc;
use super::super::admin::{in_leader, of_instance};
use super::fields::{first_index, insert_in_tag_order, set_008};

/// Writes the administrative metadata of `instance` when it differs from `out`'s.
pub(super) fn write(out: &mut Record, instance: &Description) {
    let wanted: Vec<&Statement> = instance
        .statements
        .iter()
        .filter(|s| s.property == "adminMetadata")
        .collect();
    let current = of_instance(&Marc(out));
    if wanted.len() == current.len() && wanted.iter().zip(&current).all(|(w, c)| *w == c) {
        return;
    }
    let empty = Vec::new();
    let admin = wanted
        .iter()
        .find_map(|s| match &s.value {
            Value::Resource(admin) => Some(&admin.statements),
            _ => None,
        })
        .unwrap_or(&empty);
    let first = |property: &str| admin.iter().find(|s| s.property == property);
    if let Some(Value::Concept { code, .. }) = first("status").map(|s| &s.value)
        && let [byte] = code.as_bytes()
        && let Some(position) = out.leader.get_mut(5)
    {
        *position = *byte;
    }
    // ISBD is what the LC way back writes when nothing says otherwise.
    if let Some(position) = out.leader.get_mut(18) {
        *position = if follows(admin, "aacr") { b'a' } else { b'i' };
    }
    let local = first("identifiedBy").and_then(|s| match &s.value {
        Value::Resource(local) if local.class == "Local" => Some(local),
        _ => None,
    });
    set_control(out, *b"001", local.and_then(|l| l.label.clone()));
    set_control(
        out,
        *b"003",
        local
            .and_then(|l| l.statements.iter().find(|s| s.property == "assigner"))
            .and_then(code_of)
            .map(str::to_owned),
    );
    set_control(
        out,
        *b"005",
        literal(first("changeDate")).and_then(marc_005),
    );
    if let Some(entered) = literal(first("creationDate")).and_then(marc_008_date) {
        set_008(out, 0, entered.as_bytes());
    }
    write_040(out, admin);
}

/// Whether the administrative metadata `description` holds names the description
/// conventions `code`.
pub(super) fn description_follows(description: &Description, code: &str) -> bool {
    description
        .statements
        .iter()
        .filter(|s| s.property == "adminMetadata")
        .any(|s| match &s.value {
            Value::Resource(admin) => follows(&admin.statements, code),
            _ => false,
        })
}

fn follows(admin: &[Statement], code: &str) -> bool {
    admin
        .iter()
        .filter(|s| s.property == "descriptionConventions")
        .any(|s| code_of(s) == Some(code))
}

/// `2026-08-23T19:29:13` → `20260823192913.0`; a date without a time gets midnight.
fn marc_005(date: &str) -> Option<String> {
    let digits: String = date.chars().filter(char::is_ascii_digit).collect();
    match digits.len() {
        14 => Some(format!("{digits}.0")),
        8 => Some(format!("{digits}000000.0")),
        _ => None,
    }
}

/// `2025-03-21` → `250321`.
fn marc_008_date(date: &str) -> Option<String> {
    let digits: String = date.chars().filter(char::is_ascii_digit).collect();
    (digits.len() == 8).then(|| digits.get(2..).unwrap_or_default().to_owned())
}

/// `040 $a $b $c $d $e`; `$c`, the transcribing agency, is the cataloguing agency, as the
/// LC way back writes it.
fn write_040(out: &mut Record, admin: &[Statement]) {
    let codes = |property: &str| -> Vec<&str> {
        admin
            .iter()
            .filter(|s| s.property == property)
            .filter_map(code_of)
            .collect()
    };
    let assigners = codes("assigner");
    let mut subfields: Vec<Subfield> = Vec::new();
    let mut push = |code: u8, values: &[&str]| {
        subfields.extend(values.iter().map(|value| Subfield {
            code,
            value: value.as_bytes().to_vec(),
        }));
    };
    push(b'a', &assigners);
    push(b'b', &codes("descriptionLanguage"));
    push(b'c', assigners.get(..1).unwrap_or_default());
    push(b'd', &codes("descriptionModifier"));
    // Leader/18 says AACR 2 and ISBD; `$e` names the others.
    let conventions: Vec<&str> = codes("descriptionConventions")
        .into_iter()
        .filter(|code| !in_leader(code))
        .collect();
    push(b'e', &conventions);
    match first_index(out, *b"040") {
        Some(index) => {
            let Some(Field::Data {
                subfields: present, ..
            }) = out.fields.get_mut(index)
            else {
                return;
            };
            present.retain(|s| !matches!(s.code, b'a'..=b'e'));
            present.splice(0..0, subfields);
            if present.is_empty() {
                out.fields.remove(index);
            }
        }
        None if !subfields.is_empty() => insert_in_tag_order(
            out,
            vec![Field::Data {
                tag: Tag::new(*b"040"),
                indicators: *b"  ",
                subfields,
            }],
        ),
        None => {}
    }
}

/// Replaces the control field `tag`, inserts it in tag order, or removes it for `None`.
fn set_control(out: &mut Record, tag: [u8; 3], value: Option<String>) {
    let index = first_index(out, tag);
    match (index, value) {
        (Some(index), Some(value)) => {
            if let Some(field) = out.fields.get_mut(index) {
                *field = Field::Control {
                    tag: Tag::new(tag),
                    value: value.into_bytes(),
                };
            }
        }
        (Some(index), None) => {
            out.fields.remove(index);
        }
        (None, Some(value)) => insert_in_tag_order(
            out,
            vec![Field::Control {
                tag: Tag::new(tag),
                value: value.into_bytes(),
            }],
        ),
        (None, None) => {}
    }
}

/// The code of an agent, language or convention: a concept's code or the `bf:code` of a
/// resource.
fn code_of(statement: &Statement) -> Option<&str> {
    match &statement.value {
        Value::Concept { code, .. } => Some(code),
        Value::Resource(Resource { statements, .. }) => {
            literal(statements.iter().find(|s| s.property == "code"))
        }
        _ => None,
    }
}

fn literal(statement: Option<&Statement>) -> Option<&str> {
    match statement.map(|s| &s.value) {
        Some(Value::Literal { text, .. }) => Some(text),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::fields::data_field;
    use super::*;

    fn control(tag: [u8; 3], value: &str) -> Field {
        Field::Control {
            tag: Tag::new(tag),
            value: value.as_bytes().to_vec(),
        }
    }

    #[test]
    fn admin_metadata_comes_back_to_leader_control_fields_and_040() {
        let source = Record {
            leader: *b"00000cam a2200000 i 4500",
            fields: vec![
                control(*b"001", "153256"),
                control(*b"003", "BR-RjBN"),
                control(*b"005", "20260823192913.0"),
                control(*b"008", "250321s1995    bl            000 0 por d"),
                data_field(
                    *b"040",
                    *b"  ",
                    &[(b'a', "BR-X"), (b'b', "por"), (b'c', "BR-X"), (b'e', "rda")],
                ),
            ],
        };
        let instance = Description {
            classes: Vec::new(),
            statements: of_instance(&Marc(&source)),
        };
        // Unchanged metadata leaves the record alone.
        let mut out = source.clone();
        write(&mut out, &instance);
        assert_eq!(out, source);
        // Written from nothing, it gives the same fields.
        let mut empty = Record {
            leader: *b"00000nam a2200000 i 4500",
            fields: Vec::new(),
        };
        write(&mut empty, &instance);
        assert_eq!(of_instance(&Marc(&empty)), instance.statements);
        assert_eq!(empty.leader, source.leader);
        assert_eq!(
            empty
                .fields
                .iter()
                .map(|f| *f.tag().as_bytes())
                .collect::<Vec<_>>(),
            [*b"001", *b"003", *b"005", *b"008", *b"040"]
        );
        assert_eq!(empty.fields.last(), source.fields.last());
    }
}
