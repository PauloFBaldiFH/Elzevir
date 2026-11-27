// SPDX-License-Identifier: GPL-3.0-or-later
//! `patrons.*_field`: the additional patron fields
//! (docs/specs/circulacao/patron-attributes.md, items 1 to 7).

use elzevir_app::commands::WriteRefusal;
use elzevir_app::patron_fields::{
    ExtraLine, ExtraProblem, ExtraValue, FieldChange, FieldKind, FieldOption, MAX_VALUES_PER_FIELD,
    PatronExtra, PatronFieldLine, PatronFieldSpec, checked_field, checked_value,
};
use elzevir_app::patrons::{PatronFields, PatronOutput, PatronRefusal};
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
             GROUP BY option_id, text HAVING count(DISTINCT patron_id) > 1)",
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

/// The options of `field`, in the library's order.
pub(crate) fn option_ids(
    connection: &Connection,
    field: PatronFieldId,
) -> Result<Vec<i64>, StoreError> {
    let mut statement = connection.prepare_cached(
        "SELECT id FROM patron_field_options WHERE field_id = ?1 ORDER BY position, id",
    )?;
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

/// A field that applies to a patron: id, kind, repeatable, unique, required.
type Applying = (i64, i64, bool, bool, bool);

/// The fields that apply to a patron of `category`.
fn applying(
    connection: &Connection,
    category: PatronCategoryId,
) -> Result<Vec<Applying>, StoreError> {
    let mut statement = connection.prepare_cached(
        "SELECT id, kind, repeatable, is_unique, required FROM patron_fields
         WHERE category_id IS NULL OR category_id = ?1",
    )?;
    let rows = statement.query_map([category.0], |row| {
        Ok((
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
        ))
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn bad(field: i64, problem: ExtraProblem) -> CommandFailed<WriteRefusal> {
    refused(PatronRefusal::BadExtra(PatronFieldId(field), problem))
}

/// Refuses a patron without a value for a required field of their category (spec,
/// items 5 and 10); only the staff form asks, so the Koha migration does not.
pub(crate) fn check_required(
    connection: &Connection,
    fields: &PatronFields,
) -> Result<(), CommandFailed<WriteRefusal>> {
    for (field, _, _, _, required) in applying(connection, fields.category)? {
        let held = fields.extra.iter().any(|extra| {
            extra.field.0 == field && matches!(checked_value(extra.value.clone()), Ok(Some(_)))
        });
        if required && !held {
            return Err(bad(field, ExtraProblem::Missing));
        }
    }
    Ok(())
}

/// Replaces the values of `patron` with those of `fields` (spec, items 9 to 11): values of
/// fields that do not apply to the category are dropped, the rest checked against their
/// field and, for a unique field, against the other patrons.
pub(crate) fn write_values(
    connection: &Connection,
    patron: i64,
    fields: &PatronFields,
) -> Result<(), CommandFailed<WriteRefusal>> {
    let applying = applying(connection, fields.category)?;
    let mut rows: Vec<(i64, Option<i64>, Option<String>)> = Vec::new();
    for extra in &fields.extra {
        let field = extra.field.0;
        let Some(&(_, kind, repeatable, unique, _)) = applying.iter().find(|(id, ..)| *id == field)
        else {
            continue;
        };
        let Some(value) =
            checked_value(extra.value.clone()).map_err(|problem| bad(field, problem))?
        else {
            continue;
        };
        let row = match (kind, value) {
            (TEXT, ExtraValue::Text(text)) => (field, None, Some(text)),
            (DATE, ExtraValue::Date(day)) => (field, None, Some(day.to_string())),
            (LIST, ExtraValue::Option(option)) => {
                let owned: i64 = connection
                    .query_row(
                        "SELECT count(*) FROM patron_field_options WHERE id = ?1 AND field_id = ?2",
                        params![option.0, field],
                        |row| row.get(0),
                    )
                    .map_err(StoreError::from)?;
                if owned == 0 {
                    return Err(bad(field, ExtraProblem::Invalid));
                }
                (field, Some(option.0), None)
            }
            _ => return Err(bad(field, ExtraProblem::Invalid)),
        };
        if rows.contains(&row) {
            continue;
        }
        let held = rows.iter().filter(|(id, ..)| *id == field).count();
        if (!repeatable && held >= 1) || held >= MAX_VALUES_PER_FIELD {
            return Err(bad(field, ExtraProblem::Repeated));
        }
        // Koha: Koha::Patron::Attribute::unique_ok.
        if unique {
            let taken = connection
                .query_row(
                    "SELECT 1 FROM patron_field_values
                     WHERE field_id = ?1 AND patron_id <> ?2
                       AND option_id IS ?3 AND text IS ?4",
                    params![field, patron, row.1, row.2],
                    |_| Ok(()),
                )
                .optional()
                .map_err(StoreError::from)?
                .is_some();
            if taken {
                return Err(bad(field, ExtraProblem::Taken));
            }
        }
        rows.push(row);
    }
    connection
        .execute(
            "DELETE FROM patron_field_values WHERE patron_id = ?1",
            [patron],
        )
        .map_err(StoreError::from)?;
    for (field, option, text) in rows {
        connection
            .execute(
                "INSERT INTO patron_field_values (patron_id, field_id, option_id, text)
                 VALUES (?1, ?2, ?3, ?4)",
                params![patron, field, option, text],
            )
            .map_err(StoreError::from)?;
    }
    Ok(())
}

/// The values `patron` holds, by field name and in the order they were saved.
pub(crate) fn values_of(
    connection: &Connection,
    patron: i64,
) -> rusqlite::Result<Vec<PatronExtra>> {
    let mut statement = connection.prepare_cached(
        "SELECT v.field_id, f.kind, v.option_id, v.text FROM patron_field_values v
         JOIN patron_fields f ON f.id = v.field_id
         WHERE v.patron_id = ?1 ORDER BY f.name, v.id",
    )?;
    let rows = statement.query_map([patron], |row| {
        let value = match (
            row.get::<_, Option<i64>>(2)?,
            row.get::<_, Option<String>>(3)?,
        ) {
            (Some(option), _) => ExtraValue::Option(PatronFieldOptionId(option)),
            (None, Some(text)) if row.get::<_, i64>(1)? == DATE => {
                ExtraValue::Date(text.parse().map_err(|error: jiff::Error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        3,
                        rusqlite::types::Type::Text,
                        error.into(),
                    )
                })?)
            }
            (None, text) => ExtraValue::Text(text.unwrap_or_default()),
        };
        Ok(PatronExtra {
            field: PatronFieldId(row.get(0)?),
            value,
        })
    })?;
    rows.collect()
}

/// The values `patron` holds as the record shows them; with `opac`, only the fields shown
/// on the OPAC (spec, items 12 and 13).
pub(crate) fn lines_of(
    connection: &Connection,
    patron: i64,
    opac: bool,
) -> rusqlite::Result<Vec<ExtraLine>> {
    let mut statement = connection.prepare_cached(
        "SELECT f.name, coalesce(o.label, v.text) FROM patron_field_values v
         JOIN patron_fields f ON f.id = v.field_id
         LEFT JOIN patron_field_options o ON o.id = v.option_id
         WHERE v.patron_id = ?1 AND (?2 = 0 OR f.opac_shown = 1)
         ORDER BY f.name, v.id",
    )?;
    let rows = statement.query_map(params![patron, opac], |row| {
        Ok(ExtraLine {
            name: row.get(0)?,
            value: row.get(1)?,
        })
    })?;
    rows.collect()
}
