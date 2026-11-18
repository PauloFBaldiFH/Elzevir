// SPDX-License-Identifier: GPL-3.0-or-later
//! An Instance's Items saved as one batch, all or nothing, and read for the grid
//! (docs/specs/catalogo/exemplares.md).

use std::collections::HashMap;

use elzevir_app::catalog::{
    CatalogOutput, CatalogReadError, CatalogRefusal, CodeLists, ItemData, ItemStatus, Named, Place,
};
use elzevir_app::codes::{CodeTable, NamedCode};
use elzevir_app::commands::WriteRefusal;
use elzevir_app::items::{
    GridItem, ItemColumn, ItemFields, ItemGridSource, ItemRow, ItemsUpdate, RowProblem, RowRefusal,
    repeated_barcodes,
};
use elzevir_domain::catalog::{
    BranchId, CollectionId, InstanceId, ItemId, Money, PolicyId, SublocationId,
};
use rusqlite::{Connection, OptionalExtension, params};

use crate::catalog::{Places, insert_item, lookup, record_changed, refused, status_at};
use crate::editing::{check_version, version};
use crate::{CommandFailed, ElzevirStore, StoreError, WriteTx};

type Saved = Result<CatalogOutput, CommandFailed<WriteRefusal>>;

/// `catalog.save_items` (items 3 to 9).
pub(crate) fn save_items(tx: &WriteTx<'_>, update: &ItemsUpdate) -> Saved {
    let connection = tx.connection();
    let instance = update.instance;
    let current = version(
        connection,
        "SELECT row_version FROM instances WHERE id = ?1",
        instance.0,
    )?
    .ok_or_else(|| refused(CatalogRefusal::UnknownInstance(instance)))?;
    check_version(update.row_version, current)?;
    if update.rows.is_empty() {
        return Ok(CatalogOutput::ItemsSaved {
            instance,
            row_version: current,
            items: Vec::new(),
        });
    }
    let mut refusals = refusals(connection, instance, &update.rows)?;
    if !refusals.is_empty() {
        refusals.sort_by_key(|refusal| refusal.row);
        return Err(refused(CatalogRefusal::ItemRows(refusals)));
    }
    // Barcodes may move between rows of the batch (item 6): the existing rows let go of
    // theirs first, so no intermediate state breaks the unique index.
    for id in update.rows.iter().filter_map(|row| row.id) {
        connection
            .execute(
                "UPDATE items SET barcode = NULL WHERE id = ?1",
                params![id.0],
            )
            .map_err(StoreError::from)?;
    }
    let mut items = Vec::with_capacity(update.rows.len());
    for row in &update.rows {
        items.push(match row.id {
            Some(id) => {
                update_item(connection, id, &row.fields)?;
                id
            }
            None => add_grid_item(connection, instance, &row.fields)?,
        });
    }
    record_changed(tx, instance)?;
    let row_version = version(
        connection,
        "SELECT row_version FROM instances WHERE id = ?1",
        instance.0,
    )?
    .unwrap_or(current + 1);
    Ok(CatalogOutput::ItemsSaved {
        instance,
        row_version,
        items,
    })
}

/// Every cell of the batch the catalog refuses (item 5), found before anything is
/// written.
fn refusals(
    connection: &Connection,
    instance: InstanceId,
    rows: &[ItemRow],
) -> Result<Vec<RowRefusal>, StoreError> {
    let mut refusals = repeated_barcodes(rows);
    let mut owned = Vec::with_capacity(rows.len());
    for row in rows {
        owned.push(match row.id {
            Some(id) => {
                let instance_of_item: Option<i64> = connection
                    .query_row(
                        "SELECT instance_id FROM items WHERE id = ?1",
                        params![id.0],
                        |row| row.get(0),
                    )
                    .optional()?;
                instance_of_item == Some(instance.0)
            }
            None => true,
        });
    }
    // The barcode each existing Item of the batch ends with.
    let moving: HashMap<i64, Option<&str>> = rows
        .iter()
        .zip(&owned)
        .filter(|(_, owned)| **owned)
        .filter_map(|(row, _)| row.id.map(|id| (id.0, row.fields.barcode.as_deref())))
        .collect();
    for (position, (row, owned)) in rows.iter().zip(&owned).enumerate() {
        let mut refuse = |column, problem| {
            refusals.push(RowRefusal {
                row: position,
                column,
                problem,
            });
        };
        if !owned {
            refuse(ItemColumn::Row, RowProblem::UnknownItem);
        }
        check_places(connection, &row.fields, &mut refuse)?;
        if let Some(barcode) = &row.fields.barcode {
            let holder = lookup(
                connection,
                "SELECT id FROM items WHERE barcode = ?1",
                barcode,
            )?;
            // Taken, unless by this row's own Item or by an Item of the batch that lets
            // go of it.
            let taken = holder.is_some_and(|holder| {
                Some(ItemId(holder)) != row.id
                    && moving
                        .get(&holder)
                        .is_none_or(|ends_with| *ends_with == Some(barcode.as_str()))
            });
            if taken {
                refuse(ItemColumn::Barcode, RowProblem::BarcodeTaken);
            }
        }
    }
    Ok(refusals)
}

pub(crate) fn check_places(
    connection: &Connection,
    fields: &ItemFields,
    mut refuse: impl FnMut(ItemColumn, RowProblem),
) -> Result<(), StoreError> {
    if !exists(
        connection,
        "SELECT 1 FROM branches WHERE id = ?1",
        fields.home_branch.0,
    )? {
        refuse(ItemColumn::HomeBranch, RowProblem::UnknownBranch);
    }
    if !exists(
        connection,
        "SELECT 1 FROM branches WHERE id = ?1",
        fields.holding_branch.0,
    )? {
        refuse(ItemColumn::HoldingBranch, RowProblem::UnknownBranch);
    }
    if !exists(
        connection,
        "SELECT 1 FROM circulation_policies WHERE id = ?1",
        fields.policy.0,
    )? {
        refuse(ItemColumn::Policy, RowProblem::UnknownPolicy);
    }
    // A shelving place or collection belongs to one branch: the Item's home one.
    let in_branch = |sql: &str, id: i64| -> Result<bool, StoreError> {
        Ok(connection
            .query_row(sql, params![id, fields.home_branch.0], |_| Ok(()))
            .optional()?
            .is_some())
    };
    if let Some(location) = fields.location
        && !in_branch(
            "SELECT 1 FROM sublocations WHERE id = ?1 AND branch_id = ?2",
            location.0,
        )?
    {
        refuse(ItemColumn::Location, RowProblem::NotInBranch);
    }
    if let Some(collection) = fields.collection
        && !in_branch(
            "SELECT 1 FROM collections WHERE id = ?1 AND branch_id = ?2",
            collection.0,
        )?
    {
        refuse(ItemColumn::Collection, RowProblem::NotInBranch);
    }
    Ok(())
}

fn exists(connection: &Connection, sql: &str, id: i64) -> Result<bool, StoreError> {
    Ok(connection
        .query_row(sql, params![id], |_| Ok(()))
        .optional()?
        .is_some())
}

/// The rows a grid row names, checked in `refusals` inside the same savepoint.
fn places(fields: &ItemFields) -> Places {
    Places {
        home: fields.home_branch.0,
        holding: fields.holding_branch.0,
        policy: fields.policy.0,
        location: fields.location.map(|id| id.0),
        collection: fields.collection.map(|id| id.0),
    }
}

pub(crate) fn update_item(
    connection: &Connection,
    id: ItemId,
    fields: &ItemFields,
) -> Result<(), CommandFailed<WriteRefusal>> {
    let places = places(fields);
    connection
        .execute(
            "UPDATE items SET barcode = ?2, inventory_number = ?3, home_branch = ?4,
                              holding_branch = ?5, policy_id = ?6, call_number = ?7,
                              sublocation_id = ?8, collection_id = ?9, copy_number = ?10,
                              price_cents = ?11, row_version = row_version + 1
             WHERE id = ?1",
            params![
                id.0,
                fields.barcode,
                fields.inventory_number,
                places.home,
                places.holding,
                places.policy,
                fields.call_number,
                places.location,
                places.collection,
                fields.copy_number,
                fields.price.map(|money| money.0)
            ],
        )
        .map_err(StoreError::from)?;
    Ok(())
}

/// A new Item: the grid's columns, normal status and nothing else (item 2).
pub(crate) fn add_grid_item(
    connection: &Connection,
    instance: InstanceId,
    fields: &ItemFields,
) -> Result<ItemId, CommandFailed<WriteRefusal>> {
    let places = places(fields);
    let item = ItemData {
        barcode: fields.barcode.clone(),
        inventory_number: fields.inventory_number.clone(),
        home_branch: String::new(),
        holding_branch: String::new(),
        item_type: String::new(),
        call_number: fields.call_number.clone(),
        location: None,
        collection: None,
        price: fields.price,
        replacement_price: None,
        copy_number: fields.copy_number.clone(),
        status: ItemStatus::default(),
        acquired_on: None,
    };
    insert_item(connection, instance, &item, &places)
}

impl ItemGridSource for ElzevirStore {
    fn grid_items(&self, instance: InstanceId) -> Result<Vec<GridItem>, CatalogReadError> {
        self.readers()
            .read(|c| {
                let mut statement = c.prepare(
                    "SELECT items.id, barcode, inventory_number, home_branch, holding_branch,
                            policy_id, call_number, sublocation_id, collection_id,
                            copy_number, price_cents,
                            not_for_loan, damaged, lost, withdrawn, restricted
                     FROM items
                     WHERE instance_id = ?1 ORDER BY items.id",
                )?;
                statement
                    .query_map(params![instance.0], |row| {
                        Ok(GridItem {
                            id: ItemId(row.get(0)?),
                            fields: ItemFields {
                                barcode: row.get(1)?,
                                inventory_number: row.get(2)?,
                                home_branch: BranchId(row.get(3)?),
                                holding_branch: BranchId(row.get(4)?),
                                policy: PolicyId(row.get(5)?),
                                call_number: row.get(6)?,
                                location: row.get::<_, Option<i64>>(7)?.map(SublocationId),
                                collection: row.get::<_, Option<i64>>(8)?.map(CollectionId),
                                copy_number: row.get(9)?,
                                price: row.get::<_, Option<i64>>(10)?.map(Money),
                            },
                            status: status_at(row, 11)?,
                        })
                    })?
                    .collect::<Result<Vec<_>, _>>()
            })
            .map_err(|e| CatalogReadError(e.to_string()))
    }
}

/// The registries with their names, by name (item 29).
pub(crate) fn code_lists(c: &Connection) -> Result<CodeLists, rusqlite::Error> {
    let mut branches = c.prepare("SELECT id, name FROM branches ORDER BY name, id")?;
    let branches = branches
        .query_map([], |row| {
            Ok(Named {
                id: BranchId(row.get(0)?),
                name: row.get(1)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut policies = c.prepare("SELECT id, name FROM circulation_policies ORDER BY name, id")?;
    let policies = policies
        .query_map([], |row| {
            Ok(Named {
                id: PolicyId(row.get(0)?),
                name: row.get(1)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(CodeLists {
        branches,
        contacts: crate::registries::contacts(c)?,
        policies,
        locations: places_of(
            c,
            "SELECT id, branch_id, name FROM sublocations ORDER BY name, id",
            SublocationId,
        )?,
        collections: places_of(
            c,
            "SELECT id, branch_id, name FROM collections ORDER BY name, id",
            CollectionId,
        )?,
        edge: edge_names(c)?,
    })
}

/// The Koha codes of the edge with the names of their rows, in code order.
fn edge_names(c: &Connection) -> Result<Vec<NamedCode>, rusqlite::Error> {
    let mut statement = c.prepare(
        "SELECT k.kind, k.code, COALESCE(b.name, s.name, l.name, p.name) FROM koha_codes k
         LEFT JOIN branches b ON k.kind = 1 AND b.id = k.local_id
         LEFT JOIN sublocations s ON k.kind = 2 AND s.id = k.local_id
         LEFT JOIN collections l ON k.kind = 3 AND l.id = k.local_id
         LEFT JOIN circulation_policies p ON k.kind = 4 AND p.id = k.local_id
         ORDER BY k.kind, k.code, k.local_id",
    )?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows
        .into_iter()
        .filter_map(|(kind, code, name)| {
            let table = match kind {
                1 => CodeTable::Branch,
                2 => CodeTable::Location,
                3 => CodeTable::Collection,
                _ => CodeTable::ItemType,
            };
            // A code whose row is gone names nothing.
            name.map(|name| NamedCode { table, code, name })
        })
        .collect())
}

/// Every row of a per-branch list.
fn places_of<Id>(
    c: &Connection,
    sql: &'static str,
    id: fn(i64) -> Id,
) -> Result<Vec<Place<Id>>, rusqlite::Error> {
    let mut statement = c.prepare(sql)?;
    statement
        .query_map([], |row| {
            Ok(Place {
                id: id(row.get(0)?),
                branch: BranchId(row.get(1)?),
                name: row.get(2)?,
            })
        })?
        .collect()
}
