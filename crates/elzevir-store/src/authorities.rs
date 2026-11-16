// SPDX-License-Identifier: GPL-3.0-or-later
//! Editing an agent once for every record that names it (docs/specs/catalogo/autoridades.md).

use elzevir_app::authorities::{AgentReader, AgentView};
use elzevir_app::catalog::{
    AgentChange, AgentUpdate, AuthorityRole, CatalogOutput, CatalogReadError, CatalogRefusal,
    NewContribution, NewSubject, WorkAuthority,
};
use elzevir_app::commands::WriteRefusal;
use elzevir_domain::catalog::{AgentId, AgentKind, InstanceId};
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
    connection
        .execute(
            "UPDATE works SET row_version = row_version + 1
             WHERE id IN (SELECT work_id FROM contributions WHERE agent_id = ?1)",
            params![agent.0],
        )
        .map_err(StoreError::from)?;
    let instances: Vec<i64> = {
        let mut statement = connection
            .prepare_cached(
                "SELECT id FROM instances WHERE work_id IN
                     (SELECT work_id FROM contributions WHERE agent_id = ?1)
                 ORDER BY id",
            )
            .map_err(StoreError::from)?;
        statement
            .query_map(params![agent.0], |row| row.get(0))
            .map_err(StoreError::from)?
            .collect::<Result<_, _>>()
            .map_err(StoreError::from)?
    };
    for instance in instances {
        record_changed(tx, InstanceId(instance))?;
    }
    Ok(CatalogOutput::AgentUpdated { agent, row_version })
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
                identifiers: Vec::new(),
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
                identifiers: Vec::new(),
            };
            let next = next_position(
                connection,
                "SELECT coalesce(max(position) + 1, 0) FROM work_subjects WHERE work_id = ?1",
                work.0,
            )?;
            link_subjects_at(connection, work.0, next, &[subject])?;
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

impl AgentReader for ElzevirStore {
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
                })
            })
            .transpose()
    }
}
