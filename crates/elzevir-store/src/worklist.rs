// SPDX-License-Identifier: GPL-3.0-or-later
//! The family heads of the work lists (docs/specs/catalogo/facetas.md, part c), read from
//! `audit_log` through `audit_entities`, as the history is.

use std::num::NonZeroUsize;

use elzevir_app::catalog::CatalogReadError;
use elzevir_app::listing::FamilyHead;
use elzevir_app::worklist::{LIST_COMMANDS, WorkListQuery, WorkListReader};
use elzevir_domain::catalog::{HubId, WorkId};
use rusqlite::{Connection, ToSql, params};

use crate::ElzevirStore;

/// From the chosen commands (`chosen`, one `seq` column) to their families: a Work by its
/// own, an Instance by its Work's, each family once at its latest command.
const FAMILIES_OF_CHOSEN: &str = "
    , families AS (
        SELECT w.hub_id AS hub, w.id AS work, e.audit_seq AS seq
        FROM chosen c
        JOIN audit_entities e ON e.audit_seq = c.seq
        JOIN works w ON w.id = CASE e.entity_kind
            WHEN 2 THEN e.entity_id
            ELSE (SELECT i.work_id FROM instances i WHERE i.id = e.entity_id) END
        WHERE e.entity_kind IN (2, 3)
    )
    SELECT hub, CASE WHEN hub IS NULL THEN work END AS lone, MAX(seq) AS last
    FROM families
    GROUP BY hub, lone
    ORDER BY last DESC
    LIMIT ?2 OFFSET ?3";

/// A person's latest commands.
const CHOSEN_BY_ACTOR: &str = "
    WITH chosen AS (
        SELECT seq FROM audit_log WHERE actor = ?1 ORDER BY seq DESC LIMIT ?4
    )";

/// The latest imports from a file since an instant: `catalog.import_record`, and
/// `catalog.add_record` with its source record (a record made on the screen has none).
/// Each kind is read newest first through `audit_by_kind`, so a day of a million imports
/// still reads at most `?4` of each.
const CHOSEN_IMPORTS: &str = "
    WITH chosen AS (
        SELECT seq FROM (
            SELECT seq FROM (
                SELECT seq FROM audit_log
                WHERE kind = 'catalog.import_record' AND at >= ?1
                ORDER BY at DESC, seq DESC LIMIT ?4)
            UNION ALL
            SELECT seq FROM (
                SELECT seq FROM audit_log
                WHERE kind = 'catalog.add_record' AND at >= ?1
                  AND json_extract(command, '$.Catalog.AddRecord.source') IS NOT NULL
                ORDER BY at DESC, seq DESC LIMIT ?4))
        ORDER BY seq DESC LIMIT ?4
    )";

impl WorkListReader for ElzevirStore {
    fn heads(
        &self,
        query: WorkListQuery,
        offset: usize,
        limit: NonZeroUsize,
    ) -> Result<Vec<FamilyHead>, CatalogReadError> {
        self.readers()
            .read(|c| heads(c, query, offset, limit))
            .map_err(|e| CatalogReadError(e.to_string()))
    }
}

fn heads(
    c: &Connection,
    query: WorkListQuery,
    offset: usize,
    limit: NonZeroUsize,
) -> Result<Vec<FamilyHead>, rusqlite::Error> {
    let limit = i64::try_from(limit.get()).unwrap_or(i64::MAX);
    let offset = i64::try_from(offset).unwrap_or(i64::MAX);
    let commands = i64::try_from(LIST_COMMANDS).unwrap_or(i64::MAX);
    // Only constant fragments are joined; every value is a parameter.
    let (chosen, first): (&str, Box<dyn ToSql>) = match query {
        WorkListQuery::TouchedBy(staff) => (CHOSEN_BY_ACTOR, Box::new(staff.0)),
        WorkListQuery::ImportedSince(since) => (
            CHOSEN_IMPORTS,
            Box::new(since.strftime("%Y-%m-%dT%H:%M:%SZ").to_string()),
        ),
    };
    let sql = format!("{chosen}{FAMILIES_OF_CHOSEN}");
    let mut statement = c.prepare_cached(&sql)?;
    statement
        .query_map(params![first, limit, offset, commands], |row| {
            let hub: Option<i64> = row.get(0)?;
            let lone: Option<i64> = row.get(1)?;
            Ok(match (hub, lone) {
                (Some(hub), _) => Some(FamilyHead::Hub(HubId(hub))),
                (None, Some(work)) => Some(FamilyHead::Work(WorkId(work))),
                (None, None) => None,
            })
        })?
        .filter_map(Result::transpose)
        .collect()
}
