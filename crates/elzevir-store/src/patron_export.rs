// SPDX-License-Identifier: GPL-3.0-or-later
//! Everything kept about one patron, as one JSON document (docs/specs/circulacao/lgpd.md,
//! items 8 to 11).

use elzevir_domain::patron::PatronId;
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OptionalExtension};
use serde_json::{Map, Value, json};

/// The version of the document's shape; it changes when a key changes meaning.
const FORMAT: &str = "elzevir-patron-export/1";

/// The record itself: every column but the password hash and the bookkeeping ones, with the
/// names of the branch and the category.
const PATRON: &str = "SELECT p.*, b.name AS branch_name, c.name AS category_name
     FROM patrons p
     JOIN branches b ON b.id = p.branch_id
     JOIN patron_categories c ON c.id = p.category_id
     WHERE p.id = ?1 AND p.anonymized_at IS NULL";

/// Columns of the record that are not about the patron, or are a secret of theirs.
const LEFT_OUT: &[&str] = &["password_hash", "anonymized_at", "row_version"];

/// Each list of the document, with the query that fills it; `?1` is the patron. Items carry
/// their barcode and title, so the document reads without the catalog.
const LISTS: &[(&str, &str)] = &[
    (
        "loans",
        "SELECT t.*, i.barcode AS item_barcode, n.title AS title FROM loans t
         LEFT JOIN items i ON i.id = t.item_id LEFT JOIN instances n ON n.id = i.instance_id
         WHERE t.patron_id = ?1 ORDER BY t.id",
    ),
    (
        "returned_loans",
        "SELECT t.*, i.barcode AS item_barcode, n.title AS title FROM returned_loans t
         LEFT JOIN items i ON i.id = t.item_id LEFT JOIN instances n ON n.id = i.instance_id
         WHERE t.patron_id = ?1 ORDER BY t.id",
    ),
    (
        "holds",
        "SELECT t.*, i.barcode AS item_barcode, n.title AS title FROM holds t
         LEFT JOIN items i ON i.id = t.item_id LEFT JOIN instances n ON n.id = t.instance_id
         WHERE t.patron_id = ?1 ORDER BY t.id",
    ),
    (
        "ended_holds",
        "SELECT t.*, i.barcode AS item_barcode, n.title AS title FROM ended_holds t
         LEFT JOIN items i ON i.id = t.item_id LEFT JOIN instances n ON n.id = t.instance_id
         WHERE t.patron_id = ?1 ORDER BY t.id",
    ),
    (
        "account_lines",
        "SELECT t.*, i.barcode AS item_barcode FROM account_lines t
         LEFT JOIN items i ON i.id = t.item_id
         WHERE t.patron_id = ?1 ORDER BY t.id",
    ),
    (
        "account_offsets",
        "SELECT o.* FROM account_offsets o
         WHERE o.credit_id IN (SELECT id FROM account_lines WHERE patron_id = ?1)
            OR o.debit_id IN (SELECT id FROM account_lines WHERE patron_id = ?1)
         ORDER BY o.id",
    ),
    // The values of the additional fields, by field name (patron-attributes.md, item 15).
    (
        "additional_fields",
        "SELECT f.name AS field, coalesce(o.label, v.text) AS value FROM patron_field_values v
         JOIN patron_fields f ON f.id = v.field_id
         LEFT JOIN patron_field_options o ON o.id = v.option_id
         WHERE v.patron_id = ?1 ORDER BY f.name, v.id",
    ),
    (
        "restrictions",
        "SELECT * FROM patron_restrictions WHERE patron_id = ?1 ORDER BY id",
    ),
    (
        "notices",
        "SELECT * FROM notices WHERE patron_id = ?1 ORDER BY id",
    ),
    (
        "notice_items",
        "SELECT t.*, i.barcode AS item_barcode FROM notice_items t
         LEFT JOIN items i ON i.id = t.item_id
         WHERE t.notice_id IN (SELECT id FROM notices WHERE patron_id = ?1)
         ORDER BY t.notice_id, t.item_id",
    ),
    // Item 10: when the record was opened, its CPF shown or this copy taken, without who
    // did it (the staff member's login is theirs, not the patron's).
    (
        "record_access",
        "SELECT at, kind FROM audit_log
         WHERE kind IN ('patrons.view', 'patrons.reveal_cpf', 'patrons.export')
           AND coalesce(json_extract(command, '$.Patrons.View'),
                        json_extract(command, '$.Patrons.RevealCpf'),
                        json_extract(command, '$.Patrons.Export')) = ?1
         ORDER BY seq",
    ),
];

/// Item 9: the document, or `None` when there is no such patron (or they were anonymized).
pub(crate) fn export(connection: &Connection, patron: PatronId) -> rusqlite::Result<Option<Value>> {
    let Some(mut record) = connection
        .query_row(PATRON, [patron.0], object)
        .optional()?
    else {
        return Ok(None);
    };
    let has_password = record
        .get("password_hash")
        .is_some_and(|hash| !hash.is_null());
    for key in LEFT_OUT {
        record.remove(*key);
    }
    record.insert("opac_password".to_owned(), Value::Bool(has_password));
    let mut document = Map::new();
    document.insert("format".to_owned(), json!(FORMAT));
    document.insert("patron".to_owned(), Value::Object(record));
    for (name, sql) in LISTS {
        let mut statement = connection.prepare_cached(sql)?;
        let rows = statement
            .query_map([patron.0], |row| object(row).map(Value::Object))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        document.insert((*name).to_owned(), Value::Array(rows));
    }
    Ok(Some(Value::Object(document)))
}

/// A row as an object keyed by its column names, so a column added later comes along.
fn object(row: &rusqlite::Row<'_>) -> rusqlite::Result<Map<String, Value>> {
    let statement = row.as_ref();
    let mut object = Map::new();
    for index in 0..statement.column_count() {
        let value = match row.get_ref(index)? {
            ValueRef::Integer(number) => json!(number),
            ValueRef::Real(number) => json!(number),
            ValueRef::Text(text) => json!(String::from_utf8_lossy(text)),
            // No column about a patron holds bytes; one that did would not read as text.
            ValueRef::Null | ValueRef::Blob(_) => Value::Null,
        };
        object.insert(statement.column_name(index)?.to_owned(), value);
    }
    Ok(object)
}
