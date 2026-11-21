// SPDX-License-Identifier: GPL-3.0-or-later
//! A reviewed Biblivre holding is identified by installation and tombo, never by barcode.

use crate::{CommandFailed, StoreError, WriteTx, catalog};
use elzevir_app::catalog::{CatalogRefusal, ImportOutcome, ImportedItems, NativeItem, NewRecord};
use elzevir_app::commands::WriteRefusal;
use elzevir_app::matching::{MatchResult, MatchScope, source_control_number};
use elzevir_domain::catalog::InstanceId;
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::BTreeSet;

fn conflict(reason: impl Into<String>) -> CommandFailed<WriteRefusal> {
    catalog::refused(CatalogRefusal::ImportConflict(reason.into()))
}

pub(crate) fn scope(
    c: &Connection,
    record: &NewRecord,
    items: &[NativeItem],
) -> Result<MatchScope, StoreError> {
    let mut scope = MatchScope::default();
    let (Some(origin), Some((system, number))) = (
        record.instance.origin.as_deref(),
        source_control_number(record),
    ) else {
        scope.conflict = Some("missing Biblivre control number or installation".into());
        return Ok(scope);
    };
    let mut candidates = c.prepare_cached(
        "SELECT i.id, w.id, w.row_version, i.row_version, s.record IS ?4
         FROM import_identities k
         JOIN instances i ON i.id = k.instance_id
         JOIN works w ON w.id = i.work_id
         LEFT JOIN source_records s ON s.instance_id = i.id
         WHERE k.origin = ?1 AND k.entity = 'instance' AND k.system = ?2 AND k.external_id = ?3
         ORDER BY i.id",
    )?;
    let mut rows = candidates.query(params![origin, system, number, record.source])?;
    while let Some(row) = rows.next()? {
        let instance = row.get(0)?;
        scope.candidates.push(InstanceId(instance));
        scope
            .editions
            .push((row.get(1)?, row.get(2)?, instance, row.get(3)?));
        if !row.get::<_, bool>(4)? {
            scope.conflict = Some("Biblivre control number already linked to a different source record; review required".into());
        }
    }
    if let Some(item) = items.first() {
        review_destination(c, item, &mut scope)?;
    }
    let mut seen = BTreeSet::new();
    let mut holdings = BTreeSet::new();
    let mut owners = c.prepare_cached(
        "SELECT i.id, i.instance_id,
                i.inventory_number IS ?2 AND i.home_branch = ?3 AND i.holding_branch = ?4
                AND i.policy_id = ?5 AND (?6 IS NULL OR i.acquired_on IS ?6)
         FROM item_origins k JOIN items i ON i.id = k.item_id
         WHERE k.origin = ?1 AND k.system = 'biblivre.949$a' AND k.external_id = ?2",
    )?;
    let mut unlinked = c.prepare_cached(
        "SELECT EXISTS(SELECT 1 FROM items i
         WHERE i.inventory_number = ?1 AND i.home_branch = ?2 AND i.id IS NOT ?3
         AND NOT EXISTS(SELECT 1 FROM item_origins k WHERE k.item_id = i.id
                        AND k.system = 'biblivre.949$a' AND k.external_id = ?1))",
    )?;
    for item in items {
        let fields = &item.fields;
        let Some(tombo) = fields.inventory_number.as_deref().filter(|n| !n.is_empty()) else {
            scope.conflict = Some("missing Biblivre tombo".into());
            continue;
        };
        if !seen.insert(tombo) {
            scope.conflict = Some(format!("repeated Biblivre tombo {tombo}"));
        }
        let owner: Option<(i64, i64, bool)> = owners
            .query_row(
                params![
                    origin,
                    tombo,
                    fields.home_branch.0,
                    fields.holding_branch.0,
                    fields.policy.0,
                    item.acquired_on
                ],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        if let Some((id, instance, agrees)) = owner {
            holdings.insert(id);
            if scope.candidates.as_slice() == [InstanceId(instance)] && agrees {
                scope.existing_tombos.insert(tombo.to_owned());
            } else {
                scope.conflict = Some(format!(
                    "Biblivre tombo {tombo} conflicts with its owner or destination data"
                ));
            }
        }
        // A manual or legacy exemplar with the same local inventory number must be reconciled by staff.
        let existing: bool = unlinked.query_row(
            params![tombo, fields.home_branch.0, owner.map(|(id, _, _)| id)],
            |row| row.get(0),
        )?;
        if existing {
            scope.conflict = Some(format!(
                "tombo {tombo} already exists without a Biblivre identity in this library"
            ));
        }
    }
    scope.holdings = crate::matching::holding_scope(c, holdings)?;
    Ok(scope)
}

/// Include the registry labels shown to staff in the review, so renames also invalidate it.
fn review_destination(
    connection: &Connection,
    item: &NativeItem,
    scope: &mut MatchScope,
) -> Result<(), StoreError> {
    let fields = &item.fields;
    scope.biblivre_registries = connection
        .query_row(
            "SELECT h.id,h.row_version,h.name,b.id,b.row_version,b.name,p.id,p.row_version,p.name
         FROM branches h, branches b, circulation_policies p WHERE h.id=?1 AND b.id=?2 AND p.id=?3",
            params![
                fields.home_branch.0,
                fields.holding_branch.0,
                fields.policy.0
            ],
            |row| {
                Ok(Box::new([
                    (row.get(0)?, row.get(1)?, row.get(2)?),
                    (row.get(3)?, row.get(4)?, row.get(5)?),
                    (row.get(6)?, row.get(7)?, row.get(8)?),
                ]))
            },
        )
        .optional()?;
    if scope.biblivre_registries.is_none() {
        scope.conflict = Some("unknown Biblivre destination library or policy".into());
    }
    Ok(())
}

pub(crate) fn confirm(
    tx: &WriteTx<'_>,
    record: &NewRecord,
    items: &[NativeItem],
) -> Result<ImportOutcome, CommandFailed<WriteRefusal>> {
    let c = tx.connection();
    let snapshot = scope(c, record, items)?;
    if record.instance.reviewed.as_deref() != Some(&snapshot) {
        return Err(conflict("Biblivre requires a current confirmed preview"));
    }
    let matched = match snapshot.result() {
        MatchResult::Unique(id) => Some(id),
        MatchResult::Absent => None,
        MatchResult::Conflict(reason) => return Err(conflict(reason)),
        MatchResult::Ambiguous => return Err(conflict("ambiguous Biblivre control number")),
    };
    let instance = match matched {
        Some(id) => id,
        None => catalog::add_work_and_instance(c, record)?.1,
    };
    let mut done = ImportedItems::default();
    for (position, item) in items.iter().enumerate() {
        let tombo = item
            .fields
            .inventory_number
            .as_deref()
            .ok_or_else(|| conflict("missing Biblivre tombo"))?;
        if snapshot.existing_tombos.contains(tombo) {
            continue;
        }
        let id = crate::native_items::write(c, instance, None, position, item)?;
        c.execute(
            "INSERT INTO item_origins(origin, system, external_id, item_id)
             VALUES (?1, 'biblivre.949$a', ?2, ?3)",
            params![record.instance.origin, tombo, id.0],
        )
        .map_err(StoreError::from)?;
        done.added.push(id);
    }
    if matched.is_some() {
        if !done.added.is_empty() {
            catalog::record_changed(tx, instance)?;
        }
        Ok(ImportOutcome::Matched {
            instance,
            items: done,
        })
    } else {
        catalog::reindex(tx, instance)?;
        Ok(ImportOutcome::Created {
            instance,
            matched: None,
            items: done,
        })
    }
}
