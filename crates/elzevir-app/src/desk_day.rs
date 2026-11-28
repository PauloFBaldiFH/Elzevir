// SPDX-License-Identifier: GPL-3.0-or-later
//! A staged desk day on Koha and on the Elzevir: the plan, Koha's answers, each answer in
//! Koha's words and the comparison (docs/specs/migracao/desk-day.md).

use elzevir_domain::catalog::{BranchId, Money};
use elzevir_domain::checkout::Confirm;
use elzevir_domain::patron::PatronId;
use jiff::Zoned;
use serde::{Deserialize, Serialize};

use crate::holds::HoldRefused;
use crate::koha_parity::renewal_answer;
use crate::loans::{CheckoutRefusal, RenewRefusal};

/// The most steps a day may have (item 1).
pub const MAX_STEPS: usize = 10_000;

/// The format of Koha's minutes, of `at` and of due dates.
const KOHA_MINUTE: &str = "%Y-%m-%d %H:%M";

/// A record hold or an item hold.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepLevel {
    /// Any Item of the record.
    #[default]
    Record,
    /// The Item named.
    Item,
}

/// One action at the desk, with Koha's numbers (item 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Step {
    /// Lend the Item to the patron.
    Checkout {
        /// The patron.
        borrowernumber: u64,
        /// The Item.
        itemnumber: u64,
        /// The staff confirm what the check asks to confirm.
        #[serde(default)]
        confirm: bool,
    },
    /// Return the Item.
    Checkin {
        /// The Item.
        itemnumber: u64,
        /// The patron whose account to read when the Item is not on loan, as a lost Item
        /// found at the checkin (lost-items.md, item 19).
        #[serde(default)]
        borrowernumber: Option<u64>,
    },
    /// Mark the Item lost from its page (lost-items.md, item 3).
    Lose {
        /// The Item.
        itemnumber: u64,
        /// The lost number.
        #[serde(default = "first_lost")]
        lost: i64,
    },
    /// Renew the Item's loan.
    Renew {
        /// The Item.
        itemnumber: u64,
    },
    /// Place a hold for the patron.
    Hold {
        /// The patron.
        borrowernumber: u64,
        /// The Item, or one Item of the record.
        itemnumber: u64,
        /// Record or item hold.
        #[serde(default)]
        level: StepLevel,
    },
    /// The patron pays at the desk.
    Pay {
        /// The patron.
        borrowernumber: u64,
        /// How much, in cents.
        amount_cents: i64,
    },
}

impl Step {
    /// The step's kind, as the plan names it.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Checkout { .. } => "checkout",
            Self::Checkin { .. } => "checkin",
            Self::Lose { .. } => "lose",
            Self::Renew { .. } => "renew",
            Self::Hold { .. } => "hold",
            Self::Pay { .. } => "pay",
        }
    }
}

/// The lost number a `lose` step sets when it names none.
const fn first_lost() -> i64 {
    1
}

/// A step as Koha did it (item 6).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct KohaStep {
    /// What was asked.
    #[serde(flatten)]
    pub step: Step,
    /// Koha's clock, `YYYY-MM-DD HH:MM`.
    pub at: String,
    /// What it gave, in Koha's words.
    pub answer: String,
    /// What the step's patron owed after it, in cents; none when there is no patron.
    pub balance_cents: Option<i64>,
    /// The patron's account lines after it, oldest first, as [`account_line`] writes them;
    /// none in a day recorded before they were (item 6).
    #[serde(default)]
    pub lines: Option<Vec<String>>,
}

/// The output of `tools/koha-parity/desk_day.pl` (item 6).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct KohaDay {
    /// Koha's version.
    pub koha_version: Option<String>,
    /// Each step, in order.
    pub steps: Vec<KohaStep>,
}

/// An Item of a step, found by its `itemnumber` (item 8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeskItem {
    /// Its barcode, which the desk commands take.
    pub barcode: String,
    /// The branch where it is: the desk of a checkout, checkin or renewal.
    pub holding: BranchId,
    /// Who has it on loan.
    pub borrower: Option<PatronId>,
    /// Its lost number, 0 when it is not lost.
    pub lost: i64,
}

/// A step as the Elzevir did it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepAnswer {
    /// What it gave, in Koha's words.
    pub answer: String,
    /// What the step's patron owed after it.
    pub balance: Option<Money>,
    /// The patron's account lines after it, oldest first.
    pub lines: Option<Vec<String>>,
}

/// A step that differs (item 10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayDivergence {
    /// Its position in the plan, from 1.
    pub position: usize,
    /// The kind of step.
    pub op: &'static str,
    /// Koha's answer and balance.
    pub koha: String,
    /// The Elzevir's answer and balance, or why it could not be done.
    pub elzevir: String,
}

/// The comparison of a day (item 10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayCheck {
    /// How many steps.
    pub steps: usize,
    /// How many gave the same answer and balance.
    pub same: usize,
    /// The others.
    pub divergences: Vec<DayDivergence>,
}

impl DayCheck {
    /// Every step gave the same.
    #[must_use]
    pub const fn agrees(&self) -> bool {
        self.divergences.is_empty() && self.same == self.steps
    }
}

/// An account line as the day compares it: the debit or credit type, the status, the
/// amount and what is still owed, in cents, credits negative (item 6).
#[must_use]
pub fn account_line(kind: &str, status: Option<&str>, amount: i64, outstanding: i64) -> String {
    format!("{kind} {} {amount} {outstanding}", status.unwrap_or("-"))
}

/// An answer, a balance and the account lines as the report shows them.
fn shown(answer: &str, balance_cents: Option<i64>, lines: Option<&[String]>) -> String {
    let mut text = match balance_cents {
        Some(cents) => format!("{answer} (owes {cents})"),
        None => answer.to_owned(),
    };
    if let Some(lines) = lines {
        text.push_str(" [");
        text.push_str(&lines.join("; "));
        text.push(']');
    }
    text
}

/// Compares Koha's day with the Elzevir's, step by step (item 10). `elzevir` holds one
/// entry per Koha step: the answer, or why the step could not be done. The account lines
/// are compared when Koha's day has them.
#[must_use]
pub fn compare(koha: &KohaDay, elzevir: &[Result<StepAnswer, String>]) -> DayCheck {
    let mut check = DayCheck {
        steps: koha.steps.len(),
        same: 0,
        divergences: Vec::new(),
    };
    for (position, step) in koha.steps.iter().enumerate() {
        let koha_lines = step.lines.as_deref();
        let koha_side = shown(&step.answer, step.balance_cents, koha_lines);
        let elzevir_side = match elzevir.get(position) {
            Some(Ok(answer)) => {
                let balance = answer.balance.map(|owed| owed.0);
                let lines = koha_lines.and(answer.lines.as_deref());
                if answer.answer == step.answer
                    && balance == step.balance_cents
                    && lines == koha_lines
                {
                    check.same += 1;
                    continue;
                }
                shown(&answer.answer, balance, lines)
            }
            Some(Err(gap)) => gap.clone(),
            None => "not done".to_owned(),
        };
        check.divergences.push(DayDivergence {
            position: position + 1,
            op: step.step.name(),
            koha: koha_side,
            elzevir: elzevir_side,
        });
    }
    check
}

/// A moment as Koha writes it.
fn minute(at: &Zoned) -> String {
    at.strftime(KOHA_MINUTE).to_string()
}

/// A checkout in Koha's words: `ok due …`, or the sorted keys of `CanBookBeIssued`
/// (item 9).
///
/// Koha: `C4::Circulation::CanBookBeIssued`.
#[must_use]
pub fn checkout_answer(result: Result<&Zoned, &CheckoutRefusal>) -> String {
    let refusal = match result {
        Ok(due) => return format!("ok due {}", minute(due)),
        Err(refusal) => refusal,
    };
    match refusal {
        CheckoutRefusal::Impossible(reasons) => keys(
            "impossible",
            reasons
                .iter()
                .flat_map(|reason| reason.koha_keys().iter().copied()),
        ),
        CheckoutRefusal::NeedsConfirmation(confirms) => {
            keys("confirm", confirms.iter().map(Confirm::koha_key))
        }
        CheckoutRefusal::UnknownBarcode => "impossible UNKNOWN_BARCODE".to_owned(),
        CheckoutRefusal::UnknownPatron => "unknown".to_owned(),
        CheckoutRefusal::OnLoan => "on_loan".to_owned(),
        CheckoutRefusal::NoDueDate => "no_due_date".to_owned(),
    }
}

/// `what` and the keys, sorted and each once, as the recorder writes them.
fn keys<'a>(what: &str, keys: impl Iterator<Item = &'a str>) -> String {
    let mut keys: Vec<&str> = keys.collect();
    keys.sort_unstable();
    keys.dedup();
    format!("{what} {}", keys.join(","))
}

/// A renewal in Koha's words: `ok due …`, or the reason of `CanBookBeRenewed` (item 9).
#[must_use]
pub fn renew_answer(result: Result<&Zoned, &RenewRefusal>) -> String {
    match result {
        Ok(due) => format!("ok due {}", minute(due)),
        Err(RenewRefusal::Refused(refusal)) => renewal_answer(Err(refusal)).to_owned(),
        Err(RenewRefusal::NotOnLoan) => "no_checkout".to_owned(),
        Err(RenewRefusal::Restricted) => "restriction".to_owned(),
        Err(RenewRefusal::NoDueDate) => "no_due_date".to_owned(),
        Err(RenewRefusal::UnknownBarcode) => "unknown".to_owned(),
    }
}

/// A hold in Koha's words: `ok priority N`, or the status of `CanBookBeReserved` and
/// `CanItemBeReserved` (item 9).
#[must_use]
pub fn hold_placed_answer(result: Result<u32, &HoldRefused>) -> String {
    match result {
        Ok(priority) => format!("ok priority {priority}"),
        Err(HoldRefused::Rules(refusal)) => refusal.code().to_owned(),
        Err(_) => "unknown".to_owned(),
    }
}

/// A checkin in Koha's words: whether the loan ended, and the hold the Item answers
/// (item 9).
#[must_use]
pub fn checkin_answer(returned: &str, hold: &str) -> String {
    format!("{returned}; {hold}")
}

#[cfg(test)]
mod tests {
    use elzevir_domain::checkout::{HeldFor, Impossible};
    use elzevir_domain::hold::HoldRefusal;
    use elzevir_domain::renewal::RenewalRefusal;
    use elzevir_domain::suspension::SuspensionEnd;

    use super::*;

    fn day(text: &str) -> KohaDay {
        serde_json::from_str(text).unwrap()
    }

    const DAY: &str = r#"{
        "koha_version": "26.0505000",
        "steps": [
            {"op": "checkout", "borrowernumber": 2, "itemnumber": 7,
             "at": "2026-11-27 10:00", "answer": "ok due 2026-12-11 23:59", "balance_cents": 0},
            {"op": "pay", "borrowernumber": 2, "amount_cents": 150,
             "at": "2026-11-27 10:01", "answer": "ok", "balance_cents": 50},
            {"op": "checkin", "itemnumber": 7,
             "at": "2026-11-27 10:02", "answer": "ok; none", "balance_cents": 50}
        ]
    }"#;

    fn same(answer: &str, owed: i64) -> StepAnswer {
        StepAnswer {
            answer: answer.to_owned(),
            balance: Some(Money(owed)),
            lines: None,
        }
    }

    #[test]
    fn the_plan_and_koha_answers_are_read() {
        let day = day(DAY);
        assert_eq!(
            day.steps[0].step,
            Step::Checkout {
                borrowernumber: 2,
                itemnumber: 7,
                confirm: false
            }
        );
        assert_eq!(
            day.steps[1].step,
            Step::Pay {
                borrowernumber: 2,
                amount_cents: 150
            }
        );
        let hold: Step =
            serde_json::from_str(r#"{"op": "hold", "borrowernumber": 3, "itemnumber": 8}"#)
                .unwrap();
        assert_eq!(
            hold,
            Step::Hold {
                borrowernumber: 3,
                itemnumber: 8,
                level: StepLevel::Record
            }
        );
        assert!(serde_json::from_str::<Step>(r#"{"op": "lend", "itemnumber": 8}"#).is_err());
        let lose: Step = serde_json::from_str(r#"{"op": "lose", "itemnumber": 8}"#).unwrap();
        assert_eq!(
            lose,
            Step::Lose {
                itemnumber: 8,
                lost: 1
            }
        );
        let found: Step =
            serde_json::from_str(r#"{"op": "checkin", "itemnumber": 8, "borrowernumber": 3}"#)
                .unwrap();
        assert_eq!(
            found,
            Step::Checkin {
                itemnumber: 8,
                borrowernumber: Some(3)
            }
        );
    }

    #[test]
    fn account_lines_are_compared_when_koha_wrote_them() {
        let koha = day(r#"{"steps": [
            {"op": "lose", "itemnumber": 7, "at": "2026-11-27 10:00", "answer": "ok",
             "balance_cents": 4800,
             "lines": ["PROCESSING - 300 300", "LOST - 4500 4500"]}
        ]}"#);
        let lines = |list: &[&str]| Some(list.iter().map(|line| (*line).to_owned()).collect());
        let answer = |list: &[&str]| StepAnswer {
            answer: "ok".to_owned(),
            balance: Some(Money(4800)),
            lines: lines(list),
        };
        let agreed = [Ok(answer(&["PROCESSING - 300 300", "LOST - 4500 4500"]))];
        assert!(compare(&koha, &agreed).agrees());
        let check = compare(&koha, &[Ok(answer(&["LOST - 4500 4500"]))]);
        assert_eq!(
            check.divergences[0].elzevir,
            "ok (owes 4800) [LOST - 4500 4500]"
        );
        assert_eq!(
            account_line("LOST_FOUND", None, -4500, 0),
            "LOST_FOUND - -4500 0"
        );
        // A day recorded without lines compares answers and balances only.
        let older = day(DAY);
        let with_lines = [Ok(answer(&["x"]))];
        assert_eq!(
            compare(&older, &with_lines).divergences[0].elzevir,
            "ok (owes 4800)"
        );
    }

    #[test]
    fn the_same_day_agrees() {
        let elzevir = [
            Ok(same("ok due 2026-12-11 23:59", 0)),
            Ok(same("ok", 50)),
            Ok(same("ok; none", 50)),
        ];
        let check = compare(&day(DAY), &elzevir);
        assert!(check.agrees(), "{check:?}");
        assert_eq!((check.steps, check.same), (3, 3));
    }

    #[test]
    fn a_different_answer_balance_or_missing_step_is_a_divergence() {
        let elzevir = [Ok(same("confirm DEBT", 0)), Ok(same("ok", 0))];
        let check = compare(&day(DAY), &elzevir);
        assert!(!check.agrees());
        assert_eq!(check.same, 0);
        assert_eq!(
            check.divergences,
            vec![
                DayDivergence {
                    position: 1,
                    op: "checkout",
                    koha: "ok due 2026-12-11 23:59 (owes 0)".to_owned(),
                    elzevir: "confirm DEBT (owes 0)".to_owned(),
                },
                DayDivergence {
                    position: 2,
                    op: "pay",
                    koha: "ok (owes 50)".to_owned(),
                    elzevir: "ok (owes 0)".to_owned(),
                },
                DayDivergence {
                    position: 3,
                    op: "checkin",
                    koha: "ok; none (owes 50)".to_owned(),
                    elzevir: "not done".to_owned(),
                },
            ]
        );
        let gap = compare(&day(DAY), &[Err("the Item did not come".to_owned())]);
        assert_eq!(gap.divergences[0].elzevir, "the Item did not come");
    }

    #[test]
    fn refusals_are_named_with_koha_keys() {
        let due: Zoned = "2026-12-11T23:59:00[America/Sao_Paulo]".parse().unwrap();
        assert_eq!(checkout_answer(Ok(&due)), "ok due 2026-12-11 23:59");
        let suspended = CheckoutRefusal::Impossible(vec![
            Impossible::PatronExpired,
            Impossible::Suspended(SuspensionEnd::WhenLifted),
        ]);
        assert_eq!(
            checkout_answer(Err(&suspended)),
            "impossible DEBARRED,EXPIRED,USERBLOCKEDNOENDDATE"
        );
        let confirm = CheckoutRefusal::NeedsConfirmation(vec![
            Confirm::Held(HeldFor::Waiting),
            Confirm::Debt(Money(700)),
            Confirm::Debt(Money(700)),
        ]);
        assert_eq!(
            checkout_answer(Err(&confirm)),
            "confirm DEBT,RESERVE_WAITING"
        );
        assert_eq!(
            checkout_answer(Err(&CheckoutRefusal::UnknownBarcode)),
            "impossible UNKNOWN_BARCODE"
        );
        assert_eq!(renew_answer(Ok(&due)), "ok due 2026-12-11 23:59");
        assert_eq!(
            renew_answer(Err(&RenewRefusal::Refused(RenewalRefusal::TooMany))),
            "too_many"
        );
        assert_eq!(renew_answer(Err(&RenewRefusal::NotOnLoan)), "no_checkout");
        assert_eq!(hold_placed_answer(Ok(2)), "ok priority 2");
        assert_eq!(
            hold_placed_answer(Err(&HoldRefused::Rules(HoldRefusal::NotReservable))),
            "notReservable"
        );
        assert_eq!(
            hold_placed_answer(Err(&HoldRefused::UnknownBarcode)),
            "unknown"
        );
        assert_eq!(
            checkin_answer("ok", "Reserved borrowernumber 3"),
            "ok; Reserved borrowernumber 3"
        );
    }
}
