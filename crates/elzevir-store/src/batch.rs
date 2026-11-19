// SPDX-License-Identifier: GPL-3.0-or-later
//! `catalog.change_items`: every Item of some Instances changed in one transaction, all
//! or nothing (docs/specs/catalogo/lote.md, items 10 to 13).

use elzevir_app::batch::{ItemRefusal, ItemsChange, MAX_BATCH_ITEMS};
use elzevir_app::catalog::{CatalogOutput, CatalogRefusal};
use elzevir_app::commands::WriteRefusal;
use elzevir_app::items::ItemFields;
use elzevir_domain::catalog::{
    BranchId, CollectionId, InstanceId, ItemId, PolicyId, SublocationId,
};
use rusqlite::{Connection, params};

use crate::catalog::{record_changed, refused};
use crate::items::check_places;
use crate::{CommandFailed, StoreError, WriteTx};

/// One Item as the batch reads it.
struct Held {
    id: ItemId,
    label: String,
    fields: ItemFields,
}

/// `catalog.change_items`.
pub(crate) fn change_items(
    tx: &WriteTx<'_>,
    change: &ItemsChange,
) -> Result<CatalogOutput, CommandFailed<WriteRefusal>> {
    let connection = tx.connection();
    if change.set.is_empty() || change.instances.is_empty() {
        return Err(refused(CatalogRefusal::NothingToChange));
    }
    let mut instances: Vec<InstanceId> = Vec::with_capacity(change.instances.len());
    for &instance in &change.instances {
        if !instances.contains(&instance) {
            instances.push(instance);
        }
    }
    let mut items = Vec::new();
    for &instance in &instances {
        let exists = connection
            .prepare_cached("SELECT 1 FROM instances WHERE id = ?1")
            .and_then(|mut statement| statement.exists(params![instance.0]))
            .map_err(StoreError::from)?;
        if !exists {
            return Err(refused(CatalogRefusal::UnknownInstance(instance)));
        }
        items.extend(held(connection, instance).map_err(StoreError::from)?);
        if items.len() > MAX_BATCH_ITEMS {
            // Counted in full, so the refusal says how far over the batch goes.
            let count = count_items(connection, &instances)?;
            return Err(refused(CatalogRefusal::TooManyItems {
                count,
                max: MAX_BATCH_ITEMS,
            }));
        }
    }
    let mut refusals = Vec::new();
    for item in &mut items {
        for set in &change.set {
            set.apply(&mut item.fields);
        }
        check_places(connection, &item.fields, |column, problem| {
            refusals.push(ItemRefusal {
                item: item.id,
                label: item.label.clone(),
                column,
                problem,
            });
        })?;
    }
    if !refusals.is_empty() {
        return Err(refused(CatalogRefusal::BatchItems(refusals)));
    }
    for item in &items {
        connection
            .prepare_cached(
                "UPDATE items SET home_branch = ?2, holding_branch = ?3, policy_id = ?4,
                                  sublocation_id = ?5, collection_id = ?6,
                                  row_version = row_version + 1
                 WHERE id = ?1",
            )
            .and_then(|mut statement| {
                statement.execute(params![
                    item.id.0,
                    item.fields.home_branch.0,
                    item.fields.holding_branch.0,
                    item.fields.policy.0,
                    item.fields.location.map(|id| id.0),
                    item.fields.collection.map(|id| id.0),
                ])
            })
            .map_err(StoreError::from)?;
    }
    for &instance in &instances {
        record_changed(tx, instance)?;
    }
    Ok(CatalogOutput::ItemsChanged {
        instances,
        items: items.len(),
    })
}

/// The Items of `instance`, in id order, with the columns a batch changes.
fn held(connection: &Connection, instance: InstanceId) -> Result<Vec<Held>, rusqlite::Error> {
    let mut statement = connection.prepare_cached(
        "SELECT id, barcode, inventory_number, home_branch, holding_branch, policy_id,
                sublocation_id, collection_id
         FROM items WHERE instance_id = ?1 ORDER BY id",
    )?;
    statement
        .query_map(params![instance.0], |row| {
            let id: i64 = row.get(0)?;
            let barcode: Option<String> = row.get(1)?;
            let inventory: Option<String> = row.get(2)?;
            Ok(Held {
                id: ItemId(id),
                label: barcode.or(inventory).unwrap_or_else(|| id.to_string()),
                fields: ItemFields {
                    barcode: None,
                    inventory_number: None,
                    home_branch: BranchId(row.get(3)?),
                    holding_branch: BranchId(row.get(4)?),
                    policy: PolicyId(row.get(5)?),
                    call_number: None,
                    location: row.get::<_, Option<i64>>(6)?.map(SublocationId),
                    collection: row.get::<_, Option<i64>>(7)?.map(CollectionId),
                    copy_number: None,
                    price: None,
                },
            })
        })?
        .collect()
}

fn count_items(connection: &Connection, instances: &[InstanceId]) -> Result<usize, StoreError> {
    let mut count = 0_usize;
    for instance in instances {
        let n: i64 = connection.query_row(
            "SELECT count(*) FROM items WHERE instance_id = ?1",
            params![instance.0],
            |row| row.get(0),
        )?;
        count = count.saturating_add(usize::try_from(n).unwrap_or(usize::MAX));
    }
    Ok(count)
}
