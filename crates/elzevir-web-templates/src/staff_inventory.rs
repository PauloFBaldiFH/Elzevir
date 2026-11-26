// SPDX-License-Identifier: GPL-3.0-or-later
//! The inventory screen: the form of a reading of the shelf and its report
//! (docs/specs/catalogo/inventario.md, items 10 to 13).

use askama::Template;

use crate::I18n;

/// The inventory page.
#[derive(Template)]
#[template(path = "staff_inventory.html")]
pub struct StaffInventory<'a> {
    /// Interface texts.
    pub t: &'a I18n,
    /// Anti-forgery token of the session.
    pub csrf: &'a str,
    /// The form, as typed or blank.
    pub form: InventoryForm,
    /// Message key of why the reading was refused.
    pub refused: Option<&'static str>,
    /// The report of the reading just made.
    pub report: Option<InventoryView>,
}

/// One option of a list of the form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryChoice {
    /// The value sent.
    pub value: String,
    /// The name shown.
    pub name: String,
    /// Whether it is the one chosen.
    pub selected: bool,
}

/// The fields of the form: each box is a checkbox of its own.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "five independent checkboxes of the form, shown as typed"
)]
pub struct InventoryForm {
    /// Branches.
    pub branches: Vec<InventoryChoice>,
    /// Whether the branch is the holding one rather than the home one.
    pub holding: bool,
    /// Shelving places, named with their branch.
    pub sublocations: Vec<InventoryChoice>,
    /// Collections, named with their branch.
    pub collections: Vec<InventoryChoice>,
    /// The first call number of the range.
    pub call_number_from: String,
    /// The last call number of the range.
    pub call_number_to: String,
    /// The day of the reading, `YYYY-MM-DD`.
    pub seen_on: String,
    /// Only Items not seen since this day, `YYYY-MM-DD`, or empty.
    pub not_seen_since: String,
    /// Leave out the Items on loan.
    pub skip_on_loan: bool,
    /// Leave out the Items waiting for a hold.
    pub skip_waiting_holds: bool,
    /// List the expected Items not read.
    pub compare: bool,
    /// Check the shelf order.
    pub out_of_order: bool,
    /// The barcodes, as typed.
    pub barcodes: String,
}

/// The report of a reading.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InventoryView {
    /// What the reading did, in words.
    pub summary: String,
    /// What reading the text left out, in words; empty when nothing.
    pub left_out: String,
    /// The lines with a problem.
    pub lines: Vec<InventoryRow>,
}

/// One line of the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryRow {
    /// The barcode, or empty.
    pub barcode: String,
    /// The title, or empty.
    pub title: String,
    /// The call number, or empty.
    pub call_number: String,
    /// The problems, in words, separated by commas.
    pub problems: String,
}
