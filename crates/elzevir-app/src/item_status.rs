// SPDX-License-Identifier: GPL-3.0-or-later
//! The statuses that take an Item out of circulation, their names, and changing them at
//! the desk (docs/specs/circulacao/lost-items.md, part a).

use elzevir_domain::account::DebitType;
use elzevir_domain::catalog::{BranchId, ItemId, Money};
use jiff::Zoned;
use serde::{Deserialize, Serialize};

/// Highest number a status takes (item 2).
pub const MAX_STATUS_NUMBER: i64 = 99;
/// Longest name of a status number (item 2).
pub const MAX_STATUS_LABEL: usize = 80;

/// A kind of status an Item has (items 1 and 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum StatusKind {
    /// Lost (Koha: `itemlost`, authorised values `LOST`).
    Lost,
    /// Damaged (Koha: `damaged`, `DAMAGED`).
    Damaged,
    /// Withdrawn (Koha: `withdrawn`, `WITHDRAWN`).
    Withdrawn,
    /// Not for loan (Koha: `notforloan`, `NOT_LOAN`); named here, changed in the catalog.
    NotForLoan,
}

impl StatusKind {
    /// Every kind, in the order the pages list them.
    pub const ALL: [Self; 4] = [Self::Lost, Self::Damaged, Self::Withdrawn, Self::NotForLoan];

    /// The kinds the desk changes (item 3).
    pub const AT_THE_DESK: [Self; 3] = [Self::Lost, Self::Damaged, Self::Withdrawn];

    /// The stored code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Lost => "lost",
            Self::Damaged => "damaged",
            Self::Withdrawn => "withdrawn",
            Self::NotForLoan => "not_for_loan",
        }
    }

    /// The kind a stored code names.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.code() == code)
    }
}

/// The name a library gives one status number (item 2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusName {
    /// The kind.
    pub kind: StatusKind,
    /// The number, from 1 to [`MAX_STATUS_NUMBER`].
    pub number: i64,
    /// The name, 1 to [`MAX_STATUS_LABEL`] characters.
    pub label: String,
}

/// A change to the status names (item 2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StatusNameChange {
    /// Names a number, or renames it.
    Set(StatusName),
    /// Removes a name; the number keeps its generic name.
    Remove {
        /// The kind.
        kind: StatusKind,
        /// The number.
        number: i64,
    },
}

/// Changes one status of an Item at the desk (items 3 to 8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemStatusChange {
    /// The Item.
    pub item: ItemId,
    /// Which status; never [`StatusKind::NotForLoan`].
    pub kind: StatusKind,
    /// The number the page showed, so that a change made meanwhile is not lost.
    pub from: i64,
    /// The new number, 0 or a status number.
    pub to: i64,
    /// When, in the library's time zone.
    pub at: Zoned,
}

/// What finding a lost Item did to the charges of its loss (items 9 to 15).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LostFound {
    /// The replacement price given back (`LOST_FOUND`).
    pub lost_refunded: Option<Money>,
    /// The processing fee given back (`PROCESSING_FOUND`).
    pub processing_refunded: Option<Money>,
    /// The fine stopped by the loss counts again (`restore`).
    pub restored: bool,
    /// The fine of the last loan charged up to the return (`charge`).
    pub charged: Option<Money>,
}

/// What changing a status did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ItemStatusChanged {
    /// What the patron was charged, when an Item on loan was lost (item 4).
    pub charged: Vec<(DebitType, Money)>,
    /// Whether a loan ended (item 4).
    pub loan_ended: bool,
    /// Whether an open transfer was cancelled (item 4).
    pub transfer_cancelled: bool,
    /// What finding the Item did, when it was lost and is not any more (item 9).
    pub found: LostFound,
}

/// Why a status change was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
pub enum ItemStatusRefusal {
    /// No Item has this id or barcode.
    #[error("no such item")]
    UnknownItem,
    /// The status changed since the page was shown.
    #[error("the item's status changed meanwhile")]
    Stale,
    /// A number below 0 or above [`MAX_STATUS_NUMBER`], or a kind the desk does not change.
    #[error("not a status number")]
    BadNumber,
    /// A name that is empty or too long.
    #[error("a status name has 1 to 80 characters")]
    BadLabel,
    /// Another number of the same kind has this name.
    #[error("another status has this name")]
    LabelTaken,
}

impl ItemStatusRefusal {
    /// Fluent key of the message.
    #[must_use]
    pub const fn message_key(self) -> &'static str {
        match self {
            Self::UnknownItem => "item-status-error-unknown",
            Self::Stale => "item-status-error-stale",
            Self::BadNumber => "item-status-error-number",
            Self::BadLabel => "item-status-error-label",
            Self::LabelTaken => "item-status-error-taken",
        }
    }
}

/// The name change as stored: the label trimmed.
///
/// # Errors
///
/// [`ItemStatusRefusal::BadNumber`] or [`ItemStatusRefusal::BadLabel`].
pub fn checked_name(change: &StatusNameChange) -> Result<StatusNameChange, ItemStatusRefusal> {
    let number_ok = |number: i64| (1..=MAX_STATUS_NUMBER).contains(&number);
    match change {
        StatusNameChange::Set(name) => {
            if !number_ok(name.number) {
                return Err(ItemStatusRefusal::BadNumber);
            }
            let label = name.label.trim();
            if label.is_empty() || label.chars().count() > MAX_STATUS_LABEL {
                return Err(ItemStatusRefusal::BadLabel);
            }
            Ok(StatusNameChange::Set(StatusName {
                kind: name.kind,
                number: name.number,
                label: label.to_owned(),
            }))
        }
        StatusNameChange::Remove { kind, number } if number_ok(*number) => {
            Ok(StatusNameChange::Remove {
                kind: *kind,
                number: *number,
            })
        }
        StatusNameChange::Remove { .. } => Err(ItemStatusRefusal::BadNumber),
    }
}

/// Whether the desk may set this status to this number (item 3).
///
/// # Errors
///
/// [`ItemStatusRefusal::BadNumber`].
pub fn checked_change(change: &ItemStatusChange) -> Result<(), ItemStatusRefusal> {
    if change.kind == StatusKind::NotForLoan || !(0..=MAX_STATUS_NUMBER).contains(&change.to) {
        return Err(ItemStatusRefusal::BadNumber);
    }
    Ok(())
}

/// One status of an Item, as the desk shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemStatusLine {
    /// The kind.
    pub kind: StatusKind,
    /// The number; 0 when the status does not apply.
    pub number: i64,
    /// Since when, when it applies and the moment is known.
    pub since: Option<Zoned>,
}

/// An Item as the status page shows it, without its patron (item 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemAtTheDesk {
    /// The Item.
    pub item: ItemId,
    /// Its barcode.
    pub barcode: String,
    /// The Instance's title.
    pub title: String,
    /// The call number.
    pub call_number: Option<String>,
    /// Where it is.
    pub holding: BranchId,
    /// The name of where it is.
    pub holding_name: String,
    /// When its loan is due, while it is on loan.
    pub due: Option<Zoned>,
    /// Lost, damaged and withdrawn, in that order.
    pub statuses: Vec<ItemStatusLine>,
}

/// Reads for the status pages.
pub trait ItemStatusDesk: Send + Sync {
    /// Every status name, by kind and number.
    ///
    /// # Errors
    ///
    /// When storage fails.
    fn status_names(
        &self,
    ) -> Result<Vec<StatusName>, crate::circulation_rules::CirculationDeskError>;

    /// The Item with this barcode, if any.
    ///
    /// # Errors
    ///
    /// When storage fails.
    fn item_at_the_desk(
        &self,
        barcode: &str,
    ) -> Result<Option<ItemAtTheDesk>, crate::circulation_rules::CirculationDeskError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(kind: StatusKind, to: i64) -> ItemStatusChange {
        ItemStatusChange {
            item: ItemId(1),
            kind,
            from: 0,
            to,
            at: jiff::Timestamp::UNIX_EPOCH.to_zoned(jiff::tz::TimeZone::UTC),
        }
    }

    #[test]
    fn the_desk_changes_three_kinds_within_the_range() {
        for kind in StatusKind::AT_THE_DESK {
            assert_eq!(checked_change(&change(kind, 0)), Ok(()));
            assert_eq!(checked_change(&change(kind, 99)), Ok(()));
            assert_eq!(
                checked_change(&change(kind, 100)),
                Err(ItemStatusRefusal::BadNumber)
            );
            assert_eq!(
                checked_change(&change(kind, -1)),
                Err(ItemStatusRefusal::BadNumber)
            );
        }
        assert_eq!(
            checked_change(&change(StatusKind::NotForLoan, 1)),
            Err(ItemStatusRefusal::BadNumber)
        );
    }

    #[test]
    fn names_are_trimmed_and_bounded() {
        let set = |number, label: &str| {
            checked_name(&StatusNameChange::Set(StatusName {
                kind: StatusKind::Lost,
                number,
                label: label.to_owned(),
            }))
        };
        assert_eq!(
            set(1, "  Lost  "),
            Ok(StatusNameChange::Set(StatusName {
                kind: StatusKind::Lost,
                number: 1,
                label: "Lost".to_owned(),
            }))
        );
        assert_eq!(set(1, "   "), Err(ItemStatusRefusal::BadLabel));
        assert_eq!(set(1, &"x".repeat(81)), Err(ItemStatusRefusal::BadLabel));
        assert!(set(1, &"x".repeat(80)).is_ok());
        assert_eq!(set(0, "Lost"), Err(ItemStatusRefusal::BadNumber));
        assert_eq!(set(100, "Lost"), Err(ItemStatusRefusal::BadNumber));
        for kind in StatusKind::ALL {
            assert_eq!(StatusKind::from_code(kind.code()), Some(kind));
        }
    }
}
