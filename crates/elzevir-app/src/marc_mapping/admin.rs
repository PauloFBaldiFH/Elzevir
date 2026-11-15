// SPDX-License-Identifier: GPL-3.0-or-later
//! The administrative metadata of the description (docs/specs/catalogo/bibframe-completo.md,
//! item 29, part 5e), after the Library of Congress conversions (`marc2bibframe2`:
//! `main.xsl`, `ConvSpec-001-007.xsl` and `ConvSpec-010-048.xsl`; `bibframe2marc`:
//! `01-001-008.xml` and `02-010-048.xml`).
//!
//! One `bf:AdminMetadata` of the Instance gathers what MARC spreads over the Leader, the
//! control fields and `040`: the record status (Leader/05), the control number (`001`) and
//! who assigned it (`003`), the creation date (`008/00-05`), the date of the last change
//! (`005`), and the cataloguing source (`040`).

use elzevir_data::vocabulary;
use elzevir_domain::statement::{Resource, Statement, Value};

use super::{Marc, text};

/// The statements of the Instance: one `bf:adminMetadata`, or none when the record has
/// nothing of it but the status.
pub(super) fn of_instance(marc: &Marc<'_>) -> Vec<Statement> {
    let mut statements = Vec::new();
    let status = char::from(marc.leader_byte(5)).to_string();
    if known("mstatus", &status) {
        statements.push(concept("status", "mstatus", &status));
    }
    if let Some(number) = control_text(marc, *b"001") {
        let mut local = Vec::new();
        if let Some(assigner) = control_text(marc, *b"003") {
            local.push(agent("assigner", assigner));
        }
        statements.push(resource("identifiedBy", "Local", Some(number), local));
    }
    if let Some(field) = marc.first(b"040") {
        statements.extend(
            field
                .values(b'a')
                .filter_map(text::trimmed)
                .map(|a| agent("assigner", a)),
        );
        statements.extend(
            field
                .values(b'b')
                .filter_map(text::trimmed)
                .map(|b| resource("descriptionLanguage", "Language", None, vec![code(b)])),
        );
        statements.extend(
            field
                .values(b'd')
                .filter_map(text::trimmed)
                .map(|d| agent("descriptionModifier", d)),
        );
        statements.extend(field.values(b'e').filter_map(text::trimmed).map(|e| {
            if known("descriptionConventions", &e) {
                concept("descriptionConventions", "descriptionConventions", &e)
            } else {
                resource(
                    "descriptionConventions",
                    "DescriptionConventions",
                    None,
                    vec![code(e)],
                )
            }
        }));
    }
    if let Some(date) = marc.control(*b"008").and_then(creation_date) {
        statements.push(literal("creationDate", date));
    }
    if let Some(date) = marc.control(*b"005").and_then(change_date) {
        statements.push(literal("changeDate", date));
    }
    // The Leader always has a status; alone it describes nothing.
    if statements.iter().all(|s| s.property == "status") {
        return Vec::new();
    }
    vec![resource("adminMetadata", "AdminMetadata", None, statements)]
}

fn control_text(marc: &Marc<'_>, tag: [u8; 3]) -> Option<String> {
    marc.control(tag).and_then(text::trimmed)
}

/// `008/00-05`, `yymmdd`, as `yyyy-mm-dd`; a year below 65 is in this century, as in the
/// LC conversion.
pub(super) fn creation_date(fixed: &[u8]) -> Option<String> {
    let digits = std::str::from_utf8(fixed.get(..6)?).ok()?;
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let year: u8 = digits.get(..2)?.parse().ok()?;
    let century = if year < 65 { "20" } else { "19" };
    Some(format!(
        "{century}{}-{}-{}",
        digits.get(..2)?,
        digits.get(2..4)?,
        digits.get(4..6)?
    ))
}

/// `005`, `yyyymmddhhmmss.f`, as `yyyy-mm-ddThh:mm:ss`; `None` for an empty date.
pub(super) fn change_date(value: &[u8]) -> Option<String> {
    let digits = std::str::from_utf8(value.get(..14)?).ok()?;
    if !digits.bytes().all(|b| b.is_ascii_digit()) || digits.starts_with("0000") {
        return None;
    }
    Some(format!(
        "{}-{}-{}T{}:{}:{}",
        digits.get(..4)?,
        digits.get(4..6)?,
        digits.get(6..8)?,
        digits.get(8..10)?,
        digits.get(10..12)?,
        digits.get(12..14)?
    ))
}

fn known(scheme: &str, code: &str) -> bool {
    vocabulary::scheme(scheme).is_some_and(|s| s.concept(code).is_some())
}

fn agent(property: &str, code_text: String) -> Statement {
    resource(property, "Agent", None, vec![code(code_text)])
}

fn code(text: String) -> Statement {
    literal("code", text)
}

fn concept(property: &str, scheme: &str, code: &str) -> Statement {
    Statement {
        property: property.to_owned(),
        value: Value::Concept {
            scheme: scheme.to_owned(),
            code: code.to_owned(),
        },
    }
}

fn resource(
    property: &str,
    class: &str,
    label: Option<String>,
    statements: Vec<Statement>,
) -> Statement {
    Statement {
        property: property.to_owned(),
        value: Value::Resource(Resource {
            class: class.to_owned(),
            label,
            statements,
        }),
    }
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

#[cfg(test)]
mod tests {
    use elzevir_marc::{Field, Record, Subfield, Tag};

    use super::*;

    #[test]
    fn leader_control_fields_and_040_become_one_admin_metadata() {
        let control = |tag: [u8; 3], value: &str| Field::Control {
            tag: Tag::new(tag),
            value: value.as_bytes().to_vec(),
        };
        let record = Record {
            leader: *b"00000cam a2200000 i 4500",
            fields: vec![
                control(*b"001", "153256"),
                control(*b"003", "BR-RjBN"),
                control(*b"005", "20260823192913.0"),
                control(*b"008", "250321s1995    bl            000 0 por d"),
                Field::Data {
                    tag: Tag::new(*b"040"),
                    indicators: *b"  ",
                    subfields: [
                        (b'a', "BR-X"),
                        (b'b', "por"),
                        (b'c', "BR-X"),
                        (b'd', "BR-Y"),
                        (b'e', "aacr2"),
                        (b'e', "rda"),
                    ]
                    .iter()
                    .map(|(code, value)| Subfield {
                        code: *code,
                        value: value.as_bytes().to_vec(),
                    })
                    .collect(),
                },
            ],
        };
        let statements = of_instance(&Marc(&record));
        let [
            Statement {
                value: Value::Resource(admin),
                ..
            },
        ] = statements.as_slice()
        else {
            panic!("one bf:adminMetadata");
        };
        let properties: Vec<&str> = admin
            .statements
            .iter()
            .map(|s| s.property.as_str())
            .collect();
        assert_eq!(
            properties,
            [
                "status",
                "identifiedBy",
                "assigner",
                "descriptionLanguage",
                "descriptionModifier",
                "descriptionConventions",
                "descriptionConventions",
                "creationDate",
                "changeDate"
            ]
        );
        assert!(
            matches!(&admin.statements[5].value, Value::Resource(r) if r.class == "DescriptionConventions")
        );
        assert!(matches!(&admin.statements[6].value, Value::Concept { code, .. } if code == "rda"));
        assert_eq!(
            admin.statements[7],
            literal("creationDate", "2025-03-21".to_owned())
        );
        assert_eq!(
            admin.statements[8],
            literal("changeDate", "2026-08-23T19:29:13".to_owned())
        );
        assert_eq!(creation_date(b"991231"), Some("1999-12-31".to_owned()));
        assert_eq!(change_date(b"00000000000000.0"), None);
    }
}
