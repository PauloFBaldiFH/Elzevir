// SPDX-License-Identifier: GPL-3.0-or-later
//! Lending an Item to a patron (docs/specs/circulacao/emprestimo.md).

use elzevir_domain::catalog::BranchId;
use elzevir_domain::checkout::{Alert, Confirm, Impossible};
use elzevir_domain::patron::PatronId;
use jiff::Zoned;
use serde::{Deserialize, Serialize};

/// Most characters of a barcode the checkout reads.
pub const MAX_BARCODE_LENGTH: usize = 64;

/// One loan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct LoanId(pub i64);

/// A checkout asked for at the desk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckoutRequest {
    /// Who borrows.
    pub patron: PatronId,
    /// The Item's barcode, as typed or scanned.
    pub barcode: String,
    /// The branch of the desk, where the Item now is (Koha: the logged-in library).
    pub desk: BranchId,
    /// A loan to be used inside the library (Koha: `onsite_checkout`).
    pub on_site: bool,
    /// The staff confirmed what the check asked to confirm (Koha: `issueconfirmed`).
    pub confirmed: bool,
    /// When, in the library's time zone.
    pub at: Zoned,
}

/// A checkout that happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckedOut {
    /// The new loan.
    pub loan: LoanId,
    /// When it is due.
    pub due: Zoned,
    /// What the staff should be told.
    pub alerts: Vec<Alert>,
}

/// Why a checkout did not happen.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CheckoutRefusal {
    /// No Item has this barcode (Koha: `UNKNOWN_BARCODE`).
    #[error("no item has this barcode")]
    UnknownBarcode,
    /// No such patron.
    #[error("no such patron")]
    UnknownPatron,
    /// The checkout cannot happen.
    #[error("the checkout cannot happen")]
    Impossible(Vec<Impossible>),
    /// The staff must confirm these first.
    #[error("the checkout needs confirmation")]
    NeedsConfirmation(Vec<Confirm>),
    /// The Item is on loan; returning and renewing come with task 3.5.
    #[error("the item is on loan")]
    OnLoan,
    /// The rules give no due date (the calendar has no open day).
    #[error("no due date can be found")]
    NoDueDate,
}

impl CheckoutRefusal {
    /// Fluent key of the message.
    #[must_use]
    pub const fn message_key(&self) -> &'static str {
        match self {
            Self::UnknownBarcode => "checkout-error-unknown-barcode",
            Self::UnknownPatron => "checkout-error-unknown-patron",
            Self::Impossible(_) => "checkout-error-impossible",
            Self::NeedsConfirmation(_) => "checkout-error-needs-confirmation",
            Self::OnLoan => "checkout-error-on-loan",
            Self::NoDueDate => "checkout-error-no-due-date",
        }
    }
}

/// The barcode as the checkout looks it up: without spaces at the ends.
///
/// Koha: `barcodedecode` (with `itemBarcodeInputFilter` off, it only trims).
#[must_use]
pub fn checked_barcode(barcode: &str) -> Option<&str> {
    let barcode = barcode.trim();
    (!barcode.is_empty() && barcode.chars().count() <= MAX_BARCODE_LENGTH).then_some(barcode)
}
