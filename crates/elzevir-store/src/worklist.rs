// SPDX-License-Identifier: GPL-3.0-or-later
//! The family heads of the work lists (docs/specs/catalogo/facetas.md, part c), read from
//! `audit_log` through `audit_entities`, as the history is.

use std::num::NonZeroUsize;

use elzevir_app::catalog::CatalogReadError;
use elzevir_app::listing::FamilyHead;
use elzevir_app::worklist::{LIST_COMMANDS, SavedList, SavedListId, WorkListQuery, WorkListReader};
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
/// Read newest first through the partial index `audit_imports` (migration 43), whose
/// condition this one repeats, so a day of a million imports reads at most `?4` entries
/// and parses no command.
const CHOSEN_IMPORTS: &str = "
    WITH chosen AS (
        SELECT seq FROM audit_log INDEXED BY audit_imports
        WHERE (kind = 'catalog.import_record'
               OR (kind = 'catalog.add_record'
                   AND json_extract(command, '$.Catalog.AddRecord.source') IS NOT NULL))
          AND at >= ?1
        ORDER BY at DESC, seq DESC LIMIT ?4
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

    fn saved_lists(&self) -> Result<Vec<SavedList>, CatalogReadError> {
        self.readers()
            .read(saved_lists)
            .map_err(|e| CatalogReadError(e.to_string()))
    }
}

fn saved_lists(c: &Connection) -> Result<Vec<SavedList>, rusqlite::Error> {
    let mut statement = c.prepare_cached(
        "SELECT l.id, l.name, count(f.head) FROM saved_lists l
         LEFT JOIN saved_list_families f ON f.list_id = l.id
         GROUP BY l.id ORDER BY l.name COLLATE NOCASE, l.id",
    )?;
    statement
        .query_map([], |row| {
            Ok(SavedList {
                id: SavedListId(row.get(0)?),
                name: row.get(1)?,
                families: usize::try_from(row.get::<_, i64>(2)?).unwrap_or(usize::MAX),
            })
        })?
        .collect()
}

/// A saved list's heads, the last added first.
fn saved_heads(
    c: &Connection,
    list: SavedListId,
    offset: i64,
    limit: i64,
) -> Result<Vec<FamilyHead>, rusqlite::Error> {
    let mut statement = c.prepare_cached(
        "SELECT head FROM saved_list_families WHERE list_id = ?1
         ORDER BY added DESC LIMIT ?2 OFFSET ?3",
    )?;
    let names = statement.query_map(params![list.0, limit, offset], |row| {
        row.get::<_, String>(0)
    })?;
    let mut heads = Vec::new();
    for name in names {
        // Every stored head was written by `FamilyHead::name`.
        heads.extend(FamilyHead::from_name(&name?));
    }
    Ok(heads)
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
        // A saved list is read in the order of its own table, not from the log.
        WorkListQuery::Saved(list) => return saved_heads(c, list, offset, limit),
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
