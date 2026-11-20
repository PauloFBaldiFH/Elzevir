// SPDX-License-Identifier: GPL-3.0-or-later
//! Setting and reading circulation rules (docs/specs/circulacao/regras.md).

pub use elzevir_data::RuleKind;
use elzevir_domain::circulation_rules::{
    CirculationRule, RuleScopeKey, RuleValueError, checked_value,
};
use elzevir_domain::long_overdue::LongOverdueSettings;
use jiff::Zoned;
use serde::{Deserialize, Serialize};

use crate::accounts::{Paid, PayRefusal, PaymentRequest};
use crate::calendar::CalendarChange;
use crate::holds::{HoldAside, HoldCancellation, HoldPlaced, HoldRefused, HoldRequest, SetAside};
use crate::loans::{
    CheckedIn, CheckedOut, CheckinRefusal, CheckinRequest, CheckoutRefusal, CheckoutRequest,
    RenewRefusal, RenewRequest, Renewed,
};
use crate::staff::StaffId;

/// Most rules one change may set.
pub const MAX_RULES_PER_CHANGE: usize = RuleKind::ALL.len();

/// Circulation commands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CirculationCommand {
    /// Sets or removes rules at one scope.
    SetRules(RuleChange),
    /// Changes one branch's calendar (docs/specs/circulacao/calendario.md).
    ChangeCalendar(CalendarChange),
    /// Lends an Item to a patron (docs/specs/circulacao/emprestimo.md).
    Checkout(CheckoutRequest),
    /// Takes an Item back (docs/specs/circulacao/devolucao.md).
    Checkin(CheckinRequest),
    /// Renews a loan (docs/specs/circulacao/renovacao.md).
    Renew(RenewRequest),
    /// Pays or writes off what a patron owes (docs/specs/circulacao/conta.md).
    Pay(PaymentRequest),
    /// Places a hold (docs/specs/circulacao/reservas.md).
    PlaceHold(HoldRequest),
    /// Cancels a hold (docs/specs/circulacao/reservas.md).
    CancelHold(HoldCancellation),
    /// Sets a checked-in Item aside for the hold it answers (docs/specs/circulacao/reservas.md).
    SetHoldAside(HoldAside),
    /// Brings the fine of every overdue loan to what it is worth at `at`: the daily run
    /// (docs/specs/circulacao/conta.md).
    ChargeOverdues {
        /// When, in the library's time zone.
        at: Zoned,
    },
    /// Declares lost the Items on loan long past their due date, and charges for them when
    /// the settings say so (docs/specs/circulacao/conta.md).
    MarkLongOverdue {
        /// When, in the library's time zone.
        at: Zoned,
        /// The library's settings.
        settings: LongOverdueSettings,
    },
    /// Cancels the holds waiting past their last day to pick up
    /// (docs/specs/circulacao/reservas.md).
    ExpireHolds {
        /// When, in the library's time zone.
        at: Zoned,
    },
}

/// New values for some rules at one scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleChange {
    /// Where the rules apply.
    pub scope: RuleScopeKey,
    /// Each rule by its name (Koha's `rule_name`) with its value; `None` removes it.
    pub values: Vec<(String, Option<String>)>,
}

/// What a circulation command returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CirculationOutput {
    /// How many rules were stored and how many removed.
    RulesSet {
        /// Stored or changed.
        stored: usize,
        /// Removed.
        removed: usize,
    },
    /// How many calendar entries were stored or removed.
    CalendarChanged {
        /// Entries stored, replaced or removed.
        entries: usize,
    },
    /// An Item was lent.
    CheckedOut(CheckedOut),
    /// An Item was checked in.
    CheckedIn(CheckedIn),
    /// A loan was renewed.
    Renewed(Renewed),
    /// A payment or write-off was made.
    Paid(Paid),
    /// The daily run went through the overdue loans.
    OverduesCharged(OverduesCharged),
    /// What the run of long overdue Items did.
    LongOverdueMarked(LongOverdueMarked),
    /// A hold was placed.
    HoldPlaced(HoldPlaced),
    /// A hold was cancelled.
    HoldCancelled,
    /// An Item was set aside for a hold.
    HoldSetAside(SetAside),
    /// How many waiting holds were cancelled for being past their last day.
    HoldsExpired {
        /// Holds cancelled.
        cancelled: usize,
    },
}

/// What the run of long overdue Items did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LongOverdueMarked {
    /// Items declared lost.
    pub lost: usize,
    /// Of those, the ones charged for.
    pub charged: usize,
}

/// What the daily run of overdue fines did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OverduesCharged {
    /// Loans overdue.
    pub overdue: usize,
    /// Loans whose fine is charged now.
    pub charged: usize,
}

/// Why a circulation command was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CirculationRefusal {
    /// No rule has this name.
    #[error("no rule is named `{0}`")]
    UnknownRule(String),
    /// The value does not fit the rule.
    #[error("rule `{rule}`: {error}")]
    BadValue {
        /// The rule's name.
        rule: String,
        /// What is wrong.
        error: RuleValueError,
    },
    /// The same rule twice, or more rules than there are kinds.
    #[error("each rule may appear once")]
    Repeated,
    /// The scope names a branch that does not exist.
    #[error("no such branch")]
    UnknownBranch,
    /// The scope names a patron category that does not exist.
    #[error("no such patron category")]
    UnknownCategory,
    /// The scope names a circulation policy that does not exist.
    #[error("no such circulation policy")]
    UnknownPolicy,
    /// A closing without a title, or with a title or description too long.
    #[error("a closing needs a title of at most 50 characters")]
    BadHolidayTitle,
    /// A weekday past Saturday.
    #[error("no such weekday")]
    BadWeekday,
    /// A day that does not exist, or a range that runs backwards or is too long.
    #[error("these dates do not make a range")]
    BadDates,
    /// A checkout was refused.
    #[error(transparent)]
    Checkout(#[from] CheckoutRefusal),
    /// A checkin was refused.
    #[error(transparent)]
    Checkin(#[from] CheckinRefusal),
    /// A renewal was refused.
    #[error(transparent)]
    Renew(#[from] RenewRefusal),
    /// A payment or write-off was refused.
    #[error(transparent)]
    Pay(#[from] PayRefusal),
    /// A hold was refused, or could not be cancelled.
    #[error(transparent)]
    Hold(#[from] HoldRefused),
}

impl CirculationRefusal {
    /// Fluent key of the message.
    #[must_use]
    pub const fn message_key(&self) -> &'static str {
        match self {
            Self::UnknownRule(_) => "rules-error-unknown-rule",
            Self::BadValue {
                error: RuleValueError::OutOfScope(_),
                ..
            } => "rules-error-out-of-scope",
            Self::BadValue {
                error: RuleValueError::NotDecimal,
                ..
            } => "rules-error-not-decimal",
            Self::BadValue {
                error: RuleValueError::TooLong,
                ..
            } => "rules-error-too-long",
            Self::Repeated => "rules-error-repeated",
            Self::UnknownBranch => "rules-error-unknown-branch",
            Self::UnknownCategory => "rules-error-unknown-category",
            Self::UnknownPolicy => "rules-error-unknown-policy",
            Self::BadHolidayTitle => "calendar-error-title",
            Self::BadWeekday | Self::BadDates => "calendar-error-dates",
            Self::Checkout(refusal) => refusal.message_key(),
            Self::Checkin(refusal) => refusal.message_key(),
            Self::Renew(refusal) => refusal.message_key(),
            Self::Pay(refusal) => refusal.message_key(),
            Self::Hold(refusal) => refusal.message_key(),
        }
    }
}

/// The change as stored: each rule with its kind and the value to keep (`None` removes).
///
/// # Errors
///
/// [`CirculationRefusal`] for an unknown or repeated rule or a value that does not fit.
pub fn checked_rules(
    change: &RuleChange,
) -> Result<Vec<(RuleKind, Option<String>)>, CirculationRefusal> {
    if change.values.len() > MAX_RULES_PER_CHANGE {
        return Err(CirculationRefusal::Repeated);
    }
    let mut checked: Vec<(RuleKind, Option<String>)> = Vec::with_capacity(change.values.len());
    for (name, value) in &change.values {
        let kind = RuleKind::from_koha_name(name)
            .ok_or_else(|| CirculationRefusal::UnknownRule(name.clone()))?;
        if checked.iter().any(|(seen, _)| *seen == kind) {
            return Err(CirculationRefusal::Repeated);
        }
        let value = checked_value(kind, &change.scope, value.as_deref()).map_err(|error| {
            CirculationRefusal::BadValue {
                rule: name.clone(),
                error,
            }
        })?;
        checked.push((kind, value));
    }
    Ok(checked)
}

/// Why the rule book or the calendar could not answer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CirculationDeskError {
    /// The change was refused.
    #[error(transparent)]
    Refused(#[from] CirculationRefusal),
    /// The writer queue is full; try again.
    #[error("the writer is busy")]
    Busy,
    /// Storage failed.
    #[error("circulation storage unavailable: {0}")]
    Unavailable(String),
}

/// The stored circulation rules, for the one function that resolves them and for the
/// screen that changes them.
pub trait RuleBook: Send + Sync {
    /// Every rule.
    ///
    /// # Errors
    ///
    /// When storage fails.
    fn rules(&self) -> Result<Vec<CirculationRule>, CirculationDeskError>;

    /// Runs `circ.set_rules` as `actor`.
    ///
    /// # Errors
    ///
    /// [`CirculationDeskError::Refused`] with the refusal, or why storage failed.
    fn set_rules(
        &self,
        actor: StaffId,
        change: RuleChange,
    ) -> Result<CirculationOutput, CirculationDeskError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use elzevir_domain::catalog::PolicyId;

    fn change(values: &[(&str, Option<&str>)]) -> RuleChange {
        RuleChange {
            scope: RuleScopeKey::default(),
            values: values
                .iter()
                .map(|(name, value)| ((*name).to_owned(), value.map(str::to_owned)))
                .collect(),
        }
    }

    #[test]
    fn rules_are_known_once_and_fit_their_kind() {
        assert_eq!(
            checked_rules(&change(&[("issuelength", Some("14")), ("fine", None)])),
            Ok(vec![
                (RuleKind::Issuelength, Some("14".to_owned())),
                (RuleKind::Fine, None)
            ])
        );
        assert_eq!(
            checked_rules(&change(&[("itemtype", Some("BK"))])),
            Err(CirculationRefusal::UnknownRule("itemtype".to_owned()))
        );
        assert_eq!(
            checked_rules(&change(&[("fine", Some("1")), ("fine", Some("2"))])),
            Err(CirculationRefusal::Repeated)
        );
        let mut per_policy = change(&[("max_holds", Some("2"))]);
        per_policy.scope.policy = Some(PolicyId(1));
        assert!(matches!(
            checked_rules(&per_policy),
            Err(CirculationRefusal::BadValue { .. })
        ));
    }
}
