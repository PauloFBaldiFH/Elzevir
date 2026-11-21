// SPDX-License-Identifier: GPL-3.0-or-later
//! Importing a record against the stored ones, and replacing a stored record
//! (docs/specs/catalogo/casamento.md).

use std::collections::BTreeSet;

use elzevir_app::catalog::{
    CatalogRefusal, ImportOutcome, ImportRecord, ImportedItems, ItemData, NewRecord, RecordItem,
    RefusedItem,
};
use elzevir_app::commands::WriteRefusal;
use elzevir_app::matching::{ItemsDecision, MatchResult, RecordAction, decide, match_keys};
use elzevir_domain::catalog::{InstanceId, ItemId, Vocabulary};
use rusqlite::{Connection, OptionalExtension, params};

use crate::catalog::{
    Places, add_work_and_instance, insert_identifiers, insert_notes, item_codes, link_languages,
    optional_term, record_changed, reindex, store_source_and_keys,
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
    let scope = match_scope(
        connection,
        &import.record,
        import.matching,
        import.record.instance.origin.as_deref(),
    )?;
    if import
        .record
        .instance
        .reviewed
        .as_deref()
        .is_some_and(|expected| *expected != scope)
    {
        return Err(crate::catalog::refused(CatalogRefusal::ImportConflict(
            "catalog changed after preview".into(),
        )));
    }
    let matched = match scope.result() {
        MatchResult::Absent => None,
        MatchResult::Unique(id) => Some(id),
        MatchResult::Ambiguous => {
            return Err(crate::catalog::refused(CatalogRefusal::ImportConflict(
                "multiple candidate Instances".into(),
            )));
        }
        MatchResult::Conflict(reason) => {
            return Err(crate::catalog::refused(CatalogRefusal::ImportConflict(
                reason.into(),
            )));
        }
    };
    let decision = decide(&import.matching, matched.is_some());
    let record = &import.record;
    if matched.is_some()
        && record.instance.reviewed.is_none()
        && (decision.record == RecordAction::Replace || decision.items == ItemsDecision::Replace)
    {
        return Err(crate::catalog::refused(CatalogRefusal::ImportConflict(
            "replacement requires a confirmed preview".into(),
        )));
    }
    match (decision.record, matched) {
        (RecordAction::Ignore | RecordAction::Replace, None) => Ok(ImportOutcome::Skipped),
        (RecordAction::Ignore, Some(instance)) => {
            let items = import_items(tx, instance, record, decision.items, None)?;
            if decision.items == ItemsDecision::Replace || record.instance.reviewed.is_some() {
                reject_partial(&items)?;
            }
            Ok(ImportOutcome::Matched { instance, items })
        }
        (RecordAction::Replace, Some(instance)) => {
            replace_imported_description(tx, instance, record)?;
            let items = import_items(tx, instance, record, decision.items, Some(instance))?;
            reject_partial(&items)?;
            Ok(ImportOutcome::Replaced { instance, items })
        }
        (RecordAction::CreateNew, matched) => {
            let (_, instance) = add_work_and_instance(connection, record)?;
            reindex(tx, instance)?;
            let items = import_items(tx, instance, record, decision.items, Some(instance))?;
            if record.instance.reviewed.is_some() {
                reject_partial(&items)?;
            }
            Ok(ImportOutcome::Created {
                instance,
                matched,
                items,
            })
        }
    }
}

fn replace_imported_description(
    tx: &WriteTx<'_>,
    instance: InstanceId,
    record: &NewRecord,
) -> Result<(), CommandFailed<WriteRefusal>> {
    let connection = tx.connection();
    let mut overlay = record.clone();
    for local in crate::export::notes(connection, instance.0)
        .map_err(|e| StoreError::Sqlite(e.to_string()))?
    {
        if local.kind == elzevir_domain::catalog::NoteKind::Local {
            if let Some(incoming) = overlay
                .instance
                .notes
                .iter_mut()
                .find(|n| n.kind == local.kind && n.text == local.text)
            {
                if incoming.visibility.is_none() {
                    incoming.visibility = local.visibility;
                }
            } else {
                overlay.instance.notes.push(local);
            }
        }
    }
    let previous: Option<Vec<u8>> = connection
        .query_row(
            "SELECT record FROM source_records WHERE instance_id=?1",
            [instance.0],
            |r| r.get(0),
        )
        .optional()
        .map_err(StoreError::from)?;
    if let Some(old) = previous {
        if let Some(new) = &overlay.source {
            overlay.source = Some(
                elzevir_app::marc_mapping::preserve_local_fields(&old, new).map_err(|reason| {
                    crate::catalog::refused(CatalogRefusal::ImportConflict(reason))
                })?,
            );
        } else {
            // Native descriptions have no MARC source; retain the original as the export base.
            overlay.source = Some(elzevir_app::marc_mapping::local_fields_only(&old).map_err(
                |reason| crate::catalog::refused(CatalogRefusal::ImportConflict(reason)),
            )?);
        }
    }
    replace_description(tx, instance, &overlay)
}

fn reject_partial(items: &ImportedItems) -> Result<(), CommandFailed<WriteRefusal>> {
    if let Some(refusal) = items.refused.first() {
        return Err(crate::catalog::refused(refusal.refusal.clone()));
    }
    Ok(())
}

fn candidate_ids(
    c: &Connection,
    key: &elzevir_app::matching::MatchKey,
    origin: Option<&str>,
) -> Result<BTreeSet<InstanceId>, StoreError> {
    use elzevir_domain::catalog::MatchRule;
    let mut ids = BTreeSet::new();
    if key.rule == MatchRule::KohaBiblionumber
        && let Some(origin) = origin
    {
        let mut q = c.prepare_cached("SELECT instance_id FROM import_identities WHERE origin=?1 AND entity='instance' AND system='koha.biblionumber' AND external_id=?2
                UNION SELECT instance_id FROM match_keys AS k WHERE rule=?3 AND key=?2 AND NOT EXISTS
                (SELECT 1 FROM import_identities i WHERE i.instance_id=k.instance_id AND i.system='koha.biblionumber' AND i.external_id=k.key)")?;
        for id in q.query_map(params![origin, key.key, key.rule.code()], |r| {
            r.get::<_, i64>(0)
        })? {
            ids.insert(InstanceId(id?));
        }
    } else {
        let mut q =
            c.prepare_cached("SELECT instance_id FROM match_keys WHERE rule=?1 AND key=?2")?;
        for id in q.query_map(params![key.rule.code(), key.key], |r| r.get::<_, i64>(0))? {
            ids.insert(InstanceId(id?));
        }
    }
    Ok(ids)
}

fn known_provenance(
    c: &Connection,
    id: InstanceId,
    record: &NewRecord,
    rule: elzevir_domain::catalog::MatchRule,
    origin: Option<&str>,
) -> Result<bool, StoreError> {
    let Some(origin) = origin else {
        return Ok(false);
    };
    if rule == elzevir_domain::catalog::MatchRule::KohaBiblionumber {
        let key = record
            .source
            .as_deref()
            .and_then(elzevir_app::matching::koha_biblionumber_key);
        return Ok(c.query_row(
            "SELECT EXISTS(SELECT 1 FROM import_identities WHERE instance_id=?1
                 AND origin=?2 AND system='koha.biblionumber' AND external_id=?3)",
            params![id.0, origin, key.map(|k| k.key)],
            |r| r.get(0),
        )?);
    }
    Ok(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM instance_origins WHERE instance_id=?1)",
        [id.0],
        |r| r.get(0),
    )?)
}

fn identifiers_disagree(
    c: &Connection,
    record: &NewRecord,
    rule: elzevir_domain::catalog::MatchRule,
    origin: Option<&str>,
    candidate: InstanceId,
) -> Result<bool, StoreError> {
    use elzevir_domain::catalog::MatchRule;
    if let (Some(origin), Some((system, value))) =
        (origin, elzevir_app::matching::source_control_number(record))
    {
        let divergent: bool = c.query_row(
            "SELECT EXISTS(SELECT 1 FROM import_identities
            WHERE origin=?1 AND system=?2 AND external_id=?3 AND instance_id<>?4)",
            params![origin, system, value, candidate.0],
            |r| r.get(0),
        )?;
        if divergent {
            return Ok(true);
        }
    }
    for key in match_keys(record)
        .into_iter()
        .filter(|key| key.rule != rule)
    {
        if key.rule == MatchRule::KohaBiblionumber && origin.is_none() {
            continue;
        }
        let others = candidate_ids(c, &key, origin)?;
        if !others.is_empty() && !others.contains(&candidate) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Read exactly the catalog evidence used by both review and commit.
pub(crate) fn match_scope(
    c: &Connection,
    record: &NewRecord,
    rules: elzevir_app::matching::Matching,
    origin: Option<&str>,
) -> Result<elzevir_app::matching::MatchScope, StoreError> {
    use elzevir_app::matching::MatchScope;
    let mut ids = BTreeSet::new();
    for key in match_keys(record)
        .into_iter()
        .filter(|key| key.rule == rules.rule)
    {
        ids.extend(candidate_ids(c, &key, origin)?);
    }
    let mut scope = MatchScope {
        candidates: ids.into_iter().collect(),
        ..MatchScope::default()
    };
    let action = decide(&rules, !scope.candidates.is_empty());
    let changing = action.record == RecordAction::Replace || action.items == ItemsDecision::Replace;
    if changing
        && let [candidate] = scope.candidates.as_slice()
        && identifiers_disagree(c, record, rules.rule, origin, *candidate)?
    {
        scope.conflict = Some("bibliographic identifiers point to different Instances".into());
    }
    let mut editions = BTreeSet::new();
    for id in &scope.candidates {
        let mut q = c.prepare_cached(
            "SELECT w.id,w.row_version,s.id,s.row_version FROM instances target
             JOIN works w ON w.id=target.work_id JOIN instances s ON s.work_id=w.id
             WHERE target.id=?1",
        )?;
        for row in q.query_map([id.0], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))? {
            editions.insert(row?);
        }
        if changing && !known_provenance(c, *id, record, rules.rule, origin)? {
            scope.conflict = Some("replacement requires known provenance".into());
        }
    }
    scope.editions = editions.into_iter().collect();
    let mut items = BTreeSet::new();
    for (_, _, id, _) in &scope.editions {
        let mut q = c.prepare_cached("SELECT id FROM items WHERE instance_id=?1")?;
        for row in q.query_map([id], |r| r.get::<_, i64>(0))? {
            items.insert(row?);
        }
    }
    let sources = elzevir_app::matching::koha_item_sources(record);
    let mut seen = BTreeSet::new();
    for (position, item) in record.items.entries().enumerate() {
        if changing
            && let Some(barcode) = item.barcode()
            && !seen.insert(barcode.to_owned())
        {
            scope.conflict = Some("duplicate barcode in replacement".into());
        }
        if changing
            && let (Some(origin), Some(external)) = (
                origin,
                sources.get(position).and_then(|s| s.itemnumber.as_deref()),
            )
        {
            let holder: Option<(i64, i64, Option<String>)> = c
                .query_row(
                    "SELECT i.id, i.instance_id, i.barcode FROM item_origins k
                 JOIN items i ON i.id = k.item_id
                 WHERE k.origin = ?1 AND k.system = 'koha.itemnumber' AND k.external_id = ?2",
                    params![origin, external],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;
            if let Some((id, owner, barcode)) = holder {
                items.insert(id);
                if scope.candidates.as_slice() != [InstanceId(owner)]
                    || barcode.as_deref() != item.barcode()
                    || barcode.is_none()
                {
                    scope.conflict =
                        Some("external itemnumber conflicts with Item ownership or barcode".into());
                }
            }
        }
        if let Some(barcode) = item.barcode()
            && let Some((item, owner)) = item_by_barcode(c, barcode)?
        {
            items.insert(item.0);
            if changing
                && (scope.candidates.as_slice() != [owner]
                    || action.record == RecordAction::CreateNew)
            {
                scope.conflict = Some("barcode belongs to another Instance".into());
            }
        }
    }
    scope.holdings = holding_scope(c, items)?;
    Ok(scope)
}

fn holding_scope(
    c: &Connection,
    items: BTreeSet<i64>,
) -> Result<Vec<elzevir_app::matching::HoldingScope>, StoreError> {
    use elzevir_app::matching::HoldingScope;
    let mut holdings = Vec::with_capacity(items.len());
    let mut q = c.prepare_cached(
        "SELECT i.id, i.instance_id, i.row_version, b.name, l.id, l.due
         FROM items i JOIN branches b ON b.id=i.holding_branch
         LEFT JOIN loans l ON l.item_id=i.id WHERE i.id=?1",
    )?;
    for item in items {
        holdings.push(q.query_row([item], |r| {
            Ok(HoldingScope {
                item: r.get(0)?,
                instance: r.get(1)?,
                version: r.get(2)?,
                branch: r.get(3)?,
                loan: r.get::<_, Option<i64>>(4)?.zip(r.get(5)?),
            })
        })?);
    }
    Ok(holdings)
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
    record: &NewRecord,
    decision: ItemsDecision,
    fresh: Option<InstanceId>,
) -> Result<ImportedItems, CommandFailed<WriteRefusal>> {
    let connection = tx.connection();
    let mut done = ImportedItems::default();
    if decision == ItemsDecision::Ignore {
        return Ok(done);
    }
    let mut changed = BTreeSet::new();
    let sources = elzevir_app::matching::koha_item_sources(record);
    for (position, item) in record.items.entries().enumerate() {
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
                    RecordItem::Marc(item) => {
                        replace_item(connection, id, item, sources.get(position))
                    }
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
                let id = if stored.is_some() {
                    done.replaced.last()
                } else {
                    done.added.last()
                };
                if let Some(id) = id {
                    crate::catalog::remember_item(
                        connection,
                        *id,
                        record.instance.origin.as_deref(),
                        sources.get(position),
                    )?;
                }
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
    source: Option<&elzevir_app::matching::KohaItemSource>,
) -> Result<(), CommandFailed<WriteRefusal>> {
    let has = |code| source.is_none_or(|s| s.supplied.contains(&code));
    let resolved = item_codes(connection, item)?;
    crate::items::check_retained_places(connection, id, &resolved)?;
    let Places {
        home,
        holding,
        policy,
        location,
        collection,
    } = resolved;
    connection
        .execute(
            "UPDATE items SET barcode = ?2, inventory_number = COALESCE(?3, inventory_number), home_branch = ?4,
                 holding_branch = COALESCE(?5,holding_branch), policy_id = ?6, call_number = COALESCE(?7, call_number), sublocation_id = COALESCE(?8, sublocation_id),
                 collection_id = COALESCE(?9, collection_id), price_cents = COALESCE(?10, price_cents), replacement_price_cents = COALESCE(?11, replacement_price_cents),
                 copy_number = COALESCE(?12, copy_number), not_for_loan = COALESCE(?13,not_for_loan), damaged = COALESCE(?14,damaged), lost = COALESCE(?15,lost),
                 withdrawn = COALESCE(?16,withdrawn), restricted = COALESCE(?17,restricted), acquired_on = COALESCE(?18, acquired_on),
                 row_version = row_version + 1
             WHERE id = ?1",
            params![
                id.0,
                item.barcode,
                item.inventory_number,
                home,
                has(b'b').then_some(holding),
                policy,
                item.call_number.as_deref().filter(|_| has(b'o')),
                location,
                collection,
                item.price.map(|m| m.0),
                item.replacement_price.map(|m| m.0),
                item.copy_number,
                has(b'7').then_some(item.status.not_for_loan),
                has(b'4').then_some(item.status.damaged),
                has(b'1').then_some(item.status.lost),
                has(b'0').then_some(item.status.withdrawn),
                has(b'5').then_some(item.status.restricted),
                item.acquired_on
            ],
        )
        .map_err(StoreError::from)?;
    crate::catalog::store_issue(connection, id, item)?;
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
    crate::catalog::link_contributions_at_from(
        connection,
        work,
        0,
        &record.contributions,
        crate::catalog::HeadingSource::of(record),
    )?;
    crate::catalog::link_subjects_at_from(
        connection,
        work,
        0,
        &record.subjects,
        crate::catalog::HeadingSource::of(record),
    )?;
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
