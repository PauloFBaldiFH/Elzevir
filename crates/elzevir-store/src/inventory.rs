// SPDX-License-Identifier: GPL-3.0-or-later
//! `catalog.inventory` (docs/specs/catalogo/inventario.md): the Items read off the shelf
//! are seen, and the report says what is wrong.

use std::collections::{BTreeMap, BTreeSet};

use elzevir_app::catalog::{CatalogOutput, CatalogRefusal};
use elzevir_app::commands::WriteRefusal;
use elzevir_app::inventory::{
    BranchSide, InventoryLine, InventoryReport, InventoryScan, MAX_INVENTORY_BARCODES,
};
use elzevir_domain::catalog::{InstanceId, ItemId};
use elzevir_domain::inventory::{ReadItem, in_call_number_range, read_problems, shelf_key, unread};
use rusqlite::params;

use crate::catalog::{record_changed, refused};
use crate::{CommandFailed, StoreError, WriteTx};

/// Every Item with what the inventory looks at, in shelf order once sorted.
const ITEMS: &str = "SELECT i.id, i.instance_id, i.barcode, i.home_branch, i.holding_branch,
            i.sublocation_id, i.collection_id, i.call_number, i.lost, i.withdrawn, i.last_seen,
            EXISTS (SELECT 1 FROM loans WHERE item_id = i.id),
            EXISTS (SELECT 1 FROM holds WHERE item_id = i.id AND found = 'W'),
            n.title
     FROM items i JOIN instances n ON n.id = i.instance_id";

/// An Item as the inventory sees it.
#[expect(
    clippy::struct_excessive_bools,
    reason = "a row of the items query: each flag is a column or a fact read with it"
)]
struct Shelved {
    item: ItemId,
    instance: InstanceId,
    barcode: Option<String>,
    branches: (i64, i64),
    sublocation: Option<i64>,
    collection: Option<i64>,
    call_number: Option<String>,
    lost: bool,
    withdrawn: bool,
    last_seen: Option<String>,
    on_loan: bool,
    waiting: bool,
    title: String,
}

impl Shelved {
    /// Item 3: in the part of the collection being counted.
    fn in_area(&self, scan: &InventoryScan) -> bool {
        let area = &scan.area;
        let branch = match area.side {
            BranchSide::Home => self.branches.0,
            BranchSide::Holding => self.branches.1,
        };
        area.branch.is_none_or(|wanted| wanted.0 == branch)
            && area
                .sublocation
                .is_none_or(|wanted| Some(wanted.0) == self.sublocation)
            && area
                .collection
                .is_none_or(|wanted| Some(wanted.0) == self.collection)
            && in_call_number_range(
                &shelf_key(self.call_number.as_deref().unwrap_or_default()),
                area.call_number_from.as_deref(),
                area.call_number_to.as_deref(),
            )
    }

    /// Item 4: in the area and expected on the shelf.
    fn expected(&self, scan: &InventoryScan) -> bool {
        let filters = &scan.filters;
        self.in_area(scan)
            && !(filters.skip_on_loan && self.on_loan)
            && !(filters.skip_waiting_holds && self.waiting)
            && filters.not_seen_since.is_none_or(|day| {
                // A stored day compares as text with another day.
                self.last_seen
                    .as_deref()
                    .is_none_or(|seen| seen < day.to_string().as_str())
            })
    }

    fn line(&self, problems: Vec<elzevir_domain::inventory::InventoryProblem>) -> InventoryLine {
        InventoryLine {
            item: Some(self.item),
            barcode: self.barcode.clone(),
            title: Some(self.title.clone()),
            call_number: self.call_number.clone(),
            problems,
        }
    }
}

fn shelved(tx: &WriteTx<'_>) -> Result<Vec<Shelved>, StoreError> {
    let mut statement = tx.connection().prepare_cached(ITEMS)?;
    let rows = statement.query_map([], |row| {
        Ok(Shelved {
            item: ItemId(row.get(0)?),
            instance: InstanceId(row.get(1)?),
            barcode: row.get(2)?,
            branches: (row.get(3)?, row.get(4)?),
            sublocation: row.get(5)?,
            collection: row.get(6)?,
            call_number: row.get(7)?,
            lost: row.get::<_, i64>(8)? != 0,
            withdrawn: row.get::<_, i64>(9)? != 0,
            last_seen: row.get(10)?,
            on_loan: row.get(11)?,
            waiting: row.get(12)?,
            title: row.get(13)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Items 5 to 8.
pub(crate) fn scan(
    tx: &WriteTx<'_>,
    scan: &InventoryScan,
) -> Result<CatalogOutput, CommandFailed<WriteRefusal>> {
    if scan.barcodes.is_empty() && !scan.compare {
        return Err(refused(CatalogRefusal::NoBarcodes));
    }
    if scan.barcodes.len() > MAX_INVENTORY_BARCODES {
        return Err(refused(CatalogRefusal::TooManyItems {
            count: scan.barcodes.len(),
            max: MAX_INVENTORY_BARCODES,
        }));
    }
    let mut items = shelved(tx)?;
    items.sort_by_cached_key(|item| shelf_key(item.call_number.as_deref().unwrap_or_default()));
    // Koha finds a barcode whatever its case.
    let by_barcode: BTreeMap<String, usize> = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| Some((item.barcode.as_deref()?.to_lowercase(), index)))
        .collect();
    let mut report = InventoryReport {
        read: scan.barcodes.len(),
        ..InventoryReport::default()
    };
    let mut read = Vec::new();
    let mut found = Vec::new();
    let mut lines: Vec<Option<InventoryLine>> = Vec::new();
    for barcode in &scan.barcodes {
        match by_barcode
            .get(&barcode.to_lowercase())
            .and_then(|&index| items.get(index))
        {
            Some(item) => {
                read.push(ReadItem {
                    item: item.item,
                    sort_key: shelf_key(item.call_number.as_deref().unwrap_or_default()),
                    in_place: item.in_area(scan),
                    on_loan: item.on_loan,
                    was_lost: item.lost,
                    withdrawn: item.withdrawn,
                });
                found.push(item);
                lines.push(None);
            }
            // Item 6: a barcode no Item has is a line of its own, where it was read.
            None => lines.push(Some(InventoryLine {
                item: None,
                barcode: Some(barcode.clone()),
                title: None,
                call_number: None,
                problems: vec![elzevir_domain::inventory::InventoryProblem::UnknownBarcode],
            })),
        }
    }
    let seen_on = scan.seen_on.to_string();
    let mut changed = BTreeSet::new();
    for item in found.iter().filter(|item| !item.withdrawn) {
        // Item 5: seen that day, and found if it was lost.
        tx.connection()
            .execute(
                "UPDATE items SET last_seen = ?2, lost = 0, lost_on = NULL,
                        row_version = row_version + CASE WHEN lost <> 0 THEN 1 ELSE 0 END
                 WHERE id = ?1",
                params![item.item.0, seen_on],
            )
            .map_err(StoreError::from)?;
        report.marked += 1;
        if item.lost {
            changed.insert(item.instance);
            let place = crate::lost_found::FoundAt::ELSEWHERE;
            crate::lost_found::found_by_rules(tx.connection(), item.item.0, place, &scan.at)
                .map_err(StoreError::from)?;
        }
    }
    for instance in changed {
        record_changed(tx, instance)?;
    }
    let mut problems = read_problems(&read, scan.out_of_order).into_iter();
    let mut found = found.into_iter();
    for slot in &mut lines {
        if slot.is_none()
            && let (Some(item), Some(problems)) = (found.next(), problems.next())
            && !problems.is_empty()
        {
            *slot = Some(item.line(problems));
        }
    }
    report.lines = lines.into_iter().flatten().collect();
    let expected: Vec<&Shelved> = items.iter().filter(|item| item.expected(scan)).collect();
    report.expected = expected.len();
    if scan.compare {
        let read: BTreeSet<ItemId> = read.iter().map(|item| item.item).collect();
        let index: BTreeMap<ItemId, &Shelved> =
            expected.iter().map(|item| (item.item, *item)).collect();
        let pairs: Vec<(ItemId, bool)> = expected
            .iter()
            .map(|item| (item.item, item.barcode.is_some()))
            .collect();
        for (item, problem) in unread(&pairs, &read) {
            if let Some(shelved) = index.get(&item) {
                report.lines.push(shelved.line(vec![problem]));
            }
        }
    }
    Ok(CatalogOutput::Inventory(Box::new(report)))
}
