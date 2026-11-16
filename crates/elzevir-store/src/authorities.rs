// SPDX-License-Identifier: GPL-3.0-or-later
//! Editing an agent once for every record that names it (docs/specs/catalogo/autoridades.md).

use elzevir_app::authorities::{AgentReader, AgentView};
use elzevir_app::catalog::{
    AgentChange, AgentUpdate, CatalogOutput, CatalogReadError, CatalogRefusal,
};
use elzevir_app::commands::WriteRefusal;
use elzevir_domain::catalog::{AgentId, AgentKind, InstanceId};
use rusqlite::{OptionalExtension, params};

use crate::catalog::{record_changed, refused};
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
