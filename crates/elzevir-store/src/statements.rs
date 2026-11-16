// SPDX-License-Identifier: GPL-3.0-or-later
//! `catalog.set_statements` and the reading of BIBFRAME statements
//! (docs/specs/catalogo/bibframe-completo.md, items 10a to 10g).

use elzevir_app::catalog::{
    CatalogOutput, CatalogReadError, CatalogRefusal, ClassChoice, PropertyEdit, StatementReader,
    StatementsUpdate,
};
use elzevir_app::commands::WriteRefusal;
use elzevir_domain::catalog::{InstanceId, WorkId};
use elzevir_domain::statement::{
    self, Description, EntityKind, EntityRef, Resource, Statement, StatementRefusal, Value,
};
use rusqlite::{Connection, params};

use crate::catalog::{record_changed, refused};
use crate::editing::{bump, check_version, instances_of, version};
use crate::{CommandFailed, ElzevirStore, StoreError, WriteTx};

/// The number a subject or an entity value is stored with.
const fn kind_number(kind: EntityKind) -> i64 {
    match kind {
        EntityKind::Hub => 1,
        EntityKind::Work => 2,
        EntityKind::Instance => 3,
        EntityKind::Item => 4,
    }
}

fn kind_of(number: i64) -> Result<EntityKind, StoreError> {
    match number {
        1 => Ok(EntityKind::Hub),
        2 => Ok(EntityKind::Work),
        3 => Ok(EntityKind::Instance),
        4 => Ok(EntityKind::Item),
        other => Err(StoreError::Sqlite(format!(
            "statement entity kind {other} is unknown"
        ))),
    }
}

/// The version an edit of `entity` names: its own, or for an Instance its record's.
/// Constant SQL per kind; the id is a parameter.
fn current_version(connection: &Connection, entity: EntityRef) -> Result<Option<i64>, StoreError> {
    let sql = match entity.entity {
        EntityKind::Hub => "SELECT row_version FROM hubs WHERE id = ?1",
        EntityKind::Work => "SELECT row_version FROM works WHERE id = ?1",
        EntityKind::Instance => "SELECT row_version FROM instances WHERE id = ?1",
        EntityKind::Item => "SELECT row_version FROM items WHERE id = ?1",
    };
    version(connection, sql, entity.id)
}

type Set = Result<CatalogOutput, CommandFailed<WriteRefusal>>;

/// `catalog.set_statements` (item 10e).
pub(crate) fn set_statements(tx: &WriteTx<'_>, update: &StatementsUpdate) -> Set {
    let connection = tx.connection();
    let subject = update.subject;
    let current = current_version(connection, subject)?
        .ok_or_else(|| refused(CatalogRefusal::UnknownEntity(subject)))?;
    check_version(update.row_version, current)?;
    statement::check(subject.entity, &update.statements)
        .map_err(|refusal| refused(CatalogRefusal::BadStatement(refusal)))?;
    check_entities(connection, &update.statements)?;
    connection
        .execute(
            "DELETE FROM statements WHERE subject_kind = ?1 AND subject_id = ?2",
            params![kind_number(subject.entity), subject.id],
        )
        .map_err(StoreError::from)?;
    insert_level(connection, Owner::Subject(subject), &update.statements)?;
    let row_version = changed(tx, subject)?;
    Ok(CatalogOutput::StatementsSet {
        subject,
        row_version,
    })
}

/// `CatalogCommand::EditStatements`: each property's statements replaced in place, then
/// the version raised as `catalog.set_statements` raises it.
pub(crate) fn edit_statements(tx: &WriteTx<'_>, edit: &PropertyEdit) -> Set {
    let connection = tx.connection();
    let subject = edit.subject;
    let current = current_version(connection, subject)?
        .ok_or_else(|| refused(CatalogRefusal::UnknownEntity(subject)))?;
    check_version(edit.row_version, current)?;
    if edit.changes.is_empty() {
        return Ok(CatalogOutput::StatementsSet {
            subject,
            row_version: current,
        });
    }
    for change in &edit.changes {
        replace_property(connection, subject, &change.property, &change.statements)?;
    }
    let row_version = changed(tx, subject)?;
    Ok(CatalogOutput::StatementsSet {
        subject,
        row_version,
    })
}

/// Replaces the classes and statements of `subject`, a stored entity, without touching
/// its version: the command that changes the entity raises it (item 10h).
pub(crate) fn store_description(
    connection: &Connection,
    subject: EntityRef,
    description: &Description,
) -> Result<(), CommandFailed<WriteRefusal>> {
    statement::check_description(subject.entity, description)
        .map_err(|refusal| refused(CatalogRefusal::BadStatement(refusal)))?;
    check_entities(connection, &description.statements)?;
    let key = params![kind_number(subject.entity), subject.id];
    connection
        .execute(
            "DELETE FROM statements WHERE subject_kind = ?1 AND subject_id = ?2",
            key,
        )
        .map_err(StoreError::from)?;
    connection
        .execute(
            "DELETE FROM entity_classes WHERE subject_kind = ?1 AND subject_id = ?2",
            key,
        )
        .map_err(StoreError::from)?;
    insert_classes(connection, subject, &description.classes)?;
    insert_level(connection, Owner::Subject(subject), &description.statements)?;
    Ok(())
}

/// Replaces the statements of one `property` of `subject` (bibframe-completo.md, items 24
/// to 26): the new ones take the place of the first old one, or go last; the statements of
/// every other property stay as they are. The edit that calls it raises the version.
pub(crate) fn replace_property(
    connection: &Connection,
    subject: EntityRef,
    property: &str,
    replacing: &[Statement],
) -> Result<(), CommandFailed<WriteRefusal>> {
    let current = read_statements(connection, subject)?;
    let at = current
        .iter()
        .position(|statement| statement.property == property)
        .unwrap_or(current.len());
    let mut statements: Vec<Statement> = current
        .into_iter()
        .filter(|statement| statement.property != property)
        .collect();
    let at = at.min(statements.len());
    statements.splice(at..at, replacing.iter().cloned());
    if replacing
        .iter()
        .any(|statement| statement.property != property)
    {
        return Err(refused(CatalogRefusal::BadStatement(
            StatementRefusal::NotApplicable {
                property: property.to_owned(),
                entity: subject.entity,
            },
        )));
    }
    statement::check(subject.entity, &statements)
        .map_err(|refusal| refused(CatalogRefusal::BadStatement(refusal)))?;
    check_entities(connection, replacing)?;
    connection
        .execute(
            "DELETE FROM statements WHERE subject_kind = ?1 AND subject_id = ?2",
            params![kind_number(subject.entity), subject.id],
        )
        .map_err(StoreError::from)?;
    insert_level(connection, Owner::Subject(subject), &statements)?;
    Ok(())
}

/// Replaces the classes of `subject` with the chosen ones, checked as a description is
/// (bibframe-completo.md, item 14); the statements stay.
pub(crate) fn replace_classes(
    connection: &Connection,
    subject: EntityRef,
    chosen: &[ClassChoice],
) -> Result<(), CommandFailed<WriteRefusal>> {
    let classes: Vec<String> = chosen.iter().map(|choice| choice.class.clone()).collect();
    statement::check_description(
        subject.entity,
        &Description {
            classes: classes.clone(),
            statements: Vec::new(),
        },
    )
    .map_err(|refusal| refused(CatalogRefusal::BadStatement(refusal)))?;
    connection
        .execute(
            "DELETE FROM entity_classes WHERE subject_kind = ?1 AND subject_id = ?2",
            params![kind_number(subject.entity), subject.id],
        )
        .map_err(StoreError::from)?;
    insert_classes(connection, subject, &classes)?;
    Ok(())
}

fn insert_classes(
    connection: &Connection,
    subject: EntityRef,
    classes: &[String],
) -> Result<(), StoreError> {
    for (position, class) in (0_i64..).zip(classes) {
        let class = name_id(connection, class)?;
        connection.execute(
            "INSERT OR IGNORE INTO entity_classes (subject_kind, subject_id, position, class)
             VALUES (?1, ?2, ?3, ?4)",
            params![kind_number(subject.entity), subject.id, position, class],
        )?;
    }
    Ok(())
}

/// Replaces the descriptions of a record's Work and Instance.
pub(crate) fn store_record_descriptions(
    connection: &Connection,
    work: i64,
    instance: i64,
    record: &elzevir_app::catalog::NewRecord,
) -> Result<(), CommandFailed<WriteRefusal>> {
    let subject = |entity, id| EntityRef { entity, id };
    store_description(
        connection,
        subject(EntityKind::Work, work),
        &record.work_description,
    )?;
    store_description(
        connection,
        subject(EntityKind::Instance, instance),
        &record.instance.description,
    )
}

/// The classes and statements of `subject` (item 10h).
pub(crate) fn read_description(
    connection: &Connection,
    subject: EntityRef,
) -> Result<Description, StoreError> {
    let mut query = connection.prepare_cached(
        "SELECT n.name FROM entity_classes c JOIN ontology_names n ON n.id = c.class
         WHERE c.subject_kind = ?1 AND c.subject_id = ?2 ORDER BY c.position",
    )?;
    let classes = query
        .query_map(params![kind_number(subject.entity), subject.id], |row| {
            row.get(0)
        })?
        .collect::<Result<Vec<String>, _>>()?;
    Ok(Description {
        classes,
        statements: read_statements(connection, subject)?,
    })
}

/// Raises the subject's version and refreshes what shows it, as the edits do.
fn changed(tx: &WriteTx<'_>, subject: EntityRef) -> Result<i64, StoreError> {
    let connection = tx.connection();
    let id = subject.id;
    match subject.entity {
        EntityKind::Hub => bump(
            connection,
            "UPDATE hubs SET row_version = row_version + 1 WHERE id = ?1 RETURNING row_version",
            id,
        ),
        EntityKind::Work => {
            let row_version = bump(
                connection,
                "UPDATE works SET row_version = row_version + 1 WHERE id = ?1 RETURNING row_version",
                id,
            )?;
            for instance in instances_of(connection, WorkId(id))? {
                record_changed(tx, instance)?;
            }
            Ok(row_version)
        }
        EntityKind::Instance => {
            record_changed(tx, InstanceId(id))?;
            Ok(current_version(connection, subject)?.unwrap_or_default())
        }
        EntityKind::Item => {
            let (row_version, instance): (i64, i64) = connection.query_row(
                "UPDATE items SET row_version = row_version + 1 WHERE id = ?1
                 RETURNING row_version, instance_id",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            record_changed(tx, InstanceId(instance))?;
            Ok(row_version)
        }
    }
}

/// Every entity value names a stored entity.
fn check_entities(
    connection: &Connection,
    statements: &[Statement],
) -> Result<(), CommandFailed<WriteRefusal>> {
    for statement in statements {
        match &statement.value {
            Value::Entity(entity) => {
                if current_version(connection, *entity)?.is_none() {
                    return Err(refused(CatalogRefusal::UnknownEntity(*entity)));
                }
            }
            Value::Resource(resource) => check_entities(connection, &resource.statements)?,
            Value::Literal { .. } | Value::Concept { .. } | Value::Iri(_) => {}
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Owner {
    Subject(EntityRef),
    Parent(i64),
}

fn insert_level(
    connection: &Connection,
    owner: Owner,
    statements: &[Statement],
) -> Result<(), StoreError> {
    let (subject_kind, subject_id, parent) = match owner {
        Owner::Subject(subject) => (Some(kind_number(subject.entity)), Some(subject.id), None),
        Owner::Parent(parent) => (None, None, Some(parent)),
    };
    for (position, statement) in (0_i64..).zip(statements) {
        let property = name_id(connection, &statement.property)?;
        let mut literal = None;
        let mut language = None;
        let mut concept = None;
        let mut entity = (None, None);
        let mut iri = None;
        let mut class = None;
        let mut label = None;
        match &statement.value {
            Value::Literal {
                text,
                language: tag,
            } => {
                literal = Some(text.as_str());
                language = tag.as_deref();
            }
            Value::Concept { scheme, code } => {
                concept = Some(concept_id(connection, scheme, code)?);
            }
            Value::Entity(target) => entity = (Some(kind_number(target.entity)), Some(target.id)),
            Value::Iri(value) => iri = Some(value.as_str()),
            Value::Resource(resource) => {
                class = Some(name_id(connection, &resource.class)?);
                label = resource.label.as_deref();
            }
        }
        let id: i64 = connection.query_row(
            "INSERT INTO statements (subject_kind, subject_id, parent, position, property,
                 literal, language, concept, entity_kind, entity_id, iri, class, label)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13) RETURNING id",
            params![
                subject_kind,
                subject_id,
                parent,
                position,
                property,
                literal,
                language,
                concept,
                entity.0,
                entity.1,
                iri,
                class,
                label
            ],
            |row| row.get(0),
        )?;
        if let Value::Resource(resource) = &statement.value {
            insert_level(connection, Owner::Parent(id), &resource.statements)?;
        }
    }
    Ok(())
}

fn name_id(connection: &Connection, name: &str) -> Result<i64, StoreError> {
    connection.execute(
        "INSERT INTO ontology_names (name) VALUES (?1) ON CONFLICT (name) DO NOTHING",
        params![name],
    )?;
    Ok(connection.query_row(
        "SELECT id FROM ontology_names WHERE name = ?1",
        params![name],
        |row| row.get(0),
    )?)
}

fn concept_id(connection: &Connection, scheme: &str, code: &str) -> Result<i64, StoreError> {
    connection.execute(
        "INSERT INTO lc_concepts (scheme, code) VALUES (?1, ?2) ON CONFLICT DO NOTHING",
        params![scheme, code],
    )?;
    Ok(connection.query_row(
        "SELECT id FROM lc_concepts WHERE scheme = ?1 AND code = ?2",
        params![scheme, code],
        |row| row.get(0),
    )?)
}

/// One stored row, before its nested statements are read.
struct Row {
    id: i64,
    property: String,
    literal: Option<String>,
    language: Option<String>,
    concept: Option<(String, String)>,
    entity: Option<(i64, i64)>,
    iri: Option<String>,
    class: Option<String>,
    label: Option<String>,
}

const SELECT: &str = "SELECT s.id, p.name, s.literal, s.language, c.scheme, c.code,
        s.entity_kind, s.entity_id, s.iri, k.name, s.label
     FROM statements s
     JOIN ontology_names p ON p.id = s.property
     LEFT JOIN lc_concepts c ON c.id = s.concept
     LEFT JOIN ontology_names k ON k.id = s.class";

/// The statements of `subject`, nested ones included (item 10e).
pub(crate) fn read_statements(
    connection: &Connection,
    subject: EntityRef,
) -> Result<Vec<Statement>, StoreError> {
    let rows = rows(
        connection,
        &format!(
            "{SELECT} WHERE s.parent IS NULL AND s.subject_kind = ?1 AND s.subject_id = ?2 ORDER BY s.position"
        ),
        params![kind_number(subject.entity), subject.id],
    )?;
    build(connection, rows, 1)
}

fn rows(
    connection: &Connection,
    sql: &str,
    parameters: impl rusqlite::Params,
) -> Result<Vec<Row>, StoreError> {
    let mut query = connection.prepare_cached(sql)?;
    let rows = query
        .query_map(parameters, |row| {
            let scheme: Option<String> = row.get(4)?;
            let code: Option<String> = row.get(5)?;
            let kind: Option<i64> = row.get(6)?;
            let id: Option<i64> = row.get(7)?;
            Ok(Row {
                id: row.get(0)?,
                property: row.get(1)?,
                literal: row.get(2)?,
                language: row.get(3)?,
                concept: scheme.zip(code),
                entity: kind.zip(id),
                iri: row.get(8)?,
                class: row.get(9)?,
                label: row.get(10)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn build(
    connection: &Connection,
    stored: Vec<Row>,
    depth: usize,
) -> Result<Vec<Statement>, StoreError> {
    let mut statements = Vec::with_capacity(stored.len());
    for row in stored {
        let value = if let Some(text) = row.literal {
            Value::Literal {
                text,
                language: row.language,
            }
        } else if let Some((scheme, code)) = row.concept {
            Value::Concept { scheme, code }
        } else if let Some((kind, id)) = row.entity {
            Value::Entity(EntityRef {
                entity: kind_of(kind)?,
                id,
            })
        } else if let Some(iri) = row.iri {
            Value::Iri(iri)
        } else if let Some(class) = row.class {
            // The writer refuses deeper nesting, so this bound only stops a damaged file.
            let nested = if depth > statement::MAX_DEPTH {
                Vec::new()
            } else {
                let children = rows(
                    connection,
                    &format!("{SELECT} WHERE s.parent = ?1 ORDER BY s.position"),
                    params![row.id],
                )?;
                build(connection, children, depth + 1)?
            };
            Value::Resource(Resource {
                class,
                label: row.label,
                statements: nested,
            })
        } else {
            return Err(StoreError::Sqlite(format!(
                "statement {} has no value",
                row.id
            )));
        };
        statements.push(Statement {
            property: row.property,
            value,
        });
    }
    Ok(statements)
}

impl StatementReader for ElzevirStore {
    fn statements(&self, subject: EntityRef) -> Result<Vec<Statement>, CatalogReadError> {
        self.readers()
            .read(|c| read_statements(c, subject))
            .map_err(|e| CatalogReadError(e.to_string()))
    }

    fn description(&self, subject: EntityRef) -> Result<Description, CatalogReadError> {
        self.readers()
            .read(|c| read_description(c, subject))
            .map_err(|e| CatalogReadError(e.to_string()))
    }

    fn version(&self, subject: EntityRef) -> Result<Option<i64>, CatalogReadError> {
        self.readers()
            .read(|c| current_version(c, subject))
            .map_err(|e| CatalogReadError(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_kinds_round_trip_through_their_numbers() {
        for kind in EntityKind::ALL {
            assert_eq!(kind_of(kind_number(kind)).unwrap(), kind);
        }
        assert!(kind_of(5).is_err());
    }
}
