// SPDX-License-Identifier: GPL-3.0-or-later
//! Placing and cancelling holds at the desk (docs/specs/circulacao/reservas.md).

use elzevir_domain::catalog::BranchId;
use elzevir_domain::hold::HoldRefusal;
use elzevir_domain::patron::PatronId;
use jiff::Zoned;
use jiff::civil::Date;
use serde::{Deserialize, Serialize};

/// One hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct HoldId(pub i64);

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
}

/// A hold placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HoldPlaced {
    /// The new hold.
    pub hold: HoldId,
    /// Its place in the queue.
    pub priority: u32,
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
    /// The library's rules refuse it.
    #[error(transparent)]
    Rules(#[from] HoldRefusal),
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
    /// Its place in the queue.
    pub priority: u32,
}
