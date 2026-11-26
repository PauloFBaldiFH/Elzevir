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
    /// The zoom the list opens in, `compact` or `full` (interface §24.6).
    pub zoom: &'static str,
    /// Whether the person may manage categories.
    pub can_manage: bool,
    /// Whether a patron was just anonymized.
    pub anonymized: bool,
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
    /// Up to two initials.
    pub initials: String,
    /// Name of the home branch.
    pub branch: String,
    /// How the patron stands, as on the record.
    pub status: Vec<StatusChip>,
    /// Open loans.
    pub loans: usize,
    /// "N loans", for the full view.
    pub loans_text: String,
    /// When the first open loan is due, as shown.
    pub next_due: Option<String>,
    /// Open holds.
    pub holds: u32,
    /// "N holds", for the full view.
    pub holds_text: String,
    /// "N waiting for pickup", when some are set aside.
    pub waiting: Option<String>,
    /// What the patron owes, as shown.
    pub owed: String,
    /// Day of the enrolment, as shown.
    pub enrolled: String,
    /// Message key of the reading history choice.
    pub history_key: &'static str,
}

/// The patron's record, behind the wall.
#[derive(Template)]
#[template(path = "staff_patron.html")]
pub struct StaffPatron<'a> {
    /// Interface texts.
    pub t: &'a I18n,
    /// "Surname, First name".
    pub name: String,
    /// What the card shows of the patron (interface §24.2).
    pub header: PatronHeader,
    /// `full` or `compact`: the zoom the page opens in (interface §24.1).
    pub zoom: &'static str,
    /// Whether the enrolment is over.
    pub expired: bool,
    /// The edit form, for who manages patrons.
    pub edit_url: Option<String>,
    /// The patron's id, for the circulation form.
    pub id: i64,
    /// Anti-forgery token of the session.
    pub csrf: &'a str,
    /// The patron's restrictions that hold today, each with its lift button.
    pub restrictions: Vec<RestrictionRow>,
    /// What the patron owes, when they owe something.
    pub owed: Option<String>,
    /// The lines of the account, newest first.
    pub account: Vec<AccountRow>,
    /// The patron's holds, oldest first.
    pub holds: Vec<HoldRow>,
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

/// The top of the patron's card: who they are and how they stand (interface §24.2).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PatronHeader {
    /// Up to two initials, for the round badge.
    pub initials: String,
    /// The name, first name first.
    pub display_name: String,
    /// Library card number.
    pub cardnumber: String,
    /// Name of the category.
    pub category: String,
    /// Name of the home branch.
    pub branch: String,
    /// Last day of the enrolment, as shown.
    pub expires: Option<String>,
    /// How the patron stands, each with its tone and icon.
    pub status: Vec<StatusChip>,
    /// Contact rows: Fluent key of the label and the value as shown.
    pub contact: Vec<(&'static str, String)>,
    /// Library use rows.
    pub usage: Vec<(&'static str, String)>,
    /// Identification rows; the CPF masked unless the staff asked for it.
    pub identity: Vec<(&'static str, String)>,
    /// The CPF is masked, so "Show CPF" is offered (interface §18.4).
    pub cpf_hidden: bool,
}

/// One mark of how the patron stands, never colour alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusChip {
    /// `late`, `soon` or `ok`.
    pub tone: &'static str,
    /// Icon in the sprite.
    pub icon: &'static str,
    /// What it says.
    pub text: String,
}

/// One loan on the patron's side of the wall.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoanLine {
    /// The Item's barcode.
    pub barcode: String,
    /// The Instance's title.
    pub title: String,
    /// The Item, for the crossing to the catalogue (interface §24.4).
    pub item: i64,
    /// The catalogue's page of the Item: its barcode in the search, nothing of the patron.
    pub catalog_url: String,
    /// The due date as shown, with its urgency (interface §24.3).
    pub due: String,
    /// `late`, `soon` or `ok`: the colour of the card and its cord.
    pub urgency: &'static str,
    /// The Item's call number and the branch holding it, as shown.
    pub place: String,
    /// The renewals done, as shown, when there were some.
    pub renewals: Option<String>,
    /// How many renewals are left, as shown.
    pub renewals_left: String,
    /// Whether it was just lent, so its cord is drawn.
    pub new: bool,
}

/// A line of the patron's account as shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountRow {
    /// The line's id, for the payment form.
    pub id: i64,
    /// The day it was made.
    pub day: String,
    /// What it is, with the Item's barcode.
    pub description: String,
    /// The amount, negative for a credit.
    pub amount: String,
    /// What is left of it.
    pub outstanding: String,
    /// Whether it still owes, so it can be chosen for a payment.
    pub payable: bool,
}

/// One of the patron's restrictions as shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestrictionRow {
    /// The kind as posted by the lift button.
    pub kind: &'static str,
    /// What it says.
    pub text: String,
}

/// One of the patron's holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoldRow {
    /// The hold's id, for cancelling it.
    pub id: i64,
    /// The Instance's title.
    pub title: String,
    /// The Item's barcode, for an Item hold.
    pub barcode: Option<String>,
    /// The pickup branch's name.
    pub pickup: String,
    /// The day it was placed, as shown.
    pub day: String,
    /// Its place in the queue, or that it waits or is in transit.
    pub status: String,
}

/// The button that sets a checked-in Item aside for the hold it answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsideForm {
    /// The hold.
    pub hold: i64,
    /// The Item's barcode.
    pub barcode: String,
    /// The desk's branch.
    pub desk: i64,
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
    /// Setting the Item aside, when it answers a hold.
    pub aside: Option<AsideForm>,
    /// Barcode to prefill, when the staff header sent one (interface §22 item 2).
    pub barcode: &'a str,
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
    /// The OPAC password part, when editing a patron.
    pub password: Option<PasswordSection>,
    /// The anonymization part, on the edit form.
    pub anonymize: Option<AnonymizeSection>,
}

/// The OPAC password part of the patron form (docs/specs/opac/conta-do-leitor.md, item 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasswordSection {
    /// Where its form posts.
    pub action: String,
    /// Whether the patron has a password.
    pub has_password: bool,
    /// Message key of what the last change did.
    pub notice: Option<&'static str>,
    /// Message key of why the last password was refused.
    pub error: Option<&'static str>,
}

/// The anonymization part of the patron form (docs/specs/circulacao/lgpd.md, item 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnonymizeSection {
    /// Where its form posts.
    pub action: String,
    /// Message key of why the last anonymization was refused.
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

/// The checkout slip of a patron, to print (docs/specs/circulacao/avisos.md, items 38 to
/// 40).
#[derive(Template)]
#[template(path = "staff_checkout_slip.html")]
pub struct StaffCheckoutSlip<'a> {
    /// Interface texts.
    pub t: &'a I18n,
    /// Message key of the title: the slip or the quick slip.
    pub title_key: &'static str,
    /// The patron's name.
    pub name: String,
    /// The patron's card number.
    pub cardnumber: String,
    /// When it was printed, as shown.
    pub printed: String,
    /// The patron's record, to go back.
    pub record: String,
    /// Its parts.
    pub sections: Vec<SlipSection>,
}

/// One part of a slip, such as the overdue loans.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlipSection {
    /// Message key of its heading.
    pub heading_key: &'static str,
    /// Its loans.
    pub lines: Vec<SlipLine>,
}

/// One loan on a slip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlipLine {
    /// The title.
    pub title: String,
    /// The barcode.
    pub barcode: String,
    /// When it is due, as shown.
    pub due: String,
}
