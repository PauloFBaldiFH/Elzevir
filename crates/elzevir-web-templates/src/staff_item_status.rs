// SPDX-License-Identifier: GPL-3.0-or-later
//! The status of an Item at the desk and the names of the status numbers
//! (docs/specs/circulacao/lost-items.md, items 2 and 3).

use askama::Template;

use crate::{I18n, Notice};

/// The page of an Item's statuses.
#[derive(Template)]
#[template(path = "staff_item_status.html")]
pub struct StaffItemStatus<'a> {
    /// Interface texts.
    pub t: &'a I18n,
    /// Anti-forgery token of the session.
    pub csrf: &'a str,
    /// The barcode asked for, as typed.
    pub barcode: &'a str,
    /// What the last change did, or why the barcode was not found.
    pub notices: Vec<Notice>,
    /// The Item, when the barcode found one.
    pub item: Option<ItemStatusView>,
}

/// The Item as the page shows it, without its patron.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemStatusView {
    /// Its barcode.
    pub barcode: String,
    /// The title.
    pub title: String,
    /// The call number, or empty.
    pub call_number: String,
    /// Where it is.
    pub holding: String,
    /// "On loan until D", or empty.
    pub loan: String,
    /// Lost, damaged and withdrawn.
    pub statuses: Vec<StatusForm>,
}

/// One status and the selection that changes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusForm {
    /// The kind's stored code, sent back with the form.
    pub kind: &'static str,
    /// The kind's name.
    pub name: String,
    /// The current number.
    pub current: i64,
    /// The current status as read: its name, and since when.
    pub shown: String,
    /// The numbers to choose from, with their names; 0 first.
    pub options: Vec<(i64, String)>,
}

/// The names of the status numbers.
#[derive(Template)]
#[template(path = "staff_status_names.html")]
pub struct StaffStatusNames<'a> {
    /// Interface texts.
    pub t: &'a I18n,
    /// Anti-forgery token of the session.
    pub csrf: &'a str,
    /// What the last change did.
    pub notices: Vec<Notice>,
    /// Each kind, with its names.
    pub kinds: Vec<StatusKindNames>,
}

/// The names of one kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusKindNames {
    /// The kind's stored code.
    pub kind: &'static str,
    /// The kind's generic name.
    pub name: String,
    /// Each named number.
    pub names: Vec<(i64, String)>,
}
