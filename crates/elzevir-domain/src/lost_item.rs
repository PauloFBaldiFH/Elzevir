// SPDX-License-Identifier: GPL-3.0-or-later
//! What finding a lost Item does to the charges of its loss
//! (docs/specs/circulacao/lost-items.md, items 10 to 14).

use serde::{Deserialize, Serialize};

use crate::catalog::Money;

/// The rule `lostreturn` or `processingreturn` of the branch that finds the Item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LostReturn {
    /// "0": nothing is given back.
    NoRefund,
    /// `refund`, Koha's default: what is owed and what was paid.
    Refund,
    /// `refund_unpaid`: only what is still owed.
    RefundUnpaid,
    /// `restore`: as `refund`, and the fine stopped by the loss counts again.
    Restore,
    /// `charge`: as `refund`, and the fine of the last loan is charged up to the return.
    Charge,
}

impl LostReturn {
    /// The policy a stored rule value names; no rule means `refund`.
    ///
    /// Koha: `Koha::CirculationRules::get_lostreturn_policy` defaults to `refund`, and
    /// `Koha::Item::_set_found_trigger` gives back nothing for a false value ("0" or
    /// empty) and treats any other value it does not name like `refund`.
    #[must_use]
    pub fn from_rule(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            Some("" | "0") => Self::NoRefund,
            Some("refund_unpaid") => Self::RefundUnpaid,
            Some("restore") => Self::Restore,
            Some("charge") => Self::Charge,
            None | Some(_) => Self::Refund,
        }
    }

    /// Whether the charge is given back at all.
    #[must_use]
    pub const fn refunds(self) -> bool {
        !matches!(self, Self::NoRefund)
    }
}

/// How much a found Item gives back of one charge: what is still owed and, unless the
/// policy is `refund_unpaid`, what was paid on it (written off amounts are not paid).
///
/// Koha: `Koha::Item::_set_found_trigger` (`$credit_total`).
#[must_use]
pub fn refund(policy: LostReturn, amount: Money, outstanding: Money, paid: Money) -> Money {
    if !policy.refunds() {
        return Money(0);
    }
    if amount.0 > outstanding.0 && policy != LostReturn::RefundUnpaid {
        Money(outstanding.0 + paid.0)
    } else {
        outstanding
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_values_name_the_policy() {
        assert_eq!(LostReturn::from_rule(None), LostReturn::Refund);
        assert_eq!(LostReturn::from_rule(Some("0")), LostReturn::NoRefund);
        assert_eq!(LostReturn::from_rule(Some("")), LostReturn::NoRefund);
        assert_eq!(
            LostReturn::from_rule(Some("refund_unpaid")),
            LostReturn::RefundUnpaid
        );
        assert_eq!(LostReturn::from_rule(Some("restore")), LostReturn::Restore);
        assert_eq!(LostReturn::from_rule(Some("charge")), LostReturn::Charge);
        assert_eq!(LostReturn::from_rule(Some("refund")), LostReturn::Refund);
        assert_eq!(LostReturn::from_rule(Some("other")), LostReturn::Refund);
    }

    #[test]
    fn a_refund_gives_back_what_is_owed_and_what_was_paid() {
        let (amount, paid) = (Money(4500), Money(4500));
        // Paid in full.
        assert_eq!(
            refund(LostReturn::Refund, amount, Money(0), paid),
            Money(4500)
        );
        assert_eq!(
            refund(LostReturn::RefundUnpaid, amount, Money(0), paid),
            Money(0)
        );
        // Not paid at all.
        assert_eq!(
            refund(LostReturn::Refund, amount, amount, Money(0)),
            Money(4500)
        );
        assert_eq!(
            refund(LostReturn::RefundUnpaid, amount, amount, Money(0)),
            Money(4500)
        );
        // 1000 paid and 500 written off: the written off part is not given back.
        assert_eq!(
            refund(LostReturn::Restore, amount, Money(3000), Money(1000)),
            Money(4000)
        );
        assert_eq!(
            refund(LostReturn::NoRefund, amount, amount, Money(0)),
            Money(0)
        );
    }
}
