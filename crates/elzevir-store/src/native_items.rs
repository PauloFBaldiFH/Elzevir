// SPDX-License-Identifier: GPL-3.0-or-later
//! Native items are checked and stored inside the import command's savepoint.

use std::collections::BTreeMap;

use elzevir_app::catalog::{
    CatalogRefusal, NativeItem, NativeItemProblem, NewRecord, RecordItem, RecordItems,
};
use elzevir_app::commands::WriteRefusal;
use elzevir_app::items::{ItemColumn, RowProblem};
use elzevir_domain::catalog::{InstanceId, ItemId};
use elzevir_domain::statement::{EntityKind, EntityRef};
use rusqlite::Connection;

use crate::catalog::{add_item, lookup, refused};
use crate::{CommandFailed, items, statements};

type Rejection = CommandFailed<WriteRefusal>;

pub(crate) fn check_record(record: &NewRecord) -> Result<(), Rejection> {
    let RecordItems::Native { native } = &record.items else {
        return Ok(());
    };
    if record.source.is_some() {
        return Err(refused(CatalogRefusal::NativeItemSource));
    }
    let mut seen = BTreeMap::new();
    for (index, item) in native.iter().enumerate() {
        if let Some(barcode) = item.fields.barcode.as_deref()
            && let Some(first) = seen.insert(barcode, index + 1)
        {
            return Err(context(item, index, NativeItemProblem::Duplicate { first }));
        }
    }
    Ok(())
}

pub(crate) fn add(
    connection: &Connection,
    instance: InstanceId,
    position: usize,
    item: RecordItem<'_>,
) -> Result<ItemId, Rejection> {
    match item {
        RecordItem::Marc(item) => add_item(connection, instance, item),
        RecordItem::Native(item) => write(connection, instance, None, position, item),
    }
}

pub(crate) fn write(
    connection: &Connection,
    instance: InstanceId,
    existing: Option<ItemId>,
    position: usize,
    item: &NativeItem,
) -> Result<ItemId, Rejection> {
    let mut problem = None;
    items::check_places(connection, &item.fields, |column, failure| {
        if problem.is_none() {
            problem = Some(NativeItemProblem::Field {
                column,
                problem: failure,
            });
        }
    })?;
    if let Some(problem) = problem {
        return Err(context(item, position, problem));
    }
    if let Some(barcode) = item.fields.barcode.as_deref()
        && let Some(holder) = lookup(
            connection,
            "SELECT id FROM items WHERE barcode = ?1",
            barcode,
        )?
        && Some(ItemId(holder)) != existing
    {
        return Err(context(
            item,
            position,
            NativeItemProblem::Field {
                column: ItemColumn::Barcode,
                problem: RowProblem::BarcodeTaken,
            },
        ));
    }

    let persist = || {
        let id = if let Some(id) = existing {
            items::update_item(connection, id, &item.fields)?;
            id
        } else {
            items::add_grid_item(connection, instance, &item.fields)?
        };
        let subject = EntityRef {
            entity: EntityKind::Item,
            id: id.0,
        };
        statements::store_description(connection, subject, &item.description)?;
        Ok(id)
    };
    persist().map_err(|failure| match failure {
        CommandFailed::Rejected(WriteRefusal::Catalog(reason)) => {
            context(item, position, NativeItemProblem::Catalog(Box::new(reason)))
        }
        other => other,
    })
}

fn context(item: &NativeItem, index: usize, problem: NativeItemProblem) -> Rejection {
    refused(CatalogRefusal::NativeItem {
        position: index + 1,
        barcode: item.fields.barcode.clone(),
        problem,
    })
}
