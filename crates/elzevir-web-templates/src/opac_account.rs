// SPDX-License-Identifier: GPL-3.0-or-later
//! The patron's side of the OPAC: the sign-in panel and the account page
//! (docs/specs/opac/conta-do-leitor.md).

use askama::Template;

use crate::{AccountRow, I18n};

/// What the "Patron account" panel shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatronPanel {
    /// Patron accounts are not connected: the disabled placeholder.
    Soon,
    /// The sign-in form.
    SignIn {
        /// Anti-forgery token of the form.
        csrf: String,
        /// The card number typed, after a refusal.
        card: String,
        /// Fluent key of the refusal shown.
        error: Option<&'static str>,
    },
    /// A patron is signed in.
    SignedIn {
        /// The name they are greeted with.
        greeting: String,
        /// Anti-forgery token of their session, for signing out.
        csrf: String,
    },
}

/// `GET /conta/entrar`: the panel on a page of its own.
#[derive(Template)]
#[template(path = "opac_login.html")]
pub struct OpacLogin<'a> {
    /// Interface texts.
    pub t: &'a I18n,
    /// The panel.
    pub panel: PatronPanel,
}

/// `GET /conta`: the patron's own account.
#[derive(Template)]
#[template(path = "opac_account.html")]
pub struct OpacAccount<'a> {
    /// Interface texts.
    pub t: &'a I18n,
    /// Anti-forgery token of the session, for signing out.
    pub csrf: &'a str,
    /// Full name.
    pub name: String,
    /// Card number.
    pub cardnumber: String,
    /// Home branch.
    pub branch_name: String,
    /// Category.
    pub category_name: String,
    /// Last day of the enrolment, as shown.
    pub expires: String,
    /// The restrictions that hold today, as told to the patron.
    pub restrictions: Vec<String>,
    /// What the patron owes, when something.
    pub owed: Option<String>,
    /// What happened to a renewal just asked for.
    pub notice: Option<OpacNotice>,
    /// Whether the patron may renew here: no restriction holds.
    pub can_renew: bool,
    /// The loans, the first due first.
    pub loans: Vec<OpacLoan>,
    /// The account's lines, newest first.
    pub account: Vec<AccountRow>,
}

/// A loan on the patron's account page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpacLoan {
    /// The Item's barcode, posted by the renewal button.
    pub barcode: String,
    /// The Instance's title.
    pub title: String,
    /// The due date as shown.
    pub due: String,
    /// Whether it is overdue now.
    pub overdue: bool,
    /// The renewals left, as shown.
    pub renewals_left: String,
    /// Whether the renewal button is offered.
    pub renewable: bool,
}

/// What the account page says after a renewal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpacNotice {
    /// Whether it was refused.
    pub refused: bool,
    /// What it says.
    pub text: String,
}
