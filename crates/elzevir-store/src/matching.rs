// SPDX-License-Identifier: GPL-3.0-or-later
//! Importing a record against the stored ones, and replacing a stored record
//! (docs/specs/catalogo/casamento.md).

use std::collections::BTreeSet;

use elzevir_app::catalog::{
    CatalogRefusal, ImportOutcome, ImportRecord, ImportedItems, ItemData, NewRecord, RecordItem,
    RecordItems, RefusedItem,
};
use elzevir_app::commands::WriteRefusal;
use elzevir_app::matching::{ItemsDecision, RecordAction, decide, match_keys};
use elzevir_domain::catalog::{InstanceId, ItemId, Vocabulary};
use rusqlite::{Connection, OptionalExtension, params};

use crate::catalog::{
    Places, add_work_and_instance, insert_identifiers, insert_notes, item_codes,
    link_contributions, link_languages, link_subjects, optional_term, record_changed, reindex,
    store_source_and_keys,
};
use crate::native_items;
use crate::statements::store_record_descriptions;
use crate::{CommandFailed, StoreError, WriteTx};

/// Imports one record: finds the stored record it matches, then does what the matching
/// says with the record and its Items.
pub(crate) fn import_record(
    tx: &WriteTx<'_>,
    import: &ImportRecord,
) -> Result<ImportOutcome, CommandFailed<WriteRefusal>> {
    native_items::check_record(&import.record)?;
    let connection = tx.connection();
    let matched = find_match(connection, import)?;
    let decision = decide(&import.matching, matched.is_some());
    let record = &import.record;
    match (decision.record, matched) {
        (RecordAction::Ignore | RecordAction::Replace, None) => Ok(ImportOutcome::Skipped),
        (RecordAction::Ignore, Some(instance)) => {
            let items = import_items(tx, instance, &record.items, decision.items, None)?;
            Ok(ImportOutcome::Matched { instance, items })
        }
        (RecordAction::Replace, Some(instance)) => {
            replace_description(tx, instance, record)?;
            let items = import_items(tx, instance, &record.items, decision.items, Some(instance))?;
            Ok(ImportOutcome::Replaced { instance, items })
        }
        (RecordAction::CreateNew, matched) => {
            let (_, instance) = add_work_and_instance(connection, record)?;
            reindex(tx, instance)?;
            let items = import_items(tx, instance, &record.items, decision.items, Some(instance))?;
            Ok(ImportOutcome::Created {
                instance,
                matched,
                items,
            })
        }
    }
}

/// The oldest stored Instance that shares a key with the record under its rule.
fn find_match(
    connection: &Connection,
    import: &ImportRecord,
) -> Result<Option<InstanceId>, StoreError> {
    let mut best: Option<i64> = None;
    let mut query = connection
        .prepare_cached("SELECT min(instance_id) FROM match_keys WHERE rule = ?1 AND key = ?2")?;
    for key in match_keys(&import.record)
        .into_iter()
        .filter(|key| key.rule == import.matching.rule)
    {
        let found: Option<i64> =
            query.query_row(params![key.rule.code(), key.key], |row| row.get(0))?;
        best = match (best, found) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
    }
    Ok(best.map(InstanceId))
}

/// Stores the Items of an imported record one at a time: a refused Item is reported and
/// the next one goes on. Every Instance whose Items changed gets a new version and is
/// reindexed, except `fresh`, which the caller has already taken care of.
///
/// Koha: `C4::ImportBatch::_batchCommitItems` (a barcode already in use is an error for that
/// Item only, or, when replacing, overwrites the stored Item where it is).
fn import_items(
    tx: &WriteTx<'_>,
    instance: InstanceId,
    items: &RecordItems,
    decision: ItemsDecision,
    fresh: Option<InstanceId>,
) -> Result<ImportedItems, CommandFailed<WriteRefusal>> {
    let connection = tx.connection();
    let mut done = ImportedItems::default();
    if decision == ItemsDecision::Ignore {
        return Ok(done);
    }
    let mut changed = BTreeSet::new();
    for (position, item) in items.entries().enumerate() {
        let stored = match (item.barcode(), decision) {
            (Some(barcode), ItemsDecision::Replace) => item_by_barcode(connection, barcode)?,
            _ => None,
        };
        // A savepoint per Item, so a refused one leaves nothing behind (a location term
        // created just before its barcode was found taken, for instance).
        savepoint(connection, "SAVEPOINT import_item")?;
        let result = match stored {
            Some((id, owner)) => {
                let replacement = match item {
                    RecordItem::Marc(item) => replace_item(connection, id, item),
                    RecordItem::Native(item) => {
                        native_items::write(connection, owner, Some(id), position, item).map(|_| ())
                    }
                };
                replacement.map(|()| {
                    done.replaced.push(id);
                    owner
                })
            }
            None => native_items::add(connection, instance, position, item).map(|id| {
                done.added.push(id);
                instance
            }),
        };
        match result {
            Ok(owner) => {
                savepoint(connection, "RELEASE import_item")?;
                changed.insert(owner);
            }
            Err(CommandFailed::Rejected(WriteRefusal::Catalog(refusal))) => {
                savepoint(connection, "ROLLBACK TO import_item; RELEASE import_item")?;
                done.refused.push(RefusedItem { position, refusal });
            }
            Err(other) => return Err(other),
        }
    }
    for owner in changed {
        if Some(owner) != fresh {
            record_changed(tx, owner)?;
        }
    }
    Ok(done)
}

fn savepoint(connection: &Connection, sql: &str) -> Result<(), StoreError> {
    Ok(connection.execute_batch(sql)?)
}

/// The stored Item with `barcode`, and its Instance.
fn item_by_barcode(
    connection: &Connection,
    barcode: &str,
) -> Result<Option<(ItemId, InstanceId)>, StoreError> {
    Ok(connection
        .query_row(
            "SELECT id, instance_id FROM items WHERE barcode = ?1",
            params![barcode],
            |row| Ok((ItemId(row.get(0)?), InstanceId(row.get(1)?))),
        )
        .optional()?)
}

/// Overwrites a stored Item with an imported one, leaving it on its Instance.
///
/// Koha: `ModItemFromMarc`, called by `_batchCommitItems` with `item_action = replace`.
fn replace_item(
    connection: &Connection,
    id: ItemId,
    item: &ItemData,
) -> Result<(), CommandFailed<WriteRefusal>> {
    let Places {
        home,
        holding,
        policy,
        location,
        collection,
    } = item_codes(connection, item)?;
    connection
        .execute(
            "UPDATE items SET barcode = ?2, inventory_number = ?3, home_branch = ?4,
                 holding_branch = ?5, policy_id = ?6, call_number = ?7, sublocation_id = ?8,
                 collection_id = ?9, price_cents = ?10, replacement_price_cents = ?11,
                 copy_number = ?12, not_for_loan = ?13, damaged = ?14, lost = ?15,
                 withdrawn = ?16, restricted = ?17, acquired_on = ?18,
                 row_version = row_version + 1
             WHERE id = ?1",
            params![
                id.0,
                item.barcode,
                item.inventory_number,
                home,
                holding,
                policy,
                item.call_number,
                location,
                collection,
                item.price.map(|m| m.0),
                item.replacement_price.map(|m| m.0),
                item.copy_number,
                item.status.not_for_loan,
                item.status.damaged,
                item.status.lost,
                item.status.withdrawn,
                item.status.restricted,
                item.acquired_on
            ],
        )
        .map_err(StoreError::from)?;
    Ok(())
}

/// Replaces the description of a stored record: the fields of its Work and Instance, the
/// languages, contributions, subjects and identifiers, the source record and the match
/// keys. Ids, Items, the Hub and relations between Works stay. Every Instance of the
/// Work gets a new version and is reindexed, since their pages show the Work.
///
/// Koha: `ModBiblio`, called by `C4::ImportBatch::BatchCommitRecords` with
/// `overlay_action = replace`.
pub(crate) fn replace_description(
    tx: &WriteTx<'_>,
    instance: InstanceId,
    record: &NewRecord,
) -> Result<(), CommandFailed<WriteRefusal>> {
    let connection = tx.connection();
    let work: i64 = lookup_work(connection, instance)?
        .ok_or_else(|| crate::catalog::refused(CatalogRefusal::UnknownInstance(instance)))?;
    let scheme = optional_term(
        connection,
        Vocabulary::ClassificationScheme,
        record.classification_scheme.as_deref(),
    )?;
    connection
        .execute(
            "UPDATE works SET title = ?2, classification = ?3, classification_scheme = ?4,
                 summary = ?5, audience = ?6, sort_key = ?7, row_version = row_version + 1
             WHERE id = ?1",
            params![
                work,
                record.work_title,
                record.classification,
                scheme,
                record.summary,
                record.audience,
                elzevir_app::listing::sort_key(&record.work_title)
            ],
        )
        .map_err(StoreError::from)?;
    for (sql, id) in [
        ("DELETE FROM work_languages WHERE work_id = ?1", work),
        ("DELETE FROM contributions WHERE work_id = ?1", work),
        ("DELETE FROM work_subjects WHERE work_id = ?1", work),
        (
            "DELETE FROM instance_identifiers WHERE instance_id = ?1",
            instance.0,
        ),
        (
            "DELETE FROM instance_notes WHERE instance_id = ?1",
            instance.0,
        ),
        (
            "DELETE FROM source_records WHERE instance_id = ?1",
            instance.0,
        ),
        ("DELETE FROM match_keys WHERE instance_id = ?1", instance.0),
    ] {
        connection
            .execute(sql, params![id])
            .map_err(StoreError::from)?;
    }
    link_languages(connection, work, &record.languages)?;
    link_contributions(connection, work, &record.contributions)?;
    link_subjects(connection, work, &record.subjects)?;
    let description = &record.instance;
    connection
        .execute(
            "UPDATE instances SET title = ?2, edition = ?3, place = ?4, publisher = ?5, date = ?6,
                 extent = ?7, series = ?8
             WHERE id = ?1",
            params![
                instance.0,
                description.title,
                description.edition,
                description.place,
                description.publisher,
                description.date,
                description.extent,
                description.series
            ],
        )
        .map_err(StoreError::from)?;
    insert_identifiers(connection, instance.0, &description.identifiers)?;
    insert_notes(connection, instance.0, &description.notes)?;
    store_record_descriptions(connection, work, instance.0, record)?;
    store_source_and_keys(connection, instance, record)?;
    let mut siblings = connection
        .prepare_cached("SELECT id FROM instances WHERE work_id = ?1 ORDER BY id")
        .map_err(StoreError::from)?;
    let ids = siblings
        .query_map(params![work], |row| row.get(0))
        .and_then(Iterator::collect::<Result<Vec<i64>, _>>)
        .map_err(StoreError::from)?;
    for id in ids {
        record_changed(tx, InstanceId(id))?;
    }
    Ok(())
}

fn lookup_work(connection: &Connection, instance: InstanceId) -> Result<Option<i64>, StoreError> {
    Ok(connection
        .query_row(
            "SELECT work_id FROM instances WHERE id = ?1",
            params![instance.0],
            |row| row.get(0),
        )
        .optional()?)
}
