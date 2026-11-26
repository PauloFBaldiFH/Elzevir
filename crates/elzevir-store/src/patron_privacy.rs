// SPDX-License-Identifier: GPL-3.0-or-later
//! `patrons.anonymize` (docs/specs/circulacao/lgpd.md, items 1 to 7): the patron's personal
//! data leaves the record, the reading history and the audit log, for good.

use std::collections::BTreeSet;

use elzevir_app::commands::WriteRefusal;
use elzevir_app::patrons::{PatronOutput, PatronRefusal};
use elzevir_domain::patron::PatronId;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

use crate::patrons::refused;
use crate::{CommandFailed, StoreError};

/// The keys of the patron's personal data in a logged command, whatever its shape: the
/// patron form and Koha's migrated patron use the same names.
const PERSONAL_KEYS: &[&str] = &[
    "cardnumber",
    "surname",
    "first_name",
    "cpf",
    "date_of_birth",
    "email",
    "phone",
    "address",
    "staff_note",
    "password_hash",
];

/// Item 2: refused while the patron has loans or holds; else every personal field goes,
/// the card number becomes `anon-` and the id, and the OPAC access ends.
pub(crate) fn anonymize(
    connection: &Connection,
    patron: PatronId,
) -> Result<PatronOutput, CommandFailed<WriteRefusal>> {
    let found: Option<(String, Option<String>)> = connection
        .query_row(
            "SELECT cardnumber, cpf FROM patrons WHERE id = ?1 AND anonymized_at IS NULL",
            [patron.0],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(StoreError::from)?;
    let Some((cardnumber, cpf)) = found else {
        return Err(refused(PatronRefusal::UnknownPatron));
    };
    let has = |sql: &str| -> Result<bool, StoreError> {
        Ok(connection
            .query_row(sql, [patron.0], |_| Ok(()))
            .optional()?
            .is_some())
    };
    if has("SELECT 1 FROM loans WHERE patron_id = ?1")? {
        return Err(refused(PatronRefusal::HasLoans));
    }
    if has("SELECT 1 FROM holds WHERE patron_id = ?1")? {
        return Err(refused(PatronRefusal::HasHolds));
    }
    let mut identifiers = BTreeSet::from([cardnumber]);
    identifiers.extend(cpf);
    identifiers.extend(former_identifiers(connection, patron)?);
    scrub_audit_log(connection, identifiers)?;
    for sql in [
        // Item 3: the reading history and the ended holds stay, without the patron.
        "UPDATE returned_loans SET patron_id = NULL WHERE patron_id = ?1",
        "UPDATE ended_holds SET patron_id = NULL WHERE patron_id = ?1",
        // Item 4: a notice keeps what it was about, not where it went.
        "UPDATE notices SET to_address = NULL WHERE patron_id = ?1",
        "UPDATE patrons SET cardnumber = 'anon-' || id, surname = '', first_name = NULL,
                cpf = NULL, date_of_birth = NULL, email = NULL, phone = NULL, address = NULL,
                staff_note = NULL, password_hash = NULL, reading_history = 0,
                anonymized_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                row_version = row_version + 1
         WHERE id = ?1",
    ] {
        connection
            .execute(sql, [patron.0])
            .map_err(StoreError::from)?;
    }
    Ok(PatronOutput::Anonymized(patron))
}

/// The card numbers and CPFs the patron had before: the logged updates of the record name
/// them.
fn former_identifiers(
    connection: &Connection,
    patron: PatronId,
) -> Result<Vec<String>, StoreError> {
    let mut statement = connection.prepare_cached(
        "SELECT json_extract(command, '$.Patrons.Update.fields.cardnumber'),
                json_extract(command, '$.Patrons.Update.fields.cpf')
         FROM audit_log
         WHERE kind = 'patrons.update'
           AND json_extract(command, '$.Patrons.Update.patron') = ?1",
    )?;
    let rows = statement.query_map([patron.0], |row| {
        Ok([row.get::<_, Option<String>>(0)?, row.get(1)?])
    })?;
    let mut found = Vec::new();
    for pair in rows {
        found.extend(pair?.into_iter().flatten());
    }
    Ok(found)
}

/// Item 5: every logged command that names one of the identifiers loses the personal
/// fields of the patron it describes; the rest of the command stays. An object that is the
/// patron may carry another identifier of theirs (an old card number beside the CPF), which
/// is searched in turn.
fn scrub_audit_log(
    connection: &Connection,
    identifiers: BTreeSet<String>,
) -> Result<(), StoreError> {
    let mut known = identifiers.clone();
    let mut pending: Vec<String> = identifiers.into_iter().collect();
    while let Some(identifier) = pending.pop() {
        let mut changed = Vec::new();
        {
            let mut statement = connection.prepare_cached(
                "SELECT seq, command FROM audit_log WHERE instr(command, ?1) > 0",
            )?;
            let rows = statement.query_map([&identifier], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?;
            for row in rows {
                let (seq, command) = row?;
                // A command the log cannot parse back was not written by the store; it is
                // left as it is.
                let Ok(mut json) = serde_json::from_str::<Value>(&command) else {
                    continue;
                };
                let mut found = BTreeSet::new();
                if scrub(&mut json, &known, &mut found) {
                    changed.push((seq, json.to_string()));
                }
                for other in found {
                    if known.insert(other.clone()) {
                        pending.push(other);
                    }
                }
            }
        }
        for (seq, command) in changed {
            connection.execute(
                "UPDATE audit_log SET command = ?1 WHERE seq = ?2",
                params![command, seq],
            )?;
        }
    }
    Ok(())
}

/// Empties the personal fields of each object that is the patron (its card number or CPF
/// is one of the identifiers), keeping in `found` the identifiers it had; whether anything
/// changed.
fn scrub(value: &mut Value, identifiers: &BTreeSet<String>, found: &mut BTreeSet<String>) -> bool {
    match value {
        Value::Object(object) => {
            let names_the_patron = ["cardnumber", "cpf"].iter().any(|key| {
                object
                    .get(*key)
                    .and_then(Value::as_str)
                    .is_some_and(|text| identifiers.contains(text))
            });
            let mut changed = false;
            if names_the_patron {
                for key in ["cardnumber", "cpf"] {
                    if let Some(text) = object.get(key).and_then(Value::as_str) {
                        found.insert(text.to_owned());
                    }
                }
                for key in PERSONAL_KEYS {
                    if let Some(field) = object.get_mut(*key)
                        && !field.is_null()
                    {
                        *field = Value::Null;
                        changed = true;
                    }
                }
            }
            for child in object.values_mut() {
                changed |= scrub(child, identifiers, found);
            }
            changed
        }
        Value::Array(items) => {
            // Every item is walked, so no short-circuit.
            let mut changed = false;
            for item in items {
                changed |= scrub(item, identifiers, found);
            }
            changed
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_patron_loses_the_personal_fields() {
        let mut json: Value = serde_json::from_str(
            r#"{"patrons":[{"cardnumber":"A1","surname":"Silva","email":"a@example.org","branch":1},
                          {"cardnumber":"B2","surname":"Silva"}]}"#,
        )
        .unwrap();
        let mut found = BTreeSet::new();
        assert!(scrub(
            &mut json,
            &BTreeSet::from(["A1".to_owned()]),
            &mut found
        ));
        assert_eq!(found, BTreeSet::from(["A1".to_owned()]));
        assert_eq!(
            json.to_string(),
            r#"{"patrons":[{"branch":1,"cardnumber":null,"email":null,"surname":null},{"cardnumber":"B2","surname":"Silva"}]}"#
        );
        assert!(!scrub(
            &mut json,
            &BTreeSet::from(["A1".to_owned()]),
            &mut found
        ));
    }
}
