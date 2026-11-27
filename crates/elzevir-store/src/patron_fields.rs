// SPDX-License-Identifier: GPL-3.0-or-later
//! `patrons.*_field`: the additional patron fields
//! (docs/specs/circulacao/patron-attributes.md, items 1 to 7).

use elzevir_app::commands::WriteRefusal;
use elzevir_app::patron_fields::{
    FieldChange, FieldKind, FieldOption, PatronFieldLine, PatronFieldSpec, checked_field,
};
use elzevir_app::patrons::{PatronOutput, PatronRefusal};
use elzevir_domain::patron::{PatronCategoryId, PatronFieldId, PatronFieldOptionId};
use rusqlite::{Connection, OptionalExtension, params};

use crate::patrons::refused;
use crate::{CommandFailed, StoreError};

type Changed = Result<PatronOutput, CommandFailed<WriteRefusal>>;

const TEXT: i64 = 1;
const DATE: i64 = 2;
const LIST: i64 = 3;

/// Applies one change to the fields inside the writer's transaction.
pub(crate) fn change(connection: &Connection, change: &FieldChange) -> Changed {
    match change {
        FieldChange::Create(spec) => {
            let spec = checked_field(spec.clone()).map_err(refused)?;
            check_shared(connection, &spec, 0)?;
            connection
                .execute(
                    "INSERT INTO patron_fields (name, kind, repeatable, is_unique, required,
                         category_id, opac_shown, searchable)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        spec.name,
                        kind_number(&spec.kind),
                        spec.repeatable,
                        spec.unique,
                        spec.required,
                        spec.category.map(|category| category.0),
                        spec.opac_shown,
                        spec.searchable
                    ],
                )
                .map_err(StoreError::from)?;
            let field = PatronFieldId(connection.last_insert_rowid());
            write_options(connection, field, &spec.kind)?;
            Ok(PatronOutput::FieldChanged(Some(field)))
        }
        FieldChange::Update { field, spec } => {
            let spec = checked_field(spec.clone()).map_err(refused)?;
            update(connection, *field, &spec)?;
            Ok(PatronOutput::FieldChanged(Some(*field)))
        }
        FieldChange::Delete(field) => {
            if count(
                connection,
                "SELECT count(*) FROM patron_field_values WHERE field_id = ?1",
                field.0,
            )? > 0
            {
                return Err(refused(PatronRefusal::FieldInUse));
            }
            connection
                .execute(
                    "DELETE FROM patron_field_options WHERE field_id = ?1",
                    [field.0],
                )
                .map_err(StoreError::from)?;
            let deleted = connection
                .execute("DELETE FROM patron_fields WHERE id = ?1", [field.0])
                .map_err(StoreError::from)?;
            if deleted == 0 {
                return Err(refused(PatronRefusal::UnknownField));
            }
            Ok(PatronOutput::FieldChanged(None))
        }
    }
}

/// Refuses a name another field has, or a category that does not exist.
fn check_shared(
    connection: &Connection,
    spec: &PatronFieldSpec,
    field: i64,
) -> Result<(), CommandFailed<WriteRefusal>> {
    let taken = connection
        .query_row(
            "SELECT 1 FROM patron_fields WHERE name = ?1 AND id <> ?2",
            params![spec.name, field],
            |_| Ok(()),
        )
        .optional()
        .map_err(StoreError::from)?
        .is_some();
    if taken {
        return Err(refused(PatronRefusal::FieldNameTaken));
    }
    if let Some(category) = spec.category
        && count(
            connection,
            "SELECT count(*) FROM patron_categories WHERE id = ?1",
            category.0,
        )? == 0
    {
        return Err(refused(PatronRefusal::UnknownCategory));
    }
    Ok(())
}

fn update(
    connection: &Connection,
    field: PatronFieldId,
    spec: &PatronFieldSpec,
) -> Result<(), CommandFailed<WriteRefusal>> {
    let Some(stored) = read_field(connection, field)? else {
        return Err(refused(PatronRefusal::UnknownField));
    };
    check_shared(connection, spec, field.0)?;
    let held = count(
        connection,
        "SELECT count(*) FROM patron_field_values WHERE field_id = ?1",
        field.0,
    )?;
    if held > 0 && !stored.kind.same_sort(&spec.kind) {
        return Err(refused(PatronRefusal::FieldKindInUse));
    }
    // Koha: Koha::Patron::Attribute::Type::check_repeatables.
    if !spec.repeatable
        && count(
            connection,
            "SELECT count(*) FROM (SELECT 1 FROM patron_field_values WHERE field_id = ?1
             GROUP BY patron_id HAVING count(*) > 1)",
            field.0,
        )? > 0
    {
        return Err(refused(PatronRefusal::FieldHasRepeats));
    }
    // Koha: Koha::Patron::Attribute::Type::check_unique_ids.
    if spec.unique
        && count(
            connection,
            "SELECT count(*) FROM (SELECT 1 FROM patron_field_values WHERE field_id = ?1
             GROUP BY option_id, lower(text) HAVING count(DISTINCT patron_id) > 1)",
            field.0,
        )? > 0
    {
        return Err(refused(PatronRefusal::FieldHasShared));
    }
    connection
        .execute(
            "UPDATE patron_fields SET name = ?2, kind = ?3, repeatable = ?4, is_unique = ?5,
                 required = ?6, category_id = ?7, opac_shown = ?8, searchable = ?9,
                 row_version = row_version + 1
             WHERE id = ?1",
            params![
                field.0,
                spec.name,
                kind_number(&spec.kind),
                spec.repeatable,
                spec.unique,
                spec.required,
                spec.category.map(|category| category.0),
                spec.opac_shown,
                spec.searchable
            ],
        )
        .map_err(StoreError::from)?;
    write_options(connection, field, &spec.kind)
}

/// Makes the stored options those of `kind`: kept ones renamed and moved, new ones added,
/// the rest removed unless a patron holds them.
fn write_options(
    connection: &Connection,
    field: PatronFieldId,
    kind: &FieldKind,
) -> Result<(), CommandFailed<WriteRefusal>> {
    let options: &[FieldOption] = match kind {
        FieldKind::List(options) => options,
        FieldKind::Text | FieldKind::Date => &[],
    };
    let kept: Vec<i64> = options
        .iter()
        .filter_map(|option| option.id.map(|id| id.0))
        .collect();
    let stored = option_ids(connection, field)?;
    if kept.iter().any(|id| !stored.contains(id)) {
        return Err(refused(PatronRefusal::UnknownField));
    }
    for gone in stored.iter().filter(|id| !kept.contains(id)) {
        if count(
            connection,
            "SELECT count(*) FROM patron_field_values WHERE option_id = ?1",
            *gone,
        )? > 0
        {
            return Err(refused(PatronRefusal::FieldOptionInUse));
        }
        connection
            .execute("DELETE FROM patron_field_options WHERE id = ?1", [gone])
            .map_err(StoreError::from)?;
    }
    // Labels move out of the way first, so a swap of two labels does not hit UNIQUE.
    connection
        .execute(
            "UPDATE patron_field_options SET label = char(1) || id WHERE field_id = ?1",
            [field.0],
        )
        .map_err(StoreError::from)?;
    for (position, option) in options.iter().enumerate() {
        let position = i64::try_from(position).unwrap_or(i64::MAX);
        match option.id {
            Some(id) => connection.execute(
                "UPDATE patron_field_options SET label = ?2, position = ?3 WHERE id = ?1",
                params![id.0, option.label, position],
            ),
            None => connection.execute(
                "INSERT INTO patron_field_options (field_id, label, position) VALUES (?1, ?2, ?3)",
                params![field.0, option.label, position],
            ),
        }
        .map_err(StoreError::from)?;
    }
    Ok(())
}

fn option_ids(connection: &Connection, field: PatronFieldId) -> Result<Vec<i64>, StoreError> {
    let mut statement =
        connection.prepare_cached("SELECT id FROM patron_field_options WHERE field_id = ?1")?;
    let ids = statement.query_map([field.0], |row| row.get(0))?;
    Ok(ids.collect::<rusqlite::Result<Vec<i64>>>()?)
}

fn count(connection: &Connection, sql: &str, id: i64) -> Result<i64, StoreError> {
    Ok(connection.query_row(sql, [id], |row| row.get(0))?)
}

const fn kind_number(kind: &FieldKind) -> i64 {
    match kind {
        FieldKind::Text => TEXT,
        FieldKind::Date => DATE,
        FieldKind::List(_) => LIST,
    }
}

/// One stored field, with its options in order.
fn read_field(
    connection: &Connection,
    field: PatronFieldId,
) -> Result<Option<PatronFieldSpec>, StoreError> {
    Ok(fields(connection)?
        .into_iter()
        .find(|line| line.id == field)
        .map(|line| line.spec))
}

/// Every field, by name, with its options and how many patrons hold a value.
pub(crate) fn fields(connection: &Connection) -> Result<Vec<PatronFieldLine>, StoreError> {
    let mut statement = connection.prepare_cached(
        "SELECT f.id, f.name, f.kind, f.repeatable, f.is_unique, f.required, f.category_id,
                f.opac_shown, f.searchable,
                (SELECT count(DISTINCT v.patron_id) FROM patron_field_values v
                 WHERE v.field_id = f.id)
         FROM patron_fields f ORDER BY f.name, f.id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            PatronFieldId(row.get(0)?),
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
            [
                row.get::<_, bool>(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(7)?,
                row.get(8)?,
            ],
            row.get::<_, Option<i64>>(6)?.map(PatronCategoryId),
            u64::try_from(row.get::<_, i64>(9)?).unwrap_or(0),
        ))
    })?;
    let rows = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    let mut options = connection.prepare_cached(
        "SELECT id, label FROM patron_field_options WHERE field_id = ?1 ORDER BY position, id",
    )?;
    let mut lines = Vec::with_capacity(rows.len());
    for (
        id,
        name,
        kind,
        [repeatable, unique, required, opac_shown, searchable],
        category,
        patrons,
    ) in rows
    {
        let kind = match kind {
            DATE => FieldKind::Date,
            LIST => FieldKind::List(
                options
                    .query_map([id.0], |row| {
                        Ok(FieldOption {
                            id: Some(PatronFieldOptionId(row.get(0)?)),
                            label: row.get(1)?,
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?,
            ),
            _ => FieldKind::Text,
        };
        lines.push(PatronFieldLine {
            id,
            spec: PatronFieldSpec {
                name,
                kind,
                repeatable,
                unique,
                required,
                category,
                opac_shown,
                searchable,
            },
            patrons,
        });
    }
    Ok(lines)
}
