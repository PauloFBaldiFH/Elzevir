// SPDX-License-Identifier: GPL-3.0-or-later
//! What a self-check terminal reads beyond the circulation desk: an Item by its barcode,
//! the patron a hold is for, and whether a patron is blocked
//! (docs/specs/circulacao/sip2.md, items 20, 25 and 27).

use elzevir_domain::account::{DebtCheck, DebtSettings, check_debt};
use elzevir_domain::catalog::{BranchId, ItemId, Money};
use elzevir_domain::hold::HoldId;
use jiff::Zoned;
use jiff::civil::Date;

use crate::circulation_rules::CirculationDeskError;

/// An Item as a terminal sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent facts of the Item, which the SIP2 status ranks"
)]
pub struct SelfCheckItem {
    /// The Item.
    pub id: ItemId,
    /// Its barcode.
    pub barcode: String,
    /// The title of its Instance.
    pub title: String,
    /// The name of its home branch.
    pub home_branch: String,
    /// The name of the branch holding it.
    pub holding_branch: String,
    /// Its call number.
    pub call_number: Option<String>,
    /// When it is due, while on loan.
    pub due: Option<Zoned>,
    /// It is lost.
    pub lost: bool,
    /// It is damaged.
    pub damaged: bool,
    /// The "not for loan" value (negative: on order).
    pub not_for_loan: i64,
    /// It travels between branches.
    pub in_transit: bool,
    /// The holds of its Instance.
    pub holds: usize,
    /// It is set aside for a hold (waiting or in transit to the pickup branch).
    pub set_aside: bool,
    /// The last day its waiting hold waits.
    pub waiting_until: Option<Date>,
}

/// The patron a hold is for, as a returned Item's response names them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoldPatron {
    /// The card.
    pub cardnumber: String,
    /// The first name.
    pub first_name: Option<String>,
    /// The surname.
    pub surname: String,
    /// The e-mail address.
    pub email: Option<String>,
    /// The phone number.
    pub phone: Option<String>,
}

/// What a self-check terminal reads.
pub trait SelfCheck: Send + Sync {
    /// The Item with `barcode`.
    ///
    /// # Errors
    ///
    /// When storage fails.
    fn item(&self, barcode: &str) -> Result<Option<SelfCheckItem>, CirculationDeskError>;

    /// The patron of `hold`.
    ///
    /// # Errors
    ///
    /// When storage fails.
    fn hold_patron(&self, hold: HoldId) -> Result<Option<HoldPatron>, CirculationDeskError>;

    /// The name of `branch`.
    ///
    /// # Errors
    ///
    /// When storage fails.
    fn branch_name(&self, branch: BranchId) -> Result<Option<String>, CirculationDeskError>;
}

/// Why a patron may not borrow at a terminal, in the order Koha reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Standing {
    /// A restriction holds (Koha: `debarred`).
    pub suspended: bool,
    /// The enrolment ended before today.
    pub expired: bool,
    /// The debt is over the limit (Koha: `fine_blocked`).
    pub over_limit: bool,
}

impl Standing {
    /// A patron's standing: `restricted` when a restriction holds today, the last day of
    /// the enrolment and what they owe.
    ///
    /// Koha: `C4::SIP::ILS::Patron::new` (`charge_ok`, `debarred`, `expired`,
    /// `fine_blocked`).
    #[must_use]
    pub fn of(
        restricted: bool,
        expires: Date,
        today: Date,
        owed: Money,
        debt: &DebtSettings,
    ) -> Self {
        let strict = DebtSettings {
            allow_override: false,
            all_need_override: false,
            ..*debt
        };
        Self {
            suspended: restricted,
            expired: expires < today,
            over_limit: matches!(check_debt(owed, &strict), DebtCheck::Impossible(_)),
        }
    }

    /// Whether the patron may borrow, renew and place holds.
    #[must_use]
    pub const fn blocked(self) -> bool {
        self.suspended || self.expired || self.over_limit
    }
}

/// `money` as Perl writes a number: units, a point and the cents without trailing zeros
/// (`12.5`, `5`, `0`, `-1.25`).
#[must_use]
pub fn perl_number(money: Money) -> String {
    let sign = if money.0 < 0 { "-" } else { "" };
    let cents = money.0.unsigned_abs();
    let (units, rest) = (cents / 100, cents % 100);
    match rest {
        0 => format!("{sign}{units}"),
        rest if rest % 10 == 0 => format!("{sign}{units}.{}", rest / 10),
        rest => format!("{sign}{units}.{rest:02}"),
    }
}

/// `money` with two decimals (`12.50`), as Koha's `sprintf '%.02f'`.
#[must_use]
pub fn two_decimals(money: Money) -> String {
    let sign = if money.0 < 0 { "-" } else { "" };
    let cents = money.0.unsigned_abs();
    format!("{sign}{}.{:02}", cents / 100, cents % 100)
}

#[cfg(test)]
mod tests {
    use jiff::civil::date;

    use super::*;

    #[test]
    fn a_patron_is_blocked_by_a_restriction_an_old_enrolment_or_a_debt_over_the_limit() {
        let today = date(2026, 11, 26);
        let debt = DebtSettings::default();
        let clear = Standing::of(false, today, today, Money(500), &debt);
        assert!(!clear.blocked());
        assert!(Standing::of(false, date(2026, 11, 25), today, Money(0), &debt).expired);
        assert!(Standing::of(false, today, today, Money(501), &debt).over_limit);
        assert!(Standing::of(true, today, today, Money(0), &debt).blocked());
        let lenient = DebtSettings {
            allow_override: true,
            ..debt
        };
        assert!(Standing::of(false, today, today, Money(501), &lenient).over_limit);
    }

    #[test]
    fn money_is_written_as_perl_writes_numbers() {
        assert_eq!(perl_number(Money(1250)), "12.5");
        assert_eq!(perl_number(Money(500)), "5");
        assert_eq!(perl_number(Money(0)), "0");
        assert_eq!(perl_number(Money(-125)), "-1.25");
        assert_eq!(perl_number(Money(1205)), "12.05");
        assert_eq!(two_decimals(Money(1250)), "12.50");
        assert_eq!(two_decimals(Money(-5)), "-0.05");
    }
}
