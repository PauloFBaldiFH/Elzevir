// SPDX-License-Identifier: GPL-3.0-or-later
//! The history of an entity, read from `audit_log` through `audit_entities`
//! (docs/specs/catalogo/historico.md).

use elzevir_app::catalog::CatalogReadError;
use elzevir_app::history::{HistoryReader, LoggedCommand};
use elzevir_domain::statement::EntityRef;
use rusqlite::params;

use crate::ElzevirStore;
use crate::statements::kind_number;

impl HistoryReader for ElzevirStore {
    fn history(
        &self,
        entity: EntityRef,
        limit: usize,
    ) -> Result<Vec<LoggedCommand>, CatalogReadError> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        self.readers()
            .read(|c| {
                let mut statement = c.prepare_cached(
                    "SELECT l.at, s.login, l.command
                     FROM audit_entities e
                     JOIN audit_log l ON l.seq = e.audit_seq
                     LEFT JOIN staff_accounts s ON s.id = l.actor
                     WHERE e.entity_kind = ?1 AND e.entity_id = ?2
                     ORDER BY e.audit_seq DESC
                     LIMIT ?3",
                )?;
                statement
                    .query_map(
                        params![kind_number(entity.entity), entity.id, limit],
                        |row| {
                            Ok(LoggedCommand {
                                at: row.get(0)?,
                                actor: row.get(1)?,
                                command: row.get(2)?,
                            })
                        },
                    )?
                    .collect::<Result<Vec<_>, _>>()
            })
            .map_err(|e| CatalogReadError(e.to_string()))
    }
}
