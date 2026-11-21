// SPDX-License-Identifier: GPL-3.0-or-later
//! Editing a stored Work or Instance in place (docs/specs/catalogo/edicao.md).
//!
//! An edit names the version it was read at; if the row moved since, it is refused and
//! nothing is written. The source record is kept: the export starts from it and rewrites
//! only the fields the model maps, so fields nobody edited go out as they came in.

use elzevir_app::catalog::{
    CatalogOutput, CatalogRefusal, InstanceChange, InstanceUpdate, NewContribution, NewSubject,
    NewWorkRelation, WorkChange, WorkUpdate,
};
use elzevir_app::commands::WriteRefusal;
use elzevir_app::listing::sort_key;
use elzevir_app::matching::identifier_key;
use elzevir_domain::catalog::{InstanceId, MatchRule, Vocabulary, WorkId};
use rusqlite::{Connection, OptionalExtension, ToSql, params};

use crate::catalog::{
    HeadingSource, insert_identifiers, insert_notes, link_contributions_at_from, link_languages,
    link_relations, link_subjects_at_from, optional_term, record_changed, refused,
};
use crate::statements::{replace_classes, replace_property};
use crate::{CommandFailed, StoreError, WriteTx};
use elzevir_domain::statement::{EntityKind, EntityRef};

type Edited = Result<CatalogOutput, CommandFailed<WriteRefusal>>;

/// Applies a Work edit read at `update.row_version`. Every Instance of the Work gets a
/// new version and is reindexed, since their pages and exports show the Work.
pub(crate) fn update_work(tx: &WriteTx<'_>, update: &WorkUpdate) -> Edited {
    let connection = tx.connection();
    let work = update.work;
    let current = version(
        connection,
        "SELECT row_version FROM works WHERE id = ?1",
        work.0,
    )?
    .ok_or_else(|| refused(CatalogRefusal::UnknownWork(work)))?;
    check_version(update.row_version, current)?;
    if update.changes.is_empty() {
        return Ok(CatalogOutput::WorkUpdated {
            work,
            row_version: current,
        });
    }
    for change in &update.changes {
        apply_work_change(connection, work, change)?;
    }
    let row_version = bump(
        connection,
        "UPDATE works SET row_version = row_version + 1 WHERE id = ?1 RETURNING row_version",
        work.0,
    )?;
    for instance in instances_of(connection, work)? {
        record_changed(tx, instance)?;
    }
    Ok(CatalogOutput::WorkUpdated { work, row_version })
}

/// Applies an Instance edit read at `update.row_version`, the version of its record.
pub(crate) fn update_instance(tx: &WriteTx<'_>, update: &InstanceUpdate) -> Edited {
    let connection = tx.connection();
    let instance = update.instance;
    let current = version(
        connection,
        "SELECT row_version FROM instances WHERE id = ?1",
        instance.0,
    )?
    .ok_or_else(|| refused(CatalogRefusal::UnknownInstance(instance)))?;
    check_version(update.row_version, current)?;
    if update.changes.is_empty() {
        return Ok(CatalogOutput::InstanceUpdated {
            instance,
            row_version: current,
        });
    }
    for change in &update.changes {
        apply_instance_change(connection, instance, change)?;
    }
    record_changed(tx, instance)?;
    let row_version = version(
        connection,
        "SELECT row_version FROM instances WHERE id = ?1",
        instance.0,
    )?
    .unwrap_or(current);
    Ok(CatalogOutput::InstanceUpdated {
        instance,
        row_version,
    })
}

pub(crate) fn check_version(read: i64, current: i64) -> Result<(), CommandFailed<WriteRefusal>> {
    if read == current {
        Ok(())
    } else {
        Err(refused(CatalogRefusal::Stale { read, current }))
    }
}

pub(crate) fn version(
    connection: &Connection,
    sql: &str,
    id: i64,
) -> Result<Option<i64>, StoreError> {
    Ok(connection
        .query_row(sql, params![id], |row| row.get(0))
        .optional()?)
}

pub(crate) fn bump(connection: &Connection, sql: &str, id: i64) -> Result<i64, StoreError> {
    Ok(connection.query_row(sql, params![id], |row| row.get(0))?)
}

/// Runs one `UPDATE ... SET <column> = ?2 WHERE id = ?1`; the statement text is a constant
/// chosen by the caller, never built from the edit.
fn set(connection: &Connection, sql: &str, id: i64, value: &dyn ToSql) -> Result<(), StoreError> {
    connection.execute(sql, params![id, value])?;
    Ok(())
}

fn apply_work_change(
    connection: &Connection,
    work: WorkId,
    change: &WorkChange,
) -> Result<(), CommandFailed<WriteRefusal>> {
    let id = work.0;
    match change {
        WorkChange::Title(title) => {
            connection
                .execute(
                    "UPDATE works SET title = ?2, sort_key = ?3 WHERE id = ?1",
                    params![id, title, sort_key(title)],
                )
                .map_err(StoreError::from)?;
        }
        WorkChange::Classes(classes) => {
            let subject = EntityRef {
                entity: EntityKind::Work,
                id,
            };
            replace_classes(connection, subject, classes)?;
        }
        WorkChange::Classification { number, scheme } => {
            let scheme = optional_term(
                connection,
                Vocabulary::ClassificationScheme,
                scheme.as_deref(),
            )?;
            connection
                .execute(
                    "UPDATE works SET classification = ?2, classification_scheme = ?3
                     WHERE id = ?1",
                    params![id, number, scheme],
                )
                .map_err(StoreError::from)?;
        }
        WorkChange::Summary(summary) => set(
            connection,
            "UPDATE works SET summary = ?2 WHERE id = ?1",
            id,
            summary,
        )?,
        WorkChange::Audience(audience) => set(
            connection,
            "UPDATE works SET audience = ?2 WHERE id = ?1",
            id,
            audience,
        )?,
        WorkChange::Languages(languages) => {
            clear(
                connection,
                "DELETE FROM work_languages WHERE work_id = ?1",
                id,
            )?;
            link_languages(connection, id, languages)?;
        }
        WorkChange::Contributions(contributions) => {
            let contributions = keep_role_terms(connection, id, contributions)?;
            relink_headings(
                connection,
                id,
                "DELETE FROM contributions WHERE work_id=?1",
                &contributions,
                link_contributions_at_from,
            )?;
        }
        WorkChange::Subjects(subjects) => {
            let subjects = keep_subject_details(connection, id, subjects)?;
            relink_headings(
                connection,
                id,
                "DELETE FROM work_subjects WHERE work_id=?1",
                &subjects,
                link_subjects_at_from,
            )?;
        }
        WorkChange::Relations(relations) => replace_relations(connection, work, relations)?,
        WorkChange::Statements {
            property,
            statements,
        } => replace_property(
            connection,
            EntityRef {
                entity: EntityKind::Work,
                id,
            },
            property,
            statements,
        )?,
    }
    Ok(())
}

/// Preserve the occurrence's scoped heading when a staff edit reorders or reuses it.
fn relink_headings<T>(
    c: &Connection,
    work: i64,
    clear_sql: &str,
    headings: &[(T, Option<String>)],
    link: impl Fn(
        &Connection,
        i64,
        i64,
        &[T],
        HeadingSource<'_>,
    ) -> Result<(), CommandFailed<WriteRefusal>>,
) -> Result<(), CommandFailed<WriteRefusal>> {
    clear(c, clear_sql, work)?;
    for (position, (heading, identity)) in (0..).zip(headings) {
        let source = identity
            .as_deref()
            .map_or(HeadingSource::Catalog, HeadingSource::Retained);
        link(c, work, position, std::slice::from_ref(heading), source)?;
    }
    Ok(())
}

/// The Work's relations become `relations`; a Work does not relate to itself, which the
/// table refuses too.
fn replace_relations(
    connection: &Connection,
    work: WorkId,
    relations: &[NewWorkRelation],
) -> Result<(), CommandFailed<WriteRefusal>> {
    if let Some(relation) = relations.iter().find(|relation| relation.work == work) {
        return Err(refused(CatalogRefusal::UnknownWork(relation.work)));
    }
    clear(
        connection,
        "DELETE FROM work_relations WHERE work_id = ?1",
        work.0,
    )?;
    link_relations(connection, work.0, relations)
}

/// `contributions` with the relator terms and identifiers of the Work's current
/// contributions: the form edits only the agent and the role, so a contribution that keeps
/// them keeps the term the record wrote beside the code and the links it gave the agent.
fn keep_role_terms(
    connection: &Connection,
    work: i64,
    contributions: &[NewContribution],
) -> Result<Vec<(NewContribution, Option<String>)>, StoreError> {
    let mut ids = connection.prepare(
        "SELECT h.identity FROM contributions r JOIN agents h ON h.id=r.agent_id
        WHERE r.work_id=?1 ORDER BY r.position",
    )?;
    let identities = ids
        .query_map([work], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut current: Vec<_> = crate::export::contributions(connection, work)
        .map_err(store_error)?
        .into_iter()
        .zip(identities)
        .collect();
    Ok(contributions
        .iter()
        .map(|contribution| {
            let mut contribution = contribution.clone();
            let mut identity = None;
            let found = current.iter().position(|(present, _)| {
                (contribution.identifiers.is_empty()
                    || present.identifiers == contribution.identifiers)
                    && present.kind == contribution.kind
                    && present.heading == contribution.heading
                    && present.role == contribution.role
            });
            if let Some(found) = found {
                let (present, key) = current.remove(found);
                identity = Some(key);
                if contribution.name_parts.is_empty() {
                    contribution.name_parts.clone_from(&present.name_parts);
                }
                if contribution.role_term.is_none() {
                    contribution.role_term = present.role_term;
                }
                if contribution.identifiers.is_empty() {
                    contribution.identifiers = present.identifiers;
                }
            }
            (contribution, identity)
        })
        .collect())
}

/// `subjects` with what the Work's current subjects say beyond the heading: the form edits
/// each heading as text, so a subject that keeps its kind, heading and thesaurus keeps the
/// kinds of its subdivisions and the links the record gave it.
fn keep_subject_details(
    connection: &Connection,
    work: i64,
    subjects: &[NewSubject],
) -> Result<Vec<(NewSubject, Option<String>)>, StoreError> {
    let mut ids = connection.prepare(
        "SELECT h.identity FROM work_subjects r JOIN subjects h ON h.id=r.subject_id
        WHERE r.work_id=?1 ORDER BY r.position",
    )?;
    let identities = ids
        .query_map([work], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut current: Vec<_> = crate::export::subjects(connection, work)
        .map_err(store_error)?
        .into_iter()
        .zip(identities)
        .collect();
    Ok(subjects
        .iter()
        .map(|subject| {
            let mut subject = subject.clone();
            let mut identity = None;
            let found = current.iter().position(|(present, _)| {
                (subject.identifiers.is_empty() || present.identifiers == subject.identifiers)
                    && present.kind == subject.kind
                    && present.heading == subject.heading
                    && present.thesaurus == subject.thesaurus
            });
            if let Some(found) = found {
                let (present, key) = current.remove(found);
                identity = Some(key);
                if subject.name_parts.is_empty() {
                    subject.name_parts.clone_from(&present.name_parts);
                }
                if subject.subdivisions.is_empty() {
                    subject.subdivisions = present.subdivisions;
                }
                if subject.identifiers.is_empty() {
                    subject.identifiers = present.identifiers;
                }
            }
            (subject, identity)
        })
        .collect())
}

fn apply_instance_change(
    connection: &Connection,
    instance: InstanceId,
    change: &InstanceChange,
) -> Result<(), CommandFailed<WriteRefusal>> {
    let id = instance.0;
    let (sql, value): (&str, &dyn ToSql) = match change {
        InstanceChange::Title(title) => ("UPDATE instances SET title = ?2 WHERE id = ?1", title),
        InstanceChange::Classes(classes) => {
            let subject = EntityRef {
                entity: EntityKind::Instance,
                id,
            };
            replace_classes(connection, subject, classes)?;
            return Ok(());
        }
        InstanceChange::Edition(edition) => {
            ("UPDATE instances SET edition = ?2 WHERE id = ?1", edition)
        }
        InstanceChange::Place(place) => ("UPDATE instances SET place = ?2 WHERE id = ?1", place),
        InstanceChange::Publisher(publisher) => (
            "UPDATE instances SET publisher = ?2 WHERE id = ?1",
            publisher,
        ),
        InstanceChange::Date(date) => ("UPDATE instances SET date = ?2 WHERE id = ?1", date),
        InstanceChange::Extent(extent) => {
            ("UPDATE instances SET extent = ?2 WHERE id = ?1", extent)
        }
        InstanceChange::Series(series) => {
            ("UPDATE instances SET series = ?2 WHERE id = ?1", series)
        }
        InstanceChange::OpacHidden(hidden) => (
            "UPDATE instances SET opac_hidden = ?2 WHERE id = ?1",
            hidden,
        ),
        InstanceChange::Identifiers(identifiers) => {
            clear(
                connection,
                "DELETE FROM instance_identifiers WHERE instance_id = ?1",
                id,
            )?;
            insert_identifiers(connection, id, identifiers)?;
            return Ok(refresh_identifier_keys(connection, instance, identifiers)?);
        }
        InstanceChange::Notes(notes) => {
            clear(
                connection,
                "DELETE FROM instance_notes WHERE instance_id = ?1",
                id,
            )?;
            return Ok(insert_notes(connection, id, notes)?);
        }
        InstanceChange::Statements {
            property,
            statements,
        } => {
            return replace_property(
                connection,
                EntityRef {
                    entity: EntityKind::Instance,
                    id,
                },
                property,
                statements,
            );
        }
    };
    Ok(set(connection, sql, id, value)?)
}

/// The ISBN and ISSN match keys follow the identifiers; the Koha `biblionumber` key comes
/// from the source record, which an edit keeps, so it stays.
fn refresh_identifier_keys(
    connection: &Connection,
    instance: InstanceId,
    identifiers: &[elzevir_app::catalog::NewIdentifier],
) -> Result<(), StoreError> {
    connection.execute(
        "DELETE FROM match_keys WHERE instance_id = ?1 AND rule <> ?2",
        params![instance.0, MatchRule::KohaBiblionumber.code()],
    )?;
    for key in identifiers
        .iter()
        .filter_map(|identifier| identifier_key(identifier.kind, &identifier.value))
    {
        connection.execute(
            "INSERT OR IGNORE INTO match_keys (rule, key, instance_id) VALUES (?1, ?2, ?3)",
            params![key.rule.code(), key.key, instance.0],
        )?;
    }
    Ok(())
}

fn clear(connection: &Connection, sql: &str, id: i64) -> Result<(), StoreError> {
    connection.execute(sql, params![id])?;
    Ok(())
}

pub(crate) fn instances_of(
    connection: &Connection,
    work: WorkId,
) -> Result<Vec<InstanceId>, StoreError> {
    let mut statement =
        connection.prepare_cached("SELECT id FROM instances WHERE work_id = ?1 ORDER BY id")?;
    let ids = statement
        .query_map(params![work.0], |row| row.get(0).map(InstanceId))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ids)
}

/// A stored code this build cannot read is a corrupt or newer database: it is reported as
/// the store error the writer answers with.
fn store_error(failure: crate::catalog::ReadFailure) -> StoreError {
    match failure {
        crate::catalog::ReadFailure::Store(error) => error,
        other => StoreError::Sqlite(other.to_string()),
    }
}
