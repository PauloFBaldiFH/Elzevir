// SPDX-License-Identifier: GPL-3.0-or-later
//! Placing and cancelling holds at the desk (docs/specs/circulacao/reservas.md).

use elzevir_domain::catalog::BranchId;
use elzevir_domain::hold::{Found, HoldMatch, HoldRefusal};
use elzevir_domain::patron::PatronId;
use jiff::Zoned;
use jiff::civil::Date;
use serde::{Deserialize, Serialize};

pub use elzevir_domain::hold::HoldId;

/// What a hold asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HoldLevel {
    /// The Item with the barcode.
    Item,
    /// Any Item of the barcode's Instance.
    Record,
}

/// A hold asked for at the desk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HoldRequest {
    /// Who asks.
    pub patron: PatronId,
    /// The barcode of an Item: the one held, or one of the Instance held.
    pub barcode: String,
    /// This Item or any of its Instance.
    pub level: HoldLevel,
    /// Where it is picked up.
    pub pickup: BranchId,
    /// When, in the library's time zone.
    pub at: Zoned,
}

/// A hold cancelled at the desk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HoldCancellation {
    /// Whose hold: a hold of another patron is refused.
    pub patron: PatronId,
    /// Which hold.
    pub hold: HoldId,
    /// When, in the library's time zone.
    pub at: Zoned,
    /// The patron cancels on the OPAC: a hold already set aside is cancelled only at the
    /// desk (Koha: `Koha::Hold::is_cancelable_from_opac`).
    #[serde(default)]
    pub by_patron: bool,
}

/// How long a hold stays suspended (Koha: `suspend_until`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Suspension {
    /// Until someone resumes it.
    Indefinite,
    /// Until the start of that day, when the hourly run resumes it
    /// (Koha: `C4::Reserves::AutoUnsuspendReserves`).
    Until(Date),
}

/// What to do with a hold's suspension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SuspensionChange {
    /// Suspend it, or change how long (Koha: `Koha::Hold::suspend_hold`).
    Suspend(Suspension),
    /// Let Items fill it again (Koha: `Koha::Hold::resume`).
    Resume,
}

/// A hold suspended or resumed at the desk or on the OPAC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HoldSuspensionRequest {
    /// Whose hold: a hold of another patron is refused.
    pub patron: PatronId,
    /// Which hold.
    pub hold: HoldId,
    /// Suspend or resume.
    pub change: SuspensionChange,
}

/// A hold placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HoldPlaced {
    /// The new hold.
    pub hold: HoldId,
    /// Its place in the queue.
    pub priority: u32,
}

/// The hold a checked-in Item answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckinHold {
    /// The hold, and whether it is already set aside.
    pub found: HoldMatch,
    /// Where it is picked up.
    pub pickup: BranchId,
}

/// An Item set aside at the desk for a hold the checkin found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HoldAside {
    /// Which hold.
    pub hold: HoldId,
    /// The Item's barcode.
    pub barcode: String,
    /// The desk's branch, where the Item is.
    pub desk: BranchId,
    /// When, in the library's time zone.
    pub at: Zoned,
}

/// What setting an Item aside did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetAside {
    /// Waiting here, or in transit to the pickup branch.
    pub found: Found,
    /// Where it is picked up.
    pub pickup: BranchId,
    /// The last day to pick it up, when it is waiting.
    pub expires_on: Option<Date>,
}

/// Why a hold was not placed or cancelled.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HoldRefused {
    /// No patron has this id.
    #[error("no such patron")]
    UnknownPatron,
    /// No Item has this barcode.
    #[error("no such barcode")]
    UnknownBarcode,
    /// The pickup branch does not exist.
    #[error("no such branch")]
    UnknownBranch,
    /// The patron has no such hold.
    #[error("no such hold")]
    UnknownHold,
    /// The Item cannot fill this hold: it is of another Instance, or the hold asks for
    /// another Item.
    #[error("the item cannot fill this hold")]
    WrongItem,
    /// The library's rules refuse it.
    #[error(transparent)]
    Rules(#[from] HoldRefusal),
    /// The hold is waiting or in transit, so only the desk cancels it.
    #[error("the hold is already set aside")]
    SetAside,
    /// The hold is waiting or in transit, so it cannot be suspended
    /// (Koha: `Koha::Exceptions::Hold::CannotSuspendFound`).
    #[error("a hold already set aside cannot be suspended")]
    SuspendSetAside,
}

impl HoldRefused {
    /// Fluent key of the message.
    #[must_use]
    pub const fn message_key(&self) -> &'static str {
        match self {
            Self::UnknownPatron => "checkout-error-unknown-patron",
            Self::UnknownBarcode => "checkout-error-unknown-barcode",
            Self::UnknownBranch => "hold-error-unknown-branch",
            Self::UnknownHold => "hold-error-unknown-hold",
            Self::WrongItem => "hold-error-wrong-item",
            Self::SetAside => "hold-error-set-aside",
            Self::SuspendSetAside => "hold-error-suspend-set-aside",
            Self::Rules(refusal) => match refusal {
                HoldRefusal::PickupNotValid => "hold-error-pickup",
                HoldRefusal::Restricted => "hold-error-restricted",
                HoldRefusal::DebtLimit { .. } => "hold-error-debt",
                HoldRefusal::HoldLimit { .. } => "hold-error-hold-limit",
                HoldRefusal::ItemAlreadyOnHold => "hold-error-item-already",
                HoldRefusal::NoReservesAllowed => "hold-error-not-allowed",
                HoldRefusal::TooManyHoldsForThisRecord { .. } => "hold-error-record-limit",
                HoldRefusal::TooManyReservesToday { .. } => "hold-error-day-limit",
                HoldRefusal::TooManyReserves { .. } => "hold-error-too-many",
                HoldRefusal::NotReservable => "hold-error-not-reservable",
                HoldRefusal::CannotReserveFromOtherBranches | HoldRefusal::BranchNotInHoldGroup => {
                    "hold-error-other-branch"
                }
            },
        }
    }
}

/// A patron's hold as the staff see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatronHold {
    /// Which hold.
    pub id: HoldId,
    /// The Instance's title.
    pub title: String,
    /// The Item's barcode, for an Item hold.
    pub barcode: Option<String>,
    /// The pickup branch's name.
    pub pickup: String,
    /// The day it was placed.
    pub placed_on: Date,
    /// Its place in the queue; 0 when set aside.
    pub priority: u32,
    /// Set aside for the patron, or not yet.
    pub found: Option<Found>,
    /// The last day to pick it up, when it is waiting.
    pub expires_on: Option<Date>,
    /// Whether it is suspended, and until when.
    pub suspended: Option<Suspension>,
}
