// SPDX-License-Identifier: GPL-3.0-or-later
//! Inventory by reading barcodes off the shelf (docs/specs/catalogo/inventario.md).

use elzevir_domain::catalog::{BranchId, CollectionId, ItemId, SublocationId};
use elzevir_domain::inventory::InventoryProblem;
use jiff::civil::Date;
use serde::{Deserialize, Serialize};

use crate::loans::checked_barcode;

/// Most barcodes one inventory reads (item 2).
pub const MAX_INVENTORY_BARCODES: usize = 20_000;

/// Which branch of an Item places it (Koha: `branch`, `homebranch` or `holdingbranch`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BranchSide {
    /// The branch that owns it.
    #[default]
    Home,
    /// The branch where it is now.
    Holding,
}

/// The part of the collection being counted (item 3); `None` leaves a criterion out.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryArea {
    /// The branch.
    pub branch: Option<BranchId>,
    /// Whether `branch` is the home or the holding branch.
    pub side: BranchSide,
    /// The shelving place.
    pub sublocation: Option<SublocationId>,
    /// The collection.
    pub collection: Option<CollectionId>,
    /// The first call number of the range.
    pub call_number_from: Option<String>,
    /// The last call number of the range; with only the first, every call number that
    /// starts like it (Koha: `maxlocation` is `minlocation` and `Z`).
    pub call_number_to: Option<String>,
}

/// Which Items of the area are expected on the shelf (item 4).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryFilters {
    /// Only Items not seen since this day, or never (Koha: `datelastseen`).
    pub not_seen_since: Option<Date>,
    /// Leave out the Items on loan (Koha: `ignoreissued`).
    pub skip_on_loan: bool,
    /// Leave out the Items waiting for a hold (Koha: `ignore_waiting_holds`).
    pub skip_waiting_holds: bool,
}

/// One reading of the shelf (item 5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryScan {
    /// The barcodes read, each once, in reading order ([`read_barcodes`]).
    pub barcodes: Vec<String>,
    /// The day they were seen.
    pub seen_on: Date,
    /// The part of the collection being counted.
    pub area: InventoryArea,
    /// Which of its Items are expected.
    pub filters: InventoryFilters,
    /// Also list the expected Items that were not read (Koha: `compareinv2barcd`).
    pub compare: bool,
    /// Also check the shelf order of the Items read (Koha: `out_of_order`).
    pub out_of_order: bool,
}

/// One line of the report: an Item, or a barcode no Item has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryLine {
    /// The Item, when one has the barcode.
    pub item: Option<ItemId>,
    /// The barcode.
    pub barcode: Option<String>,
    /// The title of its Instance.
    pub title: Option<String>,
    /// Its call number.
    pub call_number: Option<String>,
    /// What is wrong.
    pub problems: Vec<InventoryProblem>,
}

/// What a reading did and found (item 6).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InventoryReport {
    /// Barcodes read.
    pub read: usize,
    /// Items marked as seen.
    pub marked: usize,
    /// The Items expected on the shelf.
    pub expected: usize,
    /// The lines with a problem: the Items read, in reading order, then those not read.
    pub lines: Vec<InventoryLine>,
}

/// What reading the text of barcodes left out (item 2).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BarcodesRead {
    /// Non-empty lines.
    pub lines: usize,
    /// Barcodes longer than an Item's barcode can be.
    pub too_long: usize,
    /// Barcodes with a character that does not print.
    pub unprintable: usize,
    /// Barcodes read again.
    pub repeated: usize,
}

/// The barcodes in `text`, one per line or separated by spaces, each once and in reading
/// order, and what was left out.
///
/// Koha: `tools/inventory.pl` (`BarcodeSeparators`, the barcode's column size and the
/// printable characters); Koha reads a barcode twice when it comes twice.
#[must_use]
pub fn read_barcodes(text: &str) -> (Vec<String>, BarcodesRead) {
    let mut found = BarcodesRead::default();
    let mut barcodes: Vec<String> = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for barcode in text.split(|c: char| c.is_whitespace() || c == ',' || c == ';') {
        if barcode.is_empty() {
            continue;
        }
        found.lines += 1;
        if barcode.chars().any(char::is_control) {
            found.unprintable += 1;
        } else if checked_barcode(barcode).is_none() {
            found.too_long += 1;
        } else if !seen.insert(barcode.to_lowercase()) {
            found.repeated += 1;
        } else {
            barcodes.push(barcode.to_owned());
        }
    }
    (barcodes, found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn barcodes_are_split_checked_and_read_once() {
        let long = "9".repeat(crate::loans::MAX_BARCODE_LENGTH + 1);
        let text = format!("0001\n0002 0003,0004;\r\n\n0001\nabc\u{7}\n{long}\n0002\n");
        let (barcodes, found) = read_barcodes(&text);
        assert_eq!(barcodes, ["0001", "0002", "0003", "0004"]);
        assert_eq!(
            found,
            BarcodesRead {
                lines: 8,
                too_long: 1,
                unprintable: 1,
                repeated: 2,
            }
        );
    }
}
