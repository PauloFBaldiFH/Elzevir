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
    /// The edit form, for who manages patrons.
    pub edit_url: Option<String>,
    /// The patron's id, for the circulation form.
    pub id: i64,
    /// Anti-forgery token of the session.
    pub csrf: &'a str,
    /// The text of the suspension, while it holds.
    pub suspended: Option<String>,
    /// What the patron owes, when they owe something.
    pub owed: Option<String>,
    /// The patron's loans, the first due first.
    pub loans: Vec<LoanLine>,
    /// The branches the desk may be at, and the one it is at.
    pub desks: Vec<DeskOption>,
    /// The barcode to keep in the box, when a checkout asks for confirmation.
    pub barcode: String,
    /// What the last circulation action did.
    pub notices: Vec<Notice>,
    /// Whether the last checkout waits for the staff's confirmation.
    pub confirm: bool,
}

/// One loan on the patron's side of the wall.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoanLine {
    /// The Item's barcode.
    pub barcode: String,
    /// The Instance's title.
    pub title: String,
    /// The due date as shown.
    pub due: String,
    /// Whether it is overdue now.
    pub overdue: bool,
    /// The renewals, as shown, when there were some.
    pub renewals: Option<String>,
    /// Whether it was just lent, so its cord is drawn.
    pub new: bool,
}

/// A branch the desk may be at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeskOption {
    /// Branch id.
    pub id: i64,
    /// Its name.
    pub name: String,
    /// Whether the desk is there.
    pub selected: bool,
}

/// A message about a circulation action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    /// Whether the action was refused or needs attention.
    pub refused: bool,
    /// The text, already formatted.
    pub text: String,
}

/// The checkin desk: return by scanning (interface §18.3).
#[derive(Template)]
#[template(path = "staff_checkin.html")]
pub struct StaffCheckin<'a> {
    /// Interface texts.
    pub t: &'a I18n,
    /// Anti-forgery token of the session.
    pub csrf: &'a str,
    /// The branches the desk may be at, and the one it is at.
    pub desks: Vec<DeskOption>,
    /// What the last checkin did.
    pub notices: Vec<Notice>,
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

/// The form that enrols or edits a patron.
#[derive(Template)]
#[template(path = "staff_patron_form.html")]
pub struct StaffPatronForm<'a> {
    /// Interface texts.
    pub t: &'a I18n,
    /// Anti-forgery token of the session.
    pub csrf: &'a str,
    /// Where the form posts.
    pub action: String,
    /// Fluent key of the title.
    pub title: &'static str,
    /// Version of the patron read, 0 for a new one.
    pub row_version: i64,
    /// The values in the fields.
    pub values: PatronFormValues,
    /// Branches to choose from: id and name.
    pub branches: Vec<(i64, String)>,
    /// Categories to choose from: id and name.
    pub categories: Vec<(i64, String)>,
    /// Whether the last change was stored.
    pub saved: bool,
    /// Message key of why the last change was refused.
    pub refused: Option<&'static str>,
}

/// The texts of the patron form, as typed or as stored.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PatronFormValues {
    /// Library card number.
    pub cardnumber: String,
    /// Surname.
    pub surname: String,
    /// First name.
    pub first_name: String,
    /// CPF, whole: only this form shows it.
    pub cpf: String,
    /// Date of birth, `YYYY-MM-DD`.
    pub date_of_birth: String,
    /// E-mail address.
    pub email: String,
    /// Phone number.
    pub phone: String,
    /// Postal address.
    pub address: String,
    /// Chosen branch id.
    pub branch: i64,
    /// Chosen category id.
    pub category: i64,
    /// Expiry date, `YYYY-MM-DD`; empty takes it from the category.
    pub expires: String,
    /// Internal note.
    pub staff_note: String,
    /// Whether the reading history is kept.
    pub history_kept: bool,
}
