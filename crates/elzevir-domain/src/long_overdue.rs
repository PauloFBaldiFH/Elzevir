// SPDX-License-Identifier: GPL-3.0-or-later
//! Items on loan for so long past their due date that they are declared lost
//! (docs/specs/circulacao/conta.md, items 20 to 26).

use jiff::{ToSpan, Zoned};
use serde::{Deserialize, Serialize};

use crate::account::DebitType;
use crate::catalog::Money;

/// Koha's default end of the range, in days (`--maxdays`).
pub const DEFAULT_MAX_DAYS: u32 = 366;

/// How the library declares long overdue Items lost.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LongOverdueSettings {
    /// Days past due after which the Item is lost (`DefaultLongOverdueDays`).
    pub days: u32,
    /// Due dates older than this many days are left alone (`--maxdays`).
    pub max_days: u32,
    /// The lost status the Item gets (`DefaultLongOverdueLostValue`).
    pub lost_status: i64,
    /// Charge the patron for the Item and end the loan (`DefaultLongOverdueChargeValue`
    /// equal to the lost status, with `MarkLostItemsAsReturned` holding `cronjob`).
    pub charge: bool,
    /// End the loan even when nothing is charged (`--mark-returned`).
    pub mark_returned: bool,
}

impl LongOverdueSettings {
    /// Whether the loan ends when its Item is declared lost.
    ///
    /// Koha: `longoverdue.pl` ends it through `LostItem` when charging, whose
    /// `MarkLostItemsAsReturned` holds `cronjob` by default, and otherwise only with
    /// `--mark-returned`.
    #[must_use]
    pub const fn ends_loan(&self) -> bool {
        self.charge || self.mark_returned
    }
}

/// Whether a loan due at `due` is long overdue at `at`: due before midnight `days` days
/// ago and not before midnight `max_days` days ago, in calendar days.
///
/// Koha: `longoverdue.pl` (`DATE_SUB(CURDATE(), INTERVAL ? DAY) > date_due` and
/// `<= date_due`).
#[must_use]
pub fn is_long_overdue(due: &Zoned, at: &Zoned, settings: &LongOverdueSettings) -> bool {
    let midnight = |days: u32| {
        at.date()
            .checked_sub(i64::from(days).days())
            .and_then(|day| day.to_zoned(at.time_zone().clone()))
            .ok()
    };
    let (Some(start), Some(end)) = (midnight(settings.days), midnight(settings.max_days)) else {
        return false;
    };
    *due < start && *due >= end
}

/// What the patron is charged for a lost Item: the processing fee and then the
/// replacement price, each only when above zero.
///
/// Koha: `C4::Accounts::chargelostitem` (without `useDefaultReplacementCost`).
#[must_use]
pub fn lost_charges(
    processing_fee: Option<Money>,
    replacement_price: Option<Money>,
) -> Vec<(DebitType, Money)> {
    [
        (DebitType::Processing, processing_fee),
        (DebitType::Lost, replacement_price),
    ]
    .into_iter()
    .filter_map(|(kind, amount)| {
        amount
            .filter(|money| money.0 > 0)
            .map(|money| (kind, money))
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> LongOverdueSettings {
        LongOverdueSettings {
            days: 30,
            max_days: DEFAULT_MAX_DAYS,
            lost_status: 1,
            charge: true,
            mark_returned: false,
        }
    }

    fn at(text: &str) -> Zoned {
        format!("{text}[America/Sao_Paulo]").parse().unwrap()
    }

    #[test]
    fn the_range_is_counted_in_days_from_midnight() {
        let now = at("2026-03-31T10:00:00-03:00");
        let lost = |due: &str| is_long_overdue(&at(due), &now, &settings());
        assert!(lost("2026-02-28T23:59:00-03:00"));
        assert!(
            !lost("2026-03-01T00:00:00-03:00"),
            "exactly 30 days is not yet"
        );
        assert!(!lost("2026-03-30T23:59:00-03:00"));
        assert!(
            lost("2025-03-30T00:00:00-03:00"),
            "366 days is still in range"
        );
        assert!(!lost("2025-03-29T23:59:00-03:00"), "older than max_days");
    }

    #[test]
    fn only_charges_above_zero_are_made_with_the_fee_first() {
        assert_eq!(
            lost_charges(Some(Money(300)), Some(Money(4500))),
            vec![
                (DebitType::Processing, Money(300)),
                (DebitType::Lost, Money(4500))
            ]
        );
        assert_eq!(
            lost_charges(Some(Money(0)), Some(Money(4500))),
            vec![(DebitType::Lost, Money(4500))]
        );
        assert!(lost_charges(None, None).is_empty());
    }

    #[test]
    fn the_loan_ends_when_charging_or_when_asked() {
        assert!(settings().ends_loan());
        let only_lost = LongOverdueSettings {
            charge: false,
            ..settings()
        };
        assert!(!only_lost.ends_loan());
        assert!(
            LongOverdueSettings {
                mark_returned: true,
                ..only_lost
            }
            .ends_loan()
        );
    }
}
