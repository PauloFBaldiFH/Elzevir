// SPDX-License-Identifier: GPL-3.0-or-later
//! A patron's account: what they owe and how an overdue fine is charged
//! (docs/specs/circulacao/conta.md).

use elzevir_data::RuleKind;

use crate::catalog::Money;
use crate::checkout::perl_whole;
use crate::circulation_rules::{CirculationRule, RuleScopeKey, effective_rule};
use crate::due_date::LengthUnit;
use crate::fine::FineRule;

/// Why a patron owes something: the types Koha installs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum DebitType {
    /// A fee for creating the patron's account (`ACCOUNT`).
    Account,
    /// A fee for renewing the patron's account (`ACCOUNT_RENEW`).
    AccountRenew,
    /// A fee for an article request (`ARTICLE_REQUEST`).
    ArticleRequest,
    /// The replacement price of an Item lost while on loan (`LOST`).
    Lost,
    /// A charge the staff made by hand (`MANUAL`).
    Manual,
    /// A fee for a new card (`NEW_CARD`).
    NewCard,
    /// An overdue fine (`OVERDUE`).
    Overdue,
    /// Money the library paid out to the patron (`PAYOUT`).
    Payout,
    /// The fee for processing a lost Item (`PROCESSING`).
    Processing,
    /// A rental fee (`RENT`).
    Rent,
    /// A daily rental fee (`RENT_DAILY`).
    RentDaily,
    /// A daily rental fee on renewal (`RENT_DAILY_RENEW`).
    RentDailyRenew,
    /// A rental fee on renewal (`RENT_RENEW`).
    RentRenew,
    /// A hold fee (`RESERVE`).
    Reserve,
    /// A fee for a hold left waiting too long (`RESERVE_EXPIRED`).
    ReserveExpired,
    /// A credit that was voided (`VOID`).
    Void,
}

impl DebitType {
    /// Every type, in the order of the codes.
    pub const ALL: [Self; 16] = [
        Self::Account,
        Self::AccountRenew,
        Self::ArticleRequest,
        Self::Lost,
        Self::Manual,
        Self::NewCard,
        Self::Overdue,
        Self::Payout,
        Self::Processing,
        Self::Rent,
        Self::RentDaily,
        Self::RentDailyRenew,
        Self::RentRenew,
        Self::Reserve,
        Self::ReserveExpired,
        Self::Void,
    ];

    /// The code stored in the account, Koha's.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Account => "ACCOUNT",
            Self::AccountRenew => "ACCOUNT_RENEW",
            Self::ArticleRequest => "ARTICLE_REQUEST",
            Self::Lost => "LOST",
            Self::Manual => "MANUAL",
            Self::NewCard => "NEW_CARD",
            Self::Overdue => "OVERDUE",
            Self::Payout => "PAYOUT",
            Self::Processing => "PROCESSING",
            Self::Rent => "RENT",
            Self::RentDaily => "RENT_DAILY",
            Self::RentDailyRenew => "RENT_DAILY_RENEW",
            Self::RentRenew => "RENT_RENEW",
            Self::Reserve => "RESERVE",
            Self::ReserveExpired => "RESERVE_EXPIRED",
            Self::Void => "VOID",
        }
    }

    /// The type of a stored code.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.code() == code)
    }
}

/// Why a patron owes less: the types Koha installs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum CreditType {
    /// A charge cancelled (`CANCELLATION`).
    Cancellation,
    /// A credit the staff gave by hand (`CREDIT`).
    Credit,
    /// A discount on a charge (`DISCOUNT`).
    Discount,
    /// A charge forgiven (`FORGIVEN`).
    Forgiven,
    /// The refund of a lost Item found again (`LOST_FOUND`).
    LostFound,
    /// The refund of an overpayment (`OVERPAYMENT`).
    Overpayment,
    /// The patron paid (`PAYMENT`).
    Payment,
    /// The refund of the processing fee of a lost Item found again (`PROCESSING_FOUND`).
    ProcessingFound,
    /// A purchase (`PURCHASE`).
    Purchase,
    /// A refund (`REFUND`).
    Refund,
    /// The library let the debt go (`WRITEOFF`).
    Writeoff,
}

impl CreditType {
    /// Every type, in the order of the codes.
    pub const ALL: [Self; 11] = [
        Self::Cancellation,
        Self::Credit,
        Self::Discount,
        Self::Forgiven,
        Self::LostFound,
        Self::Overpayment,
        Self::Payment,
        Self::ProcessingFound,
        Self::Purchase,
        Self::Refund,
        Self::Writeoff,
    ];

    /// The code stored in the account, Koha's.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Cancellation => "CANCELLATION",
            Self::Credit => "CREDIT",
            Self::Discount => "DISCOUNT",
            Self::Forgiven => "FORGIVEN",
            Self::LostFound => "LOST_FOUND",
            Self::Overpayment => "OVERPAYMENT",
            Self::Payment => "PAYMENT",
            Self::ProcessingFound => "PROCESSING_FOUND",
            Self::Purchase => "PURCHASE",
            Self::Refund => "REFUND",
            Self::Writeoff => "WRITEOFF",
        }
    }

    /// The type of a stored code.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.code() == code)
    }
}

/// What the staff does at the desk with money owed (conta.md, item 10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum PaymentKind {
    /// The patron pays.
    Payment,
    /// The library lets the debt go.
    Writeoff,
}

impl PaymentKind {
    /// The credit it leaves in the account.
    #[must_use]
    pub const fn credit(self) -> CreditType {
        match self {
            Self::Payment => CreditType::Payment,
            Self::Writeoff => CreditType::Writeoff,
        }
    }
}

/// How much of a credit goes to each debit, in the order given, and what is left of it.
///
/// Each debit takes what it still owes, while the credit lasts; the debits are
/// `(id, outstanding)` and only those that take something come back.
///
/// Koha: `Koha::Account::Line::apply`
#[must_use]
pub fn apply_credit(credit: Money, debits: &[(i64, Money)]) -> (Vec<(i64, Money)>, Money) {
    let mut left = credit.0;
    let mut applied = Vec::new();
    for &(id, Money(owed)) in debits {
        if left <= 0 {
            break;
        }
        let cancelled = owed.min(left);
        if cancelled > 0 {
            applied.push((id, Money(cancelled)));
            left -= cancelled;
        }
    }
    (applied, Money(left))
}

/// Why a payment or a write-off cannot happen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PaymentRefusal {
    /// Nothing, or less than nothing.
    #[error("the amount must be more than zero")]
    NotPositive,
    /// More than what is owed (Koha: `error_over`).
    #[error("the amount is more than the {due:?} owed")]
    OverDue {
        /// What is owed in the debits chosen, or in all of them.
        due: Money,
    },
}

/// Whether `amount` may be paid or written off against `due`.
///
/// # Errors
///
/// [`PaymentRefusal`] when it is not positive or more than is due.
///
/// Koha: `members/paycollect.pl`.
pub const fn check_payment(amount: Money, due: Money) -> Result<(), PaymentRefusal> {
    if amount.0 <= 0 {
        Err(PaymentRefusal::NotPositive)
    } else if amount.0 > due.0 {
        Err(PaymentRefusal::OverDue { due })
    } else {
        Ok(())
    }
}

/// Where an overdue fine stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OverdueStatus {
    /// The Item is still on loan, so the fine may still grow (`UNRETURNED`).
    Unreturned,
    /// The Item came back; the fine no longer changes (`RETURNED`).
    Returned,
    /// The Item was declared lost; the fine no longer changes (`LOST`).
    Lost,
}

impl OverdueStatus {
    /// The code stored in the account, Koha's.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Unreturned => "UNRETURNED",
            Self::Returned => "RETURNED",
            Self::Lost => "LOST",
        }
    }
}

/// What charging an overdue fine does to the account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FineUpdate {
    /// Nothing changes.
    Nothing,
    /// A new fine of this amount.
    Charge(Money),
    /// The loan's fine becomes this amount.
    Adjust(Money),
}

/// How the loan's fine changes when it is worth `amount`.
///
/// `charged` is the loan's fine still growing, if any; `overdue_outstanding` is what the
/// patron still owes in overdue fines, that one included; `max_fine` is `MaxFine`. A new
/// fine of zero is not charged, and the loan's fine only changes when the cents change.
///
/// Koha: `C4::Overdues::UpdateFine`
#[must_use]
pub fn update_fine(
    amount: Money,
    charged: Option<Money>,
    overdue_outstanding: Money,
    max_fine: Option<Money>,
) -> FineUpdate {
    let mut amount = amount.0;
    if let Some(Money(max)) = max_fine.filter(|max| max.0 != 0) {
        let max_increase = max - overdue_outstanding.0;
        if max_increase <= 0 {
            return FineUpdate::Nothing;
        }
        match charged {
            Some(Money(before)) if amount - before > max_increase => amount = before + max_increase,
            None if amount > max_increase => amount = max_increase,
            _ => {}
        }
    }
    match charged {
        Some(Money(before)) if before == amount => FineUpdate::Nothing,
        Some(_) => FineUpdate::Adjust(Money(amount)),
        None if amount == 0 => FineUpdate::Nothing,
        None => FineUpdate::Charge(Money(amount)),
    }
}

/// What a fine still owes after its amount changes from `before` to `after`.
///
/// What was paid stays paid; a fine that goes below what was paid owes nothing.
///
/// Koha: `Koha::Account::Line::adjust`
#[must_use]
pub fn adjusted_outstanding(before: Money, outstanding: Money, after: Money) -> Money {
    Money((outstanding.0 + after.0 - before.0).max(0))
}

/// The fine part of the rules that apply at `at`, for loans counted in `unit`.
///
/// Missing whole numbers are 0 and missing or empty amounts are none, as Koha reads them.
///
/// Koha: `C4::Overdues::CalcFine` (the rules it reads).
#[must_use]
pub fn fine_rule(rules: &[CirculationRule], at: &RuleScopeKey, unit: LengthUnit) -> FineRule {
    let value = |kind| effective_rule(rules, kind, at).map(|rule| rule.value.as_str());
    let whole = |kind| value(kind).map_or(0, perl_whole);
    let amount = |kind| value(kind).and_then(rule_money);
    FineRule {
        unit,
        grace: whole(RuleKind::Firstremind),
        charge_period: whole(RuleKind::Chargeperiod),
        charge_at_start: whole(RuleKind::ChargeperiodChargeAt) == 1,
        fine: amount(RuleKind::Fine),
        cap: amount(RuleKind::Overduefinescap),
        cap_to_replacement_price: whole(RuleKind::CapFineToReplacementPrice) == 1,
        recall_fine: amount(RuleKind::RecallOverdueFine),
    }
}

/// An amount written in a rule (`0.50`, `1`, `2.5`), in cents; `None` when empty.
///
/// Like Perl, the leading number counts and the rest is ignored; past the cents, the amount
/// is rounded half away from zero, as Koha shows prices.
#[must_use]
pub fn rule_money(value: &str) -> Option<Money> {
    let text = value.trim();
    if text.is_empty() {
        return None;
    }
    let (negative, text) = text
        .strip_prefix('-')
        .map_or((false, text), |rest| (true, rest));
    let digits = |part: &str| {
        part.find(|c: char| !c.is_ascii_digit())
            .map_or(part.len(), |end| end)
    };
    let whole_end = digits(text);
    let whole: i64 = text.get(..whole_end)?.parse().unwrap_or(0);
    let fraction = text
        .get(whole_end..)
        .and_then(|rest| rest.strip_prefix('.'))
        .map_or("", |rest| rest.get(..digits(rest)).unwrap_or(""));
    let mut tenths = fraction.bytes().map(|b| i64::from(b - b'0'));
    let cents = tenths.next().unwrap_or(0) * 10 + tenths.next().unwrap_or(0);
    let round_up = i64::from(tenths.next().unwrap_or(0) >= 5);
    let cents = whole.saturating_mul(100).saturating_add(cents + round_up);
    Some(Money(if negative { -cents } else { cents }))
}

/// The library's settings for debts, with Koha's defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DebtSettings {
    /// `noissuescharge` (Koha: 5): above it the patron cannot borrow; zero or none never
    /// blocks.
    pub limit: Option<Money>,
    /// `AllowFineOverride` (Koha: off): the staff may confirm past the limit.
    pub allow_override: bool,
    /// `AllFinesNeedOverride` (Koha: on): any debt asks the staff to confirm.
    pub all_need_override: bool,
    /// `MaxFine` (Koha: none): the most a patron owes in overdue fines.
    pub max_fine: Option<Money>,
}

impl Default for DebtSettings {
    fn default() -> Self {
        Self {
            limit: Some(Money(500)),
            allow_override: false,
            all_need_override: true,
            max_fine: None,
        }
    }
}

/// What a patron's debt does to a checkout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebtCheck {
    /// Nothing.
    Clear,
    /// The staff must confirm (`DEBT` in `needsconfirmation`).
    Confirm(Money),
    /// The checkout cannot happen (`DEBT` in `issuingimpossible`).
    Impossible(Money),
}

/// What the patron's debt, the outstanding debits, does to a checkout.
///
/// Koha: `C4::Circulation::CanBookBeIssued` (`DEBT`, with `IssuingInProcess` off) and
/// `Koha::Patron::is_patron_inside_charge_limits`.
#[must_use]
pub fn check_debt(owed: Money, settings: &DebtSettings) -> DebtCheck {
    let over = settings
        .limit
        .is_some_and(|limit| limit.0 != 0 && owed > limit);
    if over && settings.allow_override {
        DebtCheck::Confirm(owed)
    } else if over {
        DebtCheck::Impossible(owed)
    } else if owed.0 > 0 && settings.all_need_override {
        DebtCheck::Confirm(owed)
    } else {
        DebtCheck::Clear
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_credit_pays_the_debits_in_order_while_it_lasts() {
        let debits = [
            (1, Money(200)),
            (2, Money(0)),
            (3, Money(600)),
            (4, Money(100)),
        ];
        assert_eq!(
            apply_credit(Money(500), &debits),
            (vec![(1, Money(200)), (3, Money(300))], Money(0))
        );
        assert_eq!(
            apply_credit(Money(1000), &debits),
            (
                vec![(1, Money(200)), (3, Money(600)), (4, Money(100))],
                Money(100)
            )
        );
        assert_eq!(apply_credit(Money(50), &[]), (vec![], Money(50)));
    }

    #[test]
    fn a_payment_is_positive_and_no_more_than_is_due() {
        assert_eq!(
            check_payment(Money(0), Money(100)),
            Err(PaymentRefusal::NotPositive)
        );
        assert_eq!(
            check_payment(Money(101), Money(100)),
            Err(PaymentRefusal::OverDue { due: Money(100) })
        );
        assert_eq!(check_payment(Money(100), Money(100)), Ok(()));
    }

    #[test]
    fn rule_amounts_read_like_perl_numbers() {
        assert_eq!(rule_money(""), None);
        assert_eq!(rule_money("1"), Some(Money(100)));
        assert_eq!(rule_money("0.50"), Some(Money(50)));
        assert_eq!(rule_money("2.5"), Some(Money(250)));
        assert_eq!(rule_money("0.005"), Some(Money(1)));
        assert_eq!(rule_money("0.004"), Some(Money(0)));
        assert_eq!(rule_money("3 reais"), Some(Money(300)));
        assert_eq!(rule_money("abc"), Some(Money(0)));
        assert_eq!(rule_money("-1.25"), Some(Money(-125)));
    }

    // Koha's t/db_dependent/Overdues.t, "UpdateFine tests".
    #[test]
    fn a_fine_is_charged_once_and_then_adjusted() {
        assert_eq!(
            update_fine(Money(0), None, Money(0), None),
            FineUpdate::Nothing
        );
        assert_eq!(
            update_fine(Money(50), None, Money(0), None),
            FineUpdate::Charge(Money(50))
        );
        assert_eq!(
            update_fine(Money(50), Some(Money(50)), Money(50), None),
            FineUpdate::Nothing
        );
        assert_eq!(
            update_fine(Money(80), Some(Money(50)), Money(50), None),
            FineUpdate::Adjust(Money(80))
        );
        assert_eq!(
            update_fine(Money(0), Some(Money(50)), Money(50), None),
            FineUpdate::Adjust(Money(0))
        );
    }

    // Koha's t/db_dependent/Overdues.t, "UpdateFine tests", with `MaxFine` 100.
    #[test]
    fn max_fine_follows_kohas_sequence() {
        let max = Some(Money(10_000));
        let fine = |amount, charged, owed| update_fine(Money(amount), charged, Money(owed), max);
        assert_eq!(fine(5000, None, 0), FineUpdate::Charge(Money(5000)));
        assert_eq!(
            fine(8000, Some(Money(5000)), 5000),
            FineUpdate::Adjust(Money(8000))
        );
        // A second Item: only 20 are left under the cap.
        assert_eq!(fine(3000, None, 8000), FineUpdate::Charge(Money(2000)));
        assert_eq!(fine(4000, Some(Money(2000)), 10_000), FineUpdate::Nothing);
        // 30 paid on the first fine: the second may grow again.
        assert_eq!(
            fine(3000, Some(Money(2000)), 7000),
            FineUpdate::Adjust(Money(3000))
        );
        // The first Item, renewed and late again, gets a new fine capped at 20.
        assert_eq!(fine(3000, None, 8000), FineUpdate::Charge(Money(2000)));
    }

    #[test]
    fn max_fine_caps_what_the_patron_owes_in_fines() {
        let max = Some(Money(300));
        // 2.00 owed elsewhere: a new fine of 5.00 becomes 1.00.
        assert_eq!(
            update_fine(Money(500), None, Money(200), max),
            FineUpdate::Charge(Money(100))
        );
        // At the cap nothing changes, not even a decrease.
        assert_eq!(
            update_fine(Money(0), Some(Money(100)), Money(300), max),
            FineUpdate::Nothing
        );
        // The loan's 1.00 counts in what is owed: it grows by the 1.00 left.
        assert_eq!(
            update_fine(Money(500), Some(Money(100)), Money(200), max),
            FineUpdate::Adjust(Money(200))
        );
        assert_eq!(
            update_fine(Money(500), None, Money(0), Some(Money(0))),
            FineUpdate::Charge(Money(500))
        );
    }

    #[test]
    fn an_adjusted_fine_keeps_what_was_paid() {
        assert_eq!(
            adjusted_outstanding(Money(100), Money(100), Money(300)),
            Money(300)
        );
        assert_eq!(
            adjusted_outstanding(Money(300), Money(100), Money(400)),
            Money(200)
        );
        assert_eq!(
            adjusted_outstanding(Money(300), Money(100), Money(100)),
            Money(0)
        );
    }

    #[test]
    fn debts_block_or_ask_like_koha() {
        let settings = DebtSettings::default();
        assert_eq!(check_debt(Money(0), &settings), DebtCheck::Clear);
        assert_eq!(
            check_debt(Money(500), &settings),
            DebtCheck::Confirm(Money(500))
        );
        assert_eq!(
            check_debt(Money(501), &settings),
            DebtCheck::Impossible(Money(501))
        );
        let lenient = DebtSettings {
            allow_override: true,
            all_need_override: false,
            ..settings
        };
        assert_eq!(check_debt(Money(300), &lenient), DebtCheck::Clear);
        assert_eq!(
            check_debt(Money(501), &lenient),
            DebtCheck::Confirm(Money(501))
        );
        let no_limit = DebtSettings {
            limit: Some(Money(0)),
            all_need_override: false,
            ..settings
        };
        assert_eq!(check_debt(Money(10_000), &no_limit), DebtCheck::Clear);
    }

    #[test]
    fn the_fine_rule_reads_the_rules_like_koha() {
        let at = RuleScopeKey {
            branch: None,
            category: None,
            policy: None,
        };
        let rule = |kind, value: &str| CirculationRule {
            kind,
            scope: at,
            value: value.to_owned(),
        };
        let rules = [
            rule(RuleKind::Fine, "0.25"),
            rule(RuleKind::Chargeperiod, "1"),
            rule(RuleKind::Firstremind, "2"),
            rule(RuleKind::Overduefinescap, ""),
        ];
        let read = fine_rule(&rules, &at, LengthUnit::Days);
        assert_eq!(read.fine, Some(Money(25)));
        assert_eq!((read.charge_period, read.grace), (1, 2));
        assert_eq!(read.cap, None);
        assert!(!read.charge_at_start && !read.cap_to_replacement_price);
        assert_eq!(fine_rule(&[], &at, LengthUnit::Days).fine, None);
    }
}
