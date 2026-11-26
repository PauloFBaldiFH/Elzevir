// SPDX-License-Identifier: GPL-3.0-or-later
//! Editing an agent or a subject once for every record that names it, and linking one
//! to a Work (docs/specs/catalogo/autoridades.md).

use elzevir_app::authorities::{AgentView, AuthorityReader, MAX_IDENTIFIERS, SubjectView};
use elzevir_app::catalog::{
    AgentChange, AgentUpdate, AuthorityLink, AuthorityRole, CatalogOutput, CatalogReadError,
    CatalogRefusal, NewContribution, NewSubject, SubjectUpdate, WorkAuthority,
};
use elzevir_app::commands::WriteRefusal;
use elzevir_domain::catalog::{
    AgentId, AgentKind, AuthorityLinkKind, InstanceId, SubjectId, SubjectKind, WorkId,
};
use rusqlite::{Connection, OptionalExtension, params};

use crate::catalog::{exists, link_contributions_at, link_subjects_at, record_changed, refused};
use crate::editing::{bump, check_version, version};
use crate::{CommandFailed, ElzevirStore, StoreError, WriteTx};

/// `catalog.update_agent`: the agent changes, and every Work that names it and every
/// Instance of those Works get a new version, so open forms see the change and the OPAC
/// and the search index show the new heading.
pub(crate) fn update_agent(
    tx: &WriteTx<'_>,
    update: &AgentUpdate,
) -> Result<CatalogOutput, CommandFailed<WriteRefusal>> {
    let connection = tx.connection();
    let agent = update.agent;
    let current = version(
        connection,
        "SELECT row_version FROM agents WHERE id = ?1",
        agent.0,
    )?
    .ok_or_else(|| refused(CatalogRefusal::UnknownAgent(agent)))?;
    check_version(update.row_version, current)?;
    if update.changes.is_empty() {
        return Ok(CatalogOutput::AgentUpdated {
            agent,
            row_version: current,
        });
    }
    for change in &update.changes {
        match change {
            AgentChange::Heading(heading) => {
                let taken: Option<i64> = connection
                    .query_row(
                        "SELECT other.id FROM agents other JOIN agents this ON this.id = ?1
                         WHERE other.kind = this.kind AND other.heading = ?2
                           AND other.id <> this.id",
                        params![agent.0, heading],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(StoreError::from)?;
                if taken.is_some() {
                    return Err(refused(CatalogRefusal::AgentHeadingTaken(heading.clone())));
                }
                connection
                    .execute(
                        "UPDATE agents SET heading = ?2 WHERE id = ?1",
                        params![agent.0, heading],
                    )
                    .map_err(StoreError::from)?;
            }
        }
    }
    let row_version = bump(
        connection,
        "UPDATE agents SET row_version = row_version + 1 WHERE id = ?1 RETURNING row_version",
        agent.0,
    )?;
    records_changed(
        tx,
        "SELECT work_id FROM contributions WHERE agent_id = ?1",
        agent.0,
    )?;
    Ok(CatalogOutput::AgentUpdated { agent, row_version })
}

/// `catalog.update_subject`: as [`update_agent`], for a subject heading. A heading keeps
/// the typed subdivisions of the subject, so it has as many ` -- ` parts after the first.
pub(crate) fn update_subject(
    tx: &WriteTx<'_>,
    update: &SubjectUpdate,
) -> Result<CatalogOutput, CommandFailed<WriteRefusal>> {
    let connection = tx.connection();
    let subject = update.subject;
    let current = version(
        connection,
        "SELECT row_version FROM subjects WHERE id = ?1",
        subject.0,
    )?
    .ok_or_else(|| refused(CatalogRefusal::UnknownSubject(subject)))?;
    check_version(update.row_version, current)?;
    if update.changes.is_empty() {
        return Ok(CatalogOutput::SubjectUpdated {
            subject,
            row_version: current,
        });
    }
    for change in &update.changes {
        match change {
            AgentChange::Heading(heading) => {
                let subdivisions: String = connection
                    .query_row(
                        "SELECT subdivisions FROM subjects WHERE id = ?1",
                        params![subject.0],
                        |row| row.get(0),
                    )
                    .map_err(StoreError::from)?;
                let typed = subdivisions
                    .split(',')
                    .filter(|code| !code.is_empty())
                    .count();
                let parts = heading.split(" -- ").count().saturating_sub(1);
                if typed > 0 && parts != typed {
                    return Err(refused(CatalogRefusal::SubdivisionsChanged(
                        heading.clone(),
                    )));
                }
                let taken = exists_with(
                    connection,
                    "SELECT 1 FROM subjects other JOIN subjects this ON this.id = ?1
                     WHERE other.kind = this.kind AND other.heading = ?2
                       AND coalesce(other.thesaurus, 0) = coalesce(this.thesaurus, 0)
                       AND other.subdivisions = this.subdivisions AND other.id <> this.id",
                    params![subject.0, heading],
                )?;
                if taken {
                    return Err(refused(CatalogRefusal::SubjectHeadingTaken(
                        heading.clone(),
                    )));
                }
                connection
                    .execute(
                        "UPDATE subjects SET heading = ?2 WHERE id = ?1",
                        params![subject.0, heading],
                    )
                    .map_err(StoreError::from)?;
            }
        }
    }
    let row_version = bump(
        connection,
        "UPDATE subjects SET row_version = row_version + 1 WHERE id = ?1 RETURNING row_version",
        subject.0,
    )?;
    records_changed(
        tx,
        "SELECT work_id FROM work_subjects WHERE subject_id = ?1",
        subject.0,
    )?;
    Ok(CatalogOutput::SubjectUpdated {
        subject,
        row_version,
    })
}

/// Every Work `works` (a constant selecting Work ids by `?1`) names and every Instance of
/// them get a new version, and the Instances are reindexed: open forms are refused
/// instead of writing the old heading back, and the OPAC cache by version renders again.
fn records_changed(
    tx: &WriteTx<'_>,
    works: &'static str,
    id: i64,
) -> Result<(), CommandFailed<WriteRefusal>> {
    let connection = tx.connection();
    // `works` is one of the constants above, never input.
    connection
        .execute(
            &format!("UPDATE works SET row_version = row_version + 1 WHERE id IN ({works})"),
            params![id],
        )
        .map_err(StoreError::from)?;
    let instances: Vec<i64> = {
        let mut statement = connection
            .prepare_cached(&format!(
                "SELECT id FROM instances WHERE work_id IN ({works}) ORDER BY id"
            ))
            .map_err(StoreError::from)?;
        statement
            .query_map(params![id], |row| row.get(0))
            .map_err(StoreError::from)?
            .collect::<Result<_, _>>()
            .map_err(StoreError::from)?
    };
    for instance in instances {
        record_changed(tx, InstanceId(instance))?;
    }
    Ok(())
}

/// `catalog.link_authority`: the authority, found by its class and heading or created,
/// goes after the Work's other contributors or subjects; the Work and its Instances get
/// new versions, so an open form cannot write the old list back.
pub(crate) fn link_authority(
    tx: &WriteTx<'_>,
    link: &WorkAuthority,
) -> Result<CatalogOutput, CommandFailed<WriteRefusal>> {
    let connection = tx.connection();
    let work = link.work;
    if !exists(connection, "SELECT 1 FROM works WHERE id = ?1", work.0)? {
        return Err(refused(CatalogRefusal::UnknownWork(work)));
    }
    match &link.role {
        AuthorityRole::Contribution {
            kind,
            heading,
            relator,
            identifier,
        } => {
            let linked = exists_with(
                connection,
                "SELECT 1 FROM contributions c JOIN agents a ON a.id = c.agent_id
                 LEFT JOIN terms t ON t.id = c.role
                 WHERE c.work_id = ?1 AND a.kind = ?2 AND a.heading = ?3
                   AND coalesce(t.code, '') = coalesce(?4, '')",
                params![work.0, kind.code(), heading, relator],
            )?;
            if linked {
                return Err(refused(CatalogRefusal::AlreadyLinked(heading.clone())));
            }
            let contribution = NewContribution {
                kind: *kind,
                heading: heading.clone(),
                role: relator.clone(),
                role_term: None,
                primary: false,
                name_parts: Vec::new(),
                identifiers: authority_record(identifier.as_deref()),
            };
            let next = next_position(
                connection,
                "SELECT coalesce(max(position) + 1, 0) FROM contributions WHERE work_id = ?1",
                work.0,
            )?;
            link_contributions_at(connection, work.0, next, &[contribution])?;
        }
        AuthorityRole::Subject {
            kind,
            heading,
            thesaurus,
            identifier,
        } => {
            let linked = exists_with(
                connection,
                "SELECT 1 FROM work_subjects w JOIN subjects s ON s.id = w.subject_id
                 LEFT JOIN terms t ON t.id = s.thesaurus
                 WHERE w.work_id = ?1 AND s.kind = ?2 AND s.heading = ?3
                   AND coalesce(t.code, '') = coalesce(?4, '')",
                params![work.0, kind.code(), heading, thesaurus],
            )?;
            if linked {
                return Err(refused(CatalogRefusal::AlreadyLinked(heading.clone())));
            }
            let subject = NewSubject {
                kind: *kind,
                heading: heading.clone(),
                thesaurus: thesaurus.clone(),
                subdivisions: Vec::new(),
                name_parts: Vec::new(),
                identifiers: authority_record(identifier.as_deref()),
            };
            let next = next_position(
                connection,
                "SELECT coalesce(max(position) + 1, 0) FROM work_subjects WHERE work_id = ?1",
                work.0,
            )?;
            link_subjects_at(connection, work.0, next, &[subject])?;
        }
        AuthorityRole::PublicationPlace { instance, heading } => {
            return publication_place(tx, link.work, *instance, heading);
        }
    }
    let row_version = bump(
        connection,
        "UPDATE works SET row_version = row_version + 1 WHERE id = ?1 RETURNING row_version",
        work.0,
    )?;
    let instances: Vec<i64> = {
        let mut statement = connection
            .prepare_cached("SELECT id FROM instances WHERE work_id = ?1 ORDER BY id")
            .map_err(StoreError::from)?;
        statement
            .query_map(params![work.0], |row| row.get(0))
            .map_err(StoreError::from)?
            .collect::<Result<_, _>>()
            .map_err(StoreError::from)?
    };
    for instance in instances {
        record_changed(tx, InstanceId(instance))?;
    }
    Ok(CatalogOutput::WorkUpdated { work, row_version })
}

/// The `$0` of an authority approved from a source, if any (fontes-de-autoridade.md,
/// item 20).
fn authority_record(identifier: Option<&str>) -> Vec<AuthorityLink> {
    identifier
        .map(|value| AuthorityLink {
            kind: AuthorityLinkKind::AuthorityRecord,
            value: value.to_owned(),
        })
        .into_iter()
        .collect()
}

/// The place of publication of `instance`, which must be of `work`; only the Instance's
/// record changes.
fn publication_place(
    tx: &WriteTx<'_>,
    work: WorkId,
    instance: InstanceId,
    heading: &str,
) -> Result<CatalogOutput, CommandFailed<WriteRefusal>> {
    let connection = tx.connection();
    let place: Option<Option<String>> = connection
        .query_row(
            "SELECT place FROM instances WHERE id = ?1 AND work_id = ?2",
            params![instance.0, work.0],
            |row| row.get(0),
        )
        .optional()
        .map_err(StoreError::from)?;
    let Some(place) = place else {
        return Err(refused(CatalogRefusal::UnknownInstance(instance)));
    };
    if place.as_deref() == Some(heading) {
        return Err(refused(CatalogRefusal::AlreadyLinked(heading.to_owned())));
    }
    connection
        .execute(
            "UPDATE instances SET place = ?2 WHERE id = ?1",
            params![instance.0, heading],
        )
        .map_err(StoreError::from)?;
    record_changed(tx, instance)?;
    let row_version = version(
        connection,
        "SELECT row_version FROM instances WHERE id = ?1",
        instance.0,
    )?
    .unwrap_or_default();
    Ok(CatalogOutput::InstanceUpdated {
        instance,
        row_version,
    })
}

fn exists_with(
    connection: &Connection,
    sql: &str,
    values: impl rusqlite::Params,
) -> Result<bool, StoreError> {
    Ok(connection
        .query_row(sql, values, |_| Ok(()))
        .optional()?
        .is_some())
}

fn next_position(connection: &Connection, sql: &str, work: i64) -> Result<i64, StoreError> {
    Ok(connection.query_row(sql, params![work], |row| row.get(0))?)
}

impl ElzevirStore {
    /// The distinct identifiers `sql` (a constant taking the authority as `?1` and the
    /// limit as `?2`) selects, up to [`MAX_IDENTIFIERS`].
    fn identifiers(
        &self,
        sql: &'static str,
        id: i64,
    ) -> Result<Vec<AuthorityLink>, CatalogReadError> {
        let limit = i64::try_from(MAX_IDENTIFIERS).unwrap_or(i64::MAX);
        let rows: Vec<(u8, String)> = self
            .readers()
            .read(|c| {
                let mut statement = c.prepare_cached(sql)?;
                statement
                    .query_map(params![id, limit], |row| Ok((row.get(0)?, row.get(1)?)))?
                    .collect::<Result<_, _>>()
            })
            .map_err(|e: rusqlite::Error| CatalogReadError(e.to_string()))?;
        rows.into_iter()
            .map(|(code, value)| {
                let kind = AuthorityLinkKind::from_code(code).ok_or_else(|| {
                    CatalogReadError(format!("unknown identifier kind {code} (newer schema?)"))
                })?;
                Ok(AuthorityLink { kind, value })
            })
            .collect()
    }
}

impl AuthorityReader for ElzevirStore {
    fn agent(&self, id: AgentId) -> Result<Option<AgentView>, CatalogReadError> {
        self.readers()
            .read(|c| {
                c.query_row(
                    "SELECT kind, heading, row_version,
                            (SELECT count(DISTINCT work_id) FROM contributions
                             WHERE agent_id = agents.id),
                            (SELECT count(*) FROM instances WHERE work_id IN
                                (SELECT work_id FROM contributions WHERE agent_id = agents.id))
                     FROM agents WHERE id = ?1",
                    params![id.0],
                    |row| {
                        let code: u8 = row.get(0)?;
                        let works: i64 = row.get(3)?;
                        let instances: i64 = row.get(4)?;
                        Ok((
                            code,
                            row.get::<_, String>(1)?,
                            row.get(2)?,
                            works,
                            instances,
                        ))
                    },
                )
                .optional()
            })
            .map_err(|e| CatalogReadError(e.to_string()))?
            .map(|(code, heading, version, works, instances)| {
                let kind = AgentKind::from_code(code).ok_or_else(|| {
                    CatalogReadError(format!("unknown agent kind {code} (newer schema?)"))
                })?;
                Ok(AgentView {
                    id,
                    kind,
                    heading,
                    version,
                    works: usize::try_from(works).unwrap_or_default(),
                    instances: usize::try_from(instances).unwrap_or_default(),
                    identifiers: self.identifiers(
                        "SELECT DISTINCT i.kind, i.value FROM contribution_identifiers i
                         JOIN contributions c ON c.work_id = i.work_id
                                             AND c.position = i.contribution
                         WHERE c.agent_id = ?1 ORDER BY i.kind, i.value LIMIT ?2",
                        id.0,
                    )?,
                })
            })
            .transpose()
    }

    fn subject(&self, id: SubjectId) -> Result<Option<SubjectView>, CatalogReadError> {
        self.readers()
            .read(|c| {
                c.query_row(
                    "SELECT s.kind, s.heading, t.code, s.row_version,
                            (SELECT count(DISTINCT work_id) FROM work_subjects
                             WHERE subject_id = s.id),
                            (SELECT count(*) FROM instances WHERE work_id IN
                                (SELECT work_id FROM work_subjects WHERE subject_id = s.id))
                     FROM subjects s LEFT JOIN terms t ON t.id = s.thesaurus
                     WHERE s.id = ?1",
                    params![id.0],
                    |row| {
                        let code: u8 = row.get(0)?;
                        let works: i64 = row.get(4)?;
                        let instances: i64 = row.get(5)?;
                        Ok((
                            code,
                            row.get::<_, String>(1)?,
                            row.get::<_, Option<String>>(2)?,
                            row.get(3)?,
                            works,
                            instances,
                        ))
                    },
                )
                .optional()
            })
            .map_err(|e| CatalogReadError(e.to_string()))?
            .map(|(code, heading, thesaurus, version, works, instances)| {
                let kind = SubjectKind::from_code(code).ok_or_else(|| {
                    CatalogReadError(format!("unknown subject kind {code} (newer schema?)"))
                })?;
                Ok(SubjectView {
                    id,
                    kind,
                    heading,
                    thesaurus,
                    version,
                    works: usize::try_from(works).unwrap_or_default(),
                    instances: usize::try_from(instances).unwrap_or_default(),
                    identifiers: self.identifiers(
                        "SELECT DISTINCT i.kind, i.value FROM subject_identifiers i
                         JOIN work_subjects w ON w.work_id = i.work_id
                                             AND w.position = i.subject
                         WHERE w.subject_id = ?1 ORDER BY i.kind, i.value LIMIT ?2",
                        id.0,
                    )?,
                })
            })
            .transpose()
    }
}
