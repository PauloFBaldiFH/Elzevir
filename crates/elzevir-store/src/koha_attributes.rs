// SPDX-License-Identifier: GPL-3.0-or-later
//! `patrons.migrate_koha_attributes`: Koha's patron attribute types become additional
//! fields and their values come to the migrated patrons
//! (docs/specs/circulacao/patron-attributes.md, items 16 and 17). Koha's codes stay in
//! `koha_keys`.

use std::collections::BTreeMap;

use elzevir_app::commands::WriteRefusal;
use elzevir_app::koha_migration::{
    KohaAttributeValue, KohaAttributesBatch, KohaAttributesMigrated, KohaFieldType, ValueLeftover,
};
use elzevir_app::patron_fields::{
    FieldChange, FieldKind, MAX_TEXT_VALUE, MAX_VALUES_PER_FIELD, PatronFieldSpec,
};
use elzevir_app::patrons::PatronOutput;
use elzevir_domain::patron::PatronCategoryId;
use jiff::civil::Date;
use rusqlite::{Connection, OptionalExtension, params};

use crate::koha_migration::{Keyed, key, set_key};
use crate::{CommandFailed, StoreError};

type Applied = Result<PatronOutput, CommandFailed<WriteRefusal>>;

/// The unit separator between a type's code and an authorised value in `koha_keys`.
const SEPARATOR: char = '\u{1f}';

/// A field as the values need it.
struct Field {
    id: i64,
    name: String,
    kind: FieldKind,
    repeatable: bool,
    unique: bool,
}

/// Applies one write: the types (on the first), then the values.
pub(crate) fn apply(connection: &Connection, batch: &KohaAttributesBatch) -> Applied {
    let mut done = KohaAttributesMigrated::default();
    for field_type in &batch.types {
        save_type(connection, field_type, &mut done)?;
    }
    let mut fields: BTreeMap<&str, Option<Field>> = BTreeMap::new();
    for value in &batch.values {
        if !fields.contains_key(value.code.as_str()) {
            fields.insert(&value.code, field(connection, &value.code)?);
        }
        let Some(Some(field)) = fields.get(value.code.as_str()) else {
            *done
                .left
                .entry((value.code.clone(), ValueLeftover::Type))
                .or_default() += 1;
            continue;
        };
        match give(connection, field, value)? {
            Given::Yes => done.values += 1,
            Given::Already => done.already += 1,
            Given::No(why) => *done.left.entry((field.name.clone(), why)).or_default() += 1,
        }
    }
    Ok(PatronOutput::KohaAttributesMigrated(done))
}

/// Makes the type a field, unless a former run did; its options get their keys.
fn save_type(
    connection: &Connection,
    field_type: &KohaFieldType,
    done: &mut KohaAttributesMigrated,
) -> Result<(), CommandFailed<WriteRefusal>> {
    if key(connection, Keyed::AttributeType, &field_type.code)?.is_some() {
        done.fields_existing += 1;
        return Ok(());
    }
    let taken = |name: &str| -> Result<bool, StoreError> {
        Ok(connection
            .query_row(
                "SELECT 1 FROM patron_fields WHERE name = ?1",
                [name],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    };
    let name = if !taken(&field_type.spec.name)? {
        field_type.spec.name.clone()
    } else if !taken(&field_type.code)? {
        field_type.code.clone()
    } else {
        done.names_taken.push(field_type.code.clone());
        return Ok(());
    };
    let category = match &field_type.category {
        Some(code) => key(connection, Keyed::Category, code)?.map(PatronCategoryId),
        None => None,
    };
    let spec = PatronFieldSpec {
        name,
        category,
        ..field_type.spec.clone()
    };
    let Ok(PatronOutput::FieldChanged(Some(field))) =
        crate::patron_fields::change(connection, &FieldChange::Create(spec))
    else {
        // A type the registry refuses (a name it reads as empty) stays out, like a taken name.
        done.names_taken.push(field_type.code.clone());
        return Ok(());
    };
    set_key(connection, Keyed::AttributeType, &field_type.code, field.0)?;
    done.fields_created += 1;
    let options = crate::patron_fields::option_ids(connection, field)?;
    for (code, index) in &field_type.option_codes {
        if let Some(option) = options.get(*index) {
            let joined = format!("{}{SEPARATOR}{code}", field_type.code);
            set_key(connection, Keyed::AttributeOption, &joined, *option)?;
        }
    }
    done.options += options.len();
    Ok(())
}

/// The field a type became, if it did.
fn field(connection: &Connection, code: &str) -> Result<Option<Field>, StoreError> {
    let Some(id) = key(connection, Keyed::AttributeType, code)? else {
        return Ok(None);
    };
    Ok(connection
        .query_row(
            "SELECT name, kind, repeatable, is_unique FROM patron_fields WHERE id = ?1",
            [id],
            |row| {
                Ok(Field {
                    id,
                    name: row.get(0)?,
                    kind: match row.get::<_, i64>(1)? {
                        2 => FieldKind::Date,
                        3 => FieldKind::List(Vec::new()),
                        _ => FieldKind::Text,
                    },
                    repeatable: row.get(2)?,
                    unique: row.get(3)?,
                })
            },
        )
        .optional()?)
}

enum Given {
    Yes,
    Already,
    No(ValueLeftover),
}

/// Gives the patron the value when it follows the field's rules (spec, item 17).
fn give(
    connection: &Connection,
    field: &Field,
    value: &KohaAttributeValue,
) -> Result<Given, StoreError> {
    let patron: Option<i64> = connection
        .query_row(
            "SELECT p.id FROM koha_keys k JOIN patrons p ON p.id = k.local_id
             WHERE k.kind = ?1 AND k.koha_key = ?2 AND p.anonymized_at IS NULL",
            params![Keyed::Patron as i64, value.borrowernumber.to_string()],
            |row| row.get(0),
        )
        .optional()?;
    let Some(patron) = patron else {
        return Ok(Given::No(ValueLeftover::Patron));
    };
    let (option, text): (Option<i64>, Option<String>) = match field.kind {
        FieldKind::List(_) => {
            let joined = format!(
                "{}{SEPARATOR}{}",
                koha_code(connection, field.id)?,
                value.value
            );
            match key(connection, Keyed::AttributeOption, &joined)? {
                Some(option) => (Some(option), None),
                None => return Ok(Given::No(ValueLeftover::Invalid)),
            }
        }
        FieldKind::Date => match value
            .value
            .get(..10)
            .and_then(|day| day.parse::<Date>().ok())
        {
            Some(day) => (None, Some(day.to_string())),
            None => return Ok(Given::No(ValueLeftover::Invalid)),
        },
        FieldKind::Text if value.value.chars().count() > MAX_TEXT_VALUE => {
            return Ok(Given::No(ValueLeftover::Invalid));
        }
        FieldKind::Text => (None, Some(value.value.clone())),
    };
    let holders = |sql: &str| -> Result<i64, StoreError> {
        Ok(
            connection.query_row(sql, params![field.id, patron, option, text], |row| {
                row.get(0)
            })?,
        )
    };
    if holders(
        "SELECT count(*) FROM patron_field_values
         WHERE field_id = ?1 AND patron_id = ?2 AND option_id IS ?3 AND text IS ?4",
    )? > 0
    {
        return Ok(Given::Already);
    }
    let held: i64 = connection.query_row(
        "SELECT count(*) FROM patron_field_values WHERE field_id = ?1 AND patron_id = ?2",
        params![field.id, patron],
        |row| row.get(0),
    )?;
    let most = if field.repeatable {
        MAX_VALUES_PER_FIELD
    } else {
        1
    };
    if usize::try_from(held).unwrap_or(usize::MAX) >= most {
        return Ok(Given::No(ValueLeftover::Repeated));
    }
    if field.unique
        && holders(
            "SELECT count(*) FROM patron_field_values
             WHERE field_id = ?1 AND patron_id <> ?2 AND option_id IS ?3 AND text IS ?4",
        )? > 0
    {
        return Ok(Given::No(ValueLeftover::Taken));
    }
    connection.execute(
        "INSERT INTO patron_field_values (patron_id, field_id, option_id, text)
         VALUES (?1, ?2, ?3, ?4)",
        params![patron, field.id, option, text],
    )?;
    Ok(Given::Yes)
}

/// The Koha code of the type that became `field`.
fn koha_code(connection: &Connection, field: i64) -> Result<String, StoreError> {
    Ok(connection
        .query_row(
            "SELECT koha_key FROM koha_keys WHERE kind = ?1 AND local_id = ?2",
            params![Keyed::AttributeType as i64, field],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or_default())
}
