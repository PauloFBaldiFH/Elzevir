// SPDX-License-Identifier: GPL-3.0-or-later
//! Patron pages: the search, the record behind the wall and the categories
//! (docs/specs/circulacao/leitores.md, items 10 to 14).

use askama::Template;

use crate::I18n;

/// The patron search.
#[derive(Template)]
#[template(path = "staff_patrons.html")]
pub struct StaffPatrons<'a> {
    /// Interface texts.
    pub t: &'a I18n,
    /// What was typed.
    pub query: &'a str,
    /// Whether a search ran, so "nothing found" can show.
    pub searched: bool,
    /// The patrons found.
    pub rows: Vec<PatronLine>,
    /// Whether the person may manage categories.
    pub can_manage: bool,
}

/// One patron found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatronLine {
    /// Patron id, for the link.
    pub id: i64,
    /// Library card number.
    pub cardnumber: String,
    /// "Surname, First name".
    pub name: String,
    /// Category name.
    pub category: String,
    /// Expiry date as shown.
    pub expires: String,
    /// Whether the enrolment is over.
    pub expired: bool,
}

/// The patron's record, behind the wall.
#[derive(Template)]
#[template(path = "staff_patron.html")]
pub struct StaffPatron<'a> {
    /// Interface texts.
    pub t: &'a I18n,
    /// "Surname, First name".
    pub name: String,
    /// Rows of the record: Fluent key of the label and the value as shown.
    pub rows: Vec<(&'static str, String)>,
    /// Whether the enrolment is over.
    pub expired: bool,
}

/// The patron categories.
#[derive(Template)]
#[template(path = "staff_patron_categories.html")]
pub struct StaffPatronCategories<'a> {
    /// Interface texts.
    pub t: &'a I18n,
    /// Anti-forgery token of the session.
    pub csrf: &'a str,
    /// Every category, by name.
    pub categories: Vec<CategoryLine>,
    /// Whether the last change was stored.
    pub saved: bool,
    /// Message key of why the last change was refused.
    pub refused: Option<&'static str>,
}

/// One category as its form shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryLine {
    /// Category id.
    pub id: i64,
    /// Its name.
    pub name: String,
    /// Months of enrolment, empty for a fixed date.
    pub months: String,
    /// Fixed expiry date (`YYYY-MM-DD`), empty for months.
    pub until: String,
    /// How many patrons it has, as a sentence.
    pub patrons: String,
}
