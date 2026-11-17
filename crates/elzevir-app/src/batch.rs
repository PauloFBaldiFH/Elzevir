// SPDX-License-Identifier: GPL-3.0-or-later
//! Families selected in the listing, for the batch actions (docs/specs/catalogo/lote.md).

use elzevir_domain::catalog::InstanceId;

use crate::catalog::CatalogReadError;
use crate::listing::{CatalogListing, FamilyHead};

/// Most families one selection holds (lote.md, item 4).
pub const MAX_SELECTION: usize = 500;

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
