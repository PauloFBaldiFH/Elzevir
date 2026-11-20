// SPDX-License-Identifier: GPL-3.0-or-later
//! Lending an Item to a patron and taking it back (docs/specs/circulacao/emprestimo.md,
//! docs/specs/circulacao/devolucao.md).

use elzevir_domain::catalog::{BranchId, ItemId, Money};
use elzevir_domain::checkin::{Destination, LoanOutcome, LostOutcome};
use elzevir_domain::checkout::{Alert, Confirm, Impossible};
use elzevir_domain::patron::PatronId;
use elzevir_domain::renewal::RenewalRefusal;
use elzevir_domain::suspension::{RestrictionKind, SuspensionEnd};
use jiff::Zoned;
use jiff::civil::Date;
use serde::{Deserialize, Serialize};

use crate::accounts::AccountLine;
use crate::circulation_rules::{CirculationCommand, CirculationDeskError, CirculationOutput};
use crate::holds::CheckinHold;
use crate::holds::PatronHold;
use crate::staff::StaffId;

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

/// A checkin at the desk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckinRequest {
    /// The Item's barcode, as typed or scanned.
    pub barcode: String,
    /// The branch of the desk (Koha: the logged-in library).
    pub desk: BranchId,
    /// When, in the library's time zone.
    pub at: Zoned,
}

/// A checkin that happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckedIn {
    /// Who had the Item, when its loan ended.
    pub patron: Option<PatronId>,
    /// What happened to the loan.
    pub loan: LoanOutcome,
    /// What happened to a lost Item.
    pub lost: LostOutcome,
    /// Where the Item goes next.
    pub destination: Destination,
    /// When the patron's suspension ends, when a late return suspended them.
    pub suspended_until: Option<Date>,
    /// The overdue fine the late return charged, as it stands now.
    pub fine: Option<Money>,
    /// The hold the Item answers, if any (docs/specs/circulacao/reservas.md).
    pub hold: Option<CheckinHold>,
}

/// Why a checkin did not happen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CheckinRefusal {
    /// No Item has this barcode (Koha: `BadBarcode`).
    #[error("no item has this barcode")]
    UnknownBarcode,
    /// The Item is withdrawn (Koha: `withdrawn` with `BlockReturnOfWithdrawnItems`).
    #[error("the item is withdrawn")]
    Withdrawn,
}

impl CheckinRefusal {
    /// Fluent key of the message.
    #[must_use]
    pub const fn message_key(&self) -> &'static str {
        match self {
            Self::UnknownBarcode => "checkout-error-unknown-barcode",
            Self::Withdrawn => "checkin-error-withdrawn",
        }
    }
}

/// A renewal at the desk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenewRequest {
    /// The Item's barcode, as typed or scanned.
    pub barcode: String,
    /// The staff override the renewal count, the overdue block and the earliest date
    /// (Koha: `override_limit`).
    pub override_limit: bool,
    /// When, in the library's time zone.
    pub at: Zoned,
}

/// A renewal that happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Renewed {
    /// Who has the Item.
    pub patron: PatronId,
    /// When it is due now.
    pub due: Zoned,
    /// How many times the loan was renewed, this one included.
    pub renewals: u32,
}

/// Why a renewal did not happen.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RenewRefusal {
    /// No Item has this barcode.
    #[error("no item has this barcode")]
    UnknownBarcode,
    /// The Item is not on loan (Koha: `no_checkout`).
    #[error("the item is not on loan")]
    NotOnLoan,
    /// The loan may not be renewed.
    #[error(transparent)]
    Refused(#[from] RenewalRefusal),
    /// The rules give no due date (the calendar has no open day).
    #[error("no due date can be found")]
    NoDueDate,
}

impl RenewRefusal {
    /// Fluent key of the message.
    #[must_use]
    pub const fn message_key(&self) -> &'static str {
        match self {
            Self::UnknownBarcode => "checkout-error-unknown-barcode",
            Self::NotOnLoan => "renew-error-not-on-loan",
            Self::Refused(RenewalRefusal::OnSite) => "renew-error-on-site",
            Self::Refused(RenewalRefusal::TooMany) => "renew-error-too-many",
            Self::Refused(RenewalRefusal::Overdue) => "renew-error-overdue",
            Self::Refused(RenewalRefusal::TooSoon { .. }) => "renew-error-too-soon",
            Self::NoDueDate => "checkout-error-no-due-date",
        }
    }
}

/// A loan as the patron's record lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatronLoan {
    /// The Item's barcode.
    pub barcode: String,
    /// The title of the Item's Instance.
    pub title: String,
    /// When it is due.
    pub due: Zoned,
    /// When it was lent.
    pub issued: Zoned,
    /// When it was last renewed, if ever.
    pub renewed: Option<Zoned>,
    /// How many times it was renewed.
    pub renewals: u32,
    /// A loan to be used inside the library.
    pub on_site: bool,
}

/// The circulation desk: the patron's loans and suspension, and the circulation commands
/// (interface §18.3).
pub trait LoanDesk: Send + Sync {
    /// The patron's loans, the first due first.
    ///
    /// # Errors
    ///
    /// When storage fails.
    fn loans(&self, patron: PatronId) -> Result<Vec<PatronLoan>, CirculationDeskError>;

    /// The patron's restrictions that hold on `today`, by kind.
    ///
    /// # Errors
    ///
    /// When storage fails.
    fn restrictions(
        &self,
        patron: PatronId,
        today: Date,
    ) -> Result<Vec<(RestrictionKind, SuspensionEnd)>, CirculationDeskError>;

    /// What the patron owes: the outstanding debits.
    ///
    /// # Errors
    ///
    /// When storage fails.
    fn owed(&self, patron: PatronId) -> Result<Money, CirculationDeskError>;

    /// The patron's account, newest line first.
    ///
    /// # Errors
    ///
    /// When storage fails.
    fn account(&self, patron: PatronId) -> Result<Vec<AccountLine>, CirculationDeskError>;

    /// The patron's holds, oldest first.
    ///
    /// # Errors
    ///
    /// When storage fails.
    fn holds(&self, patron: PatronId) -> Result<Vec<PatronHold>, CirculationDeskError>;

    /// The branch an Item is at, as the desk of a checkout from its pill.
    ///
    /// # Errors
    ///
    /// When storage fails.
    fn holding_branch(&self, item: ItemId) -> Result<Option<BranchId>, CirculationDeskError>;

    /// Who has the Item, while it is on loan; only for the arrow across the wall.
    ///
    /// # Errors
    ///
    /// When storage fails.
    fn borrower(&self, item: ItemId) -> Result<Option<PatronId>, CirculationDeskError>;

    /// Runs one circulation command as `actor`.
    ///
    /// # Errors
    ///
    /// [`CirculationDeskError::Refused`] with the refusal, or why storage failed.
    fn circulate(
        &self,
        actor: StaffId,
        command: CirculationCommand,
    ) -> Result<CirculationOutput, CirculationDeskError>;
}
