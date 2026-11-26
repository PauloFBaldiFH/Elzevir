// SPDX-License-Identifier: GPL-3.0-or-later
//! Paying and writing off what a patron owes (docs/specs/circulacao/conta.md).

use elzevir_domain::account::{CreditType, DebitType, PaymentKind, PaymentRefusal};
use elzevir_domain::catalog::{BranchId, Money};
use elzevir_domain::patron::PatronId;
use jiff::Zoned;
use jiff::civil::Date;
use serde::{Deserialize, Serialize};

/// Most debits one payment may name.
pub const MAX_LINES_PER_PAYMENT: usize = 200;

/// One line of a patron's account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AccountLineId(pub i64);

/// A payment or a write-off asked for at the desk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaymentRequest {
    /// Who pays, or whose debt is written off.
    pub patron: PatronId,
    /// A payment or a write-off.
    pub kind: PaymentKind,
    /// How much.
    pub amount: Money,
    /// The debits to pay first, in their order; none means every debit.
    pub lines: Vec<AccountLineId>,
    /// The branch of the desk.
    pub desk: BranchId,
    /// When, in the library's time zone.
    pub at: Zoned,
}

/// A payment or write-off that happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Paid {
    /// The credit it made.
    pub credit: AccountLineId,
    /// What the patron owes now.
    pub owed: Money,
}

/// Why a payment or a write-off did not happen.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PayRefusal {
    /// No patron has this id.
    #[error("no such patron")]
    UnknownPatron,
    /// A debit named is not the patron's, owes nothing, or the list is too long.
    #[error("no such debit to pay")]
    UnknownLine,
    /// The amount does not fit what is owed.
    #[error(transparent)]
    Amount(#[from] PaymentRefusal),
}

impl PayRefusal {
    /// Fluent key of the message.
    #[must_use]
    pub const fn message_key(&self) -> &'static str {
        match self {
            Self::UnknownPatron => "checkout-error-unknown-patron",
            Self::UnknownLine => "account-error-unknown-line",
            Self::Amount(PaymentRefusal::NotPositive) => "account-error-not-positive",
            Self::Amount(PaymentRefusal::OverDue { .. }) => "account-error-over-due",
        }
    }
}

/// What a line of the account is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum LineKind {
    /// The patron owes.
    Debit(DebitType),
    /// The patron owes less.
    Credit(CreditType),
}

/// A line of a patron's account as the staff see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountLine {
    /// Which line.
    pub id: AccountLineId,
    /// Debit or credit, and why.
    pub kind: LineKind,
    /// The day it was made.
    pub day: Date,
    /// The amount: positive for a debit, negative for a credit.
    pub amount: Money,
    /// What is left of it: still owed for a debit, not yet used for a credit.
    pub outstanding: Money,
    /// The Item's barcode, when the line is about an Item.
    pub barcode: Option<String>,
}
