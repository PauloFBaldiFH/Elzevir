// SPDX-License-Identifier: GPL-3.0-or-later
//! Families selected in the listing, for the batch actions (docs/specs/catalogo/lote.md).

use elzevir_domain::catalog::{
    BranchId, CollectionId, InstanceId, ItemId, PolicyId, SublocationId,
};
use serde::{Deserialize, Serialize};

use crate::catalog::CatalogReadError;
use crate::items::{ItemColumn, ItemFields, RowProblem};
use crate::listing::{CatalogListing, FamilyHead};

/// Most families one selection holds (lote.md, item 4).
pub const MAX_SELECTION: usize = 500;

/// Most Items one batch change reaches (lote.md, item 12).
pub const MAX_BATCH_ITEMS: usize = 5_000;

/// A change of every Item of some Instances, all or nothing (lote.md, items 10 to 13).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemsChange {
    /// The Instances whose Items change.
    pub instances: Vec<InstanceId>,
    /// What changes, in order; a later change of the same column wins.
    pub set: Vec<ItemSet>,
}

/// One column set on every Item of a batch change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemSet {
    /// The home branch.
    HomeBranch(BranchId),
    /// The holding branch.
    HoldingBranch(BranchId),
    /// The circulation policy.
    Policy(PolicyId),
    /// The shelving place, or none.
    Location(Option<SublocationId>),
    /// The collection, or none.
    Collection(Option<CollectionId>),
}

impl ItemSet {
    /// `fields` with this change made.
    pub fn apply(self, fields: &mut ItemFields) {
        match self {
            Self::HomeBranch(branch) => fields.home_branch = branch,
            Self::HoldingBranch(branch) => fields.holding_branch = branch,
            Self::Policy(policy) => fields.policy = policy,
            Self::Location(location) => fields.location = location,
            Self::Collection(collection) => fields.collection = collection,
        }
    }
}

/// One Item a batch change cannot make, and why (lote.md, item 11).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemRefusal {
    /// The Item.
    pub item: ItemId,
    /// How a person finds it: its barcode, else its inventory number, else its number.
    pub label: String,
    /// The column.
    pub column: ItemColumn,
    /// Why.
    pub problem: RowProblem,
}

/// The families named in `names`, in order, each once, up to [`MAX_SELECTION`];
/// malformed names are left out (lote.md, item 6).
pub fn selection<'a>(names: impl IntoIterator<Item = &'a str>) -> Vec<FamilyHead> {
    let mut heads: Vec<FamilyHead> = Vec::new();
    for head in names.into_iter().filter_map(FamilyHead::from_name) {
        if heads.len() == MAX_SELECTION {
            break;
        }
        if !heads.contains(&head) {
            heads.push(head);
        }
    }
    heads
}

/// Every Instance of `heads`, family by family in that order, and within a family as the
/// listing shows them (lote.md, item 7). Families that are gone are left out.
///
/// # Errors
///
/// When the catalog fails.
pub fn instances_of<L: CatalogListing + ?Sized>(
    listing: &L,
    heads: &[FamilyHead],
) -> Result<Vec<InstanceId>, CatalogReadError> {
    Ok(listing
        .families(heads)?
        .iter()
        .flat_map(|family| &family.works)
        .flat_map(|work| &work.instances)
        .map(|instance| instance.id)
        .collect())
}

#[cfg(test)]
mod tests {
    use elzevir_domain::catalog::{HubId, WorkId};

    use super::*;

    #[test]
    fn a_selection_is_read_once_each_in_order_and_capped() {
        let heads = selection(["w7", "h2", "w7", "x1", "w", "h-3", "w+4", "w8x"]);
        assert_eq!(
            heads,
            [FamilyHead::Work(WorkId(7)), FamilyHead::Hub(HubId(2))]
        );
        let many: Vec<String> = (0..600).map(|n| format!("w{n}")).collect();
        let heads = selection(many.iter().map(String::as_str));
        assert_eq!(heads.len(), MAX_SELECTION);
        assert_eq!(heads.last(), Some(&FamilyHead::Work(WorkId(499))));
        assert_eq!(FamilyHead::Hub(HubId(2)).name(), "h2");
    }
}
