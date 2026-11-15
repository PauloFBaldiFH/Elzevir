// SPDX-License-Identifier: GPL-3.0-or-later
//! The Items of an Instance as a grid saved in one batch
//! (docs/specs/catalogo/exemplares.md): the batch, its refusals by cell, and the
//! validation that runs before the command.

use std::collections::HashMap;

use elzevir_domain::catalog::{
    BranchId, CollectionId, InstanceId, ItemId, Money, PolicyId, SublocationId,
};
use serde::{Deserialize, Serialize};

use crate::catalog::{CatalogReadError, ItemStatus};
use crate::marc_mapping::items::cents;

/// Most rows one batch may carry (item 11).
pub const MAX_ROWS: usize = 500;

/// The columns of one Item that the grid edits (item 1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemFields {
    /// Barcode, unique when present.
    pub barcode: Option<String>,
    /// Inventory number (tombo).
    pub inventory_number: Option<String>,
    /// Home branch.
    pub home_branch: BranchId,
    /// Holding branch.
    pub holding_branch: BranchId,
    /// Circulation policy.
    pub policy: PolicyId,
    /// Call number.
    pub call_number: Option<String>,
    /// Shelving place, in the home branch.
    pub location: Option<SublocationId>,
    /// Collection, of the home branch.
    pub collection: Option<CollectionId>,
    /// Copy number.
    pub copy_number: Option<String>,
    /// Purchase price.
    pub price: Option<Money>,
}

/// One row of the batch: an existing Item (`id`) or a new one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemRow {
    /// The Item, or `None` for a new one.
    pub id: Option<ItemId>,
    /// Its columns.
    pub fields: ItemFields,
}

/// The rows of one Instance's grid, read at `row_version` (items 3 and 4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemsUpdate {
    /// The Instance.
    pub instance: InstanceId,
    /// The record's version when the grid opened.
    pub row_version: i64,
    /// The rows that changed or are new, in the grid's order.
    pub rows: Vec<ItemRow>,
}

/// A column of the grid, as refusals name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemColumn {
    /// The row itself (an unknown Item).
    Row,
    /// Barcode.
    Barcode,
    /// Home branch.
    HomeBranch,
    /// Holding branch.
    HoldingBranch,
    /// Circulation policy.
    Policy,
    /// Shelving place.
    Location,
    /// Collection.
    Collection,
    /// Price.
    Price,
}

/// Why one cell was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RowProblem {
    /// The column cannot be empty.
    Required,
    /// The price is not a number of money.
    Price,
    /// No branch has this id.
    UnknownBranch,
    /// The home branch has no shelving place or collection with this id.
    NotInBranch,
    /// No circulation policy has this id.
    UnknownPolicy,
    /// Another Item, in the catalog or in the batch, has this barcode.
    BarcodeTaken,
    /// No Item of this Instance has this id.
    UnknownItem,
}

impl ItemColumn {
    /// The column's name in the grid and in the JSON the screen sends.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Row => "row",
            Self::Barcode => "barcode",
            Self::HomeBranch => "home_branch",
            Self::HoldingBranch => "holding_branch",
            Self::Policy => "policy",
            Self::Location => "location",
            Self::Collection => "collection",
            Self::Price => "price",
        }
    }
}

impl RowProblem {
    /// Fluent key of the message.
    #[must_use]
    pub const fn message_key(self) -> &'static str {
        match self {
            Self::Required => "items-error-required",
            Self::Price => "items-error-price",
            Self::UnknownBranch => "items-error-branch",
            Self::NotInBranch => "items-error-not-in-branch",
            Self::UnknownPolicy => "items-error-policy",
            Self::BarcodeTaken => "items-error-barcode-taken",
            Self::UnknownItem => "items-error-unknown-item",
        }
    }
}

/// One refused cell: the row's position in the batch, the column and why (item 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RowRefusal {
    /// Zero-based position of the row in the batch.
    pub row: usize,
    /// The column.
    pub column: ItemColumn,
    /// Why.
    pub problem: RowProblem,
}

/// One row as the screen sends it: every column as typed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TypedRow {
    /// The Item, or `None` for a new one.
    #[serde(default)]
    pub id: Option<i64>,
    /// Barcode.
    #[serde(default)]
    pub barcode: String,
    /// Inventory number.
    #[serde(default)]
    pub inventory_number: String,
    /// Home branch id.
    #[serde(default)]
    pub home_branch: String,
    /// Holding branch id; empty takes the home branch.
    #[serde(default)]
    pub holding_branch: String,
    /// Circulation policy id.
    #[serde(default)]
    pub policy: String,
    /// Call number.
    #[serde(default)]
    pub call_number: String,
    /// Shelving place id.
    #[serde(default)]
    pub location: String,
    /// Collection id.
    #[serde(default)]
    pub collection: String,
    /// Copy number.
    #[serde(default)]
    pub copy_number: String,
    /// Price as typed: `12,50`, `12.50` or `12`.
    #[serde(default)]
    pub price: String,
}

/// Turns the typed rows into the batch's rows (item 10). Every refused cell is reported
/// at once; nothing is stored when there is one.
///
/// # Errors
///
/// The cells that cannot be stored as typed.
pub fn item_rows(typed: Vec<TypedRow>) -> Result<Vec<ItemRow>, Vec<RowRefusal>> {
    let mut refusals = Vec::new();
    let mut rows = Vec::with_capacity(typed.len());
    for (position, row) in typed.into_iter().enumerate() {
        let mut refuse = |column, problem| {
            refusals.push(RowRefusal {
                row: position,
                column,
                problem,
            });
        };
        let home_branch = match id_of(&row.home_branch) {
            Ok(Some(id)) => BranchId(id),
            Ok(None) => {
                refuse(ItemColumn::HomeBranch, RowProblem::Required);
                BranchId(0)
            }
            Err(()) => {
                refuse(ItemColumn::HomeBranch, RowProblem::UnknownBranch);
                BranchId(0)
            }
        };
        let policy = match id_of(&row.policy) {
            Ok(Some(id)) => PolicyId(id),
            Ok(None) => {
                refuse(ItemColumn::Policy, RowProblem::Required);
                PolicyId(0)
            }
            Err(()) => {
                refuse(ItemColumn::Policy, RowProblem::UnknownPolicy);
                PolicyId(0)
            }
        };
        let price = optional(&row.price);
        let price = match price.as_deref().map(cents) {
            None => None,
            Some(Some(money)) => Some(money),
            Some(None) => {
                refuse(ItemColumn::Price, RowProblem::Price);
                None
            }
        };
        let holding_branch = id_of(&row.holding_branch).unwrap_or_else(|()| {
            refuse(ItemColumn::HoldingBranch, RowProblem::UnknownBranch);
            None
        });
        let location = id_of(&row.location).unwrap_or_else(|()| {
            refuse(ItemColumn::Location, RowProblem::NotInBranch);
            None
        });
        let collection = id_of(&row.collection).unwrap_or_else(|()| {
            refuse(ItemColumn::Collection, RowProblem::NotInBranch);
            None
        });
        rows.push(ItemRow {
            id: row.id.map(ItemId),
            fields: ItemFields {
                barcode: optional(&row.barcode),
                inventory_number: optional(&row.inventory_number),
                home_branch,
                holding_branch: holding_branch.map_or(home_branch, BranchId),
                policy,
                call_number: optional(&row.call_number),
                location: location.map(SublocationId),
                collection: collection.map(CollectionId),
                copy_number: optional(&row.copy_number),
                price,
            },
        });
    }
    refusals.extend(repeated_barcodes(&rows));
    refusals.sort_by_key(|refusal| refusal.row);
    if refusals.is_empty() {
        Ok(rows)
    } else {
        Err(refusals)
    }
}

/// Every row whose barcode another row of the batch also has (item 10).
#[must_use]
pub fn repeated_barcodes(rows: &[ItemRow]) -> Vec<RowRefusal> {
    let mut seen: HashMap<&str, usize> = HashMap::new();
    for row in rows {
        if let Some(barcode) = &row.fields.barcode {
            *seen.entry(barcode.as_str()).or_default() += 1;
        }
    }
    rows.iter()
        .enumerate()
        .filter(|(_, row)| {
            row.fields
                .barcode
                .as_deref()
                .is_some_and(|barcode| seen.get(barcode).is_some_and(|count| *count > 1))
        })
        .map(|(row, _)| RowRefusal {
            row,
            column: ItemColumn::Barcode,
            problem: RowProblem::BarcodeTaken,
        })
        .collect()
}

/// One stored Item as the grid shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridItem {
    /// The Item.
    pub id: ItemId,
    /// Its editable columns.
    pub fields: ItemFields,
    /// Its circulation status, shown as a signal and not edited (item 1).
    pub status: ItemStatus,
}

/// Port: what the grid reads.
pub trait ItemGridSource: Send + Sync {
    /// The Items of `instance`, in creation order.
    ///
    /// # Errors
    ///
    /// When storage fails.
    fn grid_items(&self, instance: InstanceId) -> Result<Vec<GridItem>, CatalogReadError>;
}

/// A cell that names a registry row by its id: empty is `None`, anything but a positive
/// number is refused.
fn id_of(text: &str) -> Result<Option<i64>, ()> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    match trimmed.parse::<i64>() {
        Ok(id) if id > 0 => Ok(Some(id)),
        _ => Err(()),
    }
}

fn optional(text: &str) -> Option<String> {
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(barcode: &str) -> TypedRow {
        TypedRow {
            barcode: barcode.to_owned(),
            home_branch: " 3 ".to_owned(),
            policy: "1".to_owned(),
            ..TypedRow::default()
        }
    }

    #[test]
    fn rows_are_trimmed_and_the_holding_branch_defaults_to_the_home_one() {
        let rows = item_rows(vec![TypedRow {
            id: Some(7),
            price: "12,5".to_owned(),
            location: "  ".to_owned(),
            ..typed(" 0001 ")
        }])
        .unwrap();
        let [row] = rows.as_slice() else {
            panic!("one row in, one row out")
        };
        assert_eq!(row.id, Some(ItemId(7)));
        assert_eq!(row.fields.barcode.as_deref(), Some("0001"));
        assert_eq!(row.fields.home_branch, BranchId(3));
        assert_eq!(row.fields.holding_branch, BranchId(3));
        assert_eq!(row.fields.location, None);
        assert_eq!(row.fields.price, Some(Money(1250)));
    }

    #[test]
    fn every_bad_cell_is_reported_with_its_row() {
        let refusals = item_rows(vec![
            typed("0001"),
            TypedRow {
                home_branch: String::new(),
                price: "doação".to_owned(),
                location: "estante".to_owned(),
                ..typed("0002")
            },
            typed("0001"),
        ])
        .unwrap_err();
        let at = |row, column, problem| RowRefusal {
            row,
            column,
            problem,
        };
        assert_eq!(
            refusals,
            [
                at(0, ItemColumn::Barcode, RowProblem::BarcodeTaken),
                at(1, ItemColumn::HomeBranch, RowProblem::Required),
                at(1, ItemColumn::Price, RowProblem::Price),
                at(1, ItemColumn::Location, RowProblem::NotInBranch),
                at(2, ItemColumn::Barcode, RowProblem::BarcodeTaken),
            ]
        );
    }
}
