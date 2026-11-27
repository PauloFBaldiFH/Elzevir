// SPDX-License-Identifier: GPL-3.0-or-later
//! Koha's own answers on a copy of the library against the Elzevir's on the same copy
//! after the switch (docs/specs/migracao/koha-parity.md). Pure: the answers come from the
//! recorder and from the store, and this module only compares them.

use std::collections::BTreeMap;

use elzevir_domain::renewal::RenewalRefusal;
use serde::Deserialize;

/// What the recorder (`tools/koha-parity/record.pl`) wrote.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct KohaAnswers {
    /// Koha's `Version` preference.
    #[serde(default)]
    pub koha_version: Option<String>,
    /// The moment of the answers, `YYYY-MM-DD HH:MM` in the library's time.
    pub now: String,
    /// Koha's clock when it asked about renewals, `YYYY-MM-DD HH:MM`; a file of part 1
    /// has none, and `now` stands for it.
    #[serde(default)]
    pub renewals_at: Option<String>,
    /// The due date of each current loan, made again at its `issuedate`.
    #[serde(default)]
    pub due_dates: Vec<KohaDue>,
    /// The fine of each overdue loan at `now`.
    #[serde(default)]
    pub fines: Vec<KohaFine>,
    /// Whether each current loan may be renewed: `ok` or Koha's reason.
    #[serde(default)]
    pub renewals: Vec<KohaAnswer>,
    /// The hold each lent Item fills when it comes back: `none`, or Koha's status and the
    /// hold's `borrowernumber`.
    #[serde(default)]
    pub holds_on_return: Vec<KohaAnswer>,
}

/// Koha's answer, as text, for one loan.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct KohaAnswer {
    /// `issues.issue_id`.
    pub issue_id: u64,
    /// The Item lent.
    pub itemnumber: u64,
    /// The answer.
    pub answer: String,
}

/// Koha's due date for one loan.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct KohaDue {
    /// `issues.issue_id`.
    pub issue_id: u64,
    /// The Item lent.
    pub itemnumber: u64,
    /// `YYYY-MM-DD HH:MM`.
    pub due: String,
}

/// Koha's fine for one overdue loan.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct KohaFine {
    /// `issues.issue_id`.
    pub issue_id: u64,
    /// The Item lent.
    pub itemnumber: u64,
    /// The amount in cents, before `MaxFine`.
    pub amount_cents: i64,
}

/// Why a Koha answer has nothing to be compared with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ParityGap {
    /// No Item of the Elzevir has the `itemnumber`.
    ItemMissing,
    /// The Item is not on loan in the Elzevir.
    NotOnLoan,
    /// The Elzevir finds no due date for the loan: its calendar has no open day to land on.
    NoDueDate,
}

/// The Elzevir's answers for the loan of one Item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoanAnswers {
    /// The due date from the loan's start, `YYYY-MM-DD HH:MM`, or why there is none.
    pub due: Result<String, ParityGap>,
    /// The fine at the recorder's `now`, in cents.
    pub fine_cents: i64,
    /// Whether the loan may be renewed at `renewals_at`, in Koha's words (see
    /// [`renewal_answer`]).
    pub renewal: String,
    /// The hold the Item fills when it comes back, in Koha's words (see [`hold_answer`]).
    pub hold_on_return: String,
}

/// The state of the hold a returned Item fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReturnHold {
    /// Already waiting for the Item.
    Waiting,
    /// The Item is in transit for it.
    InTransit,
    /// The first hold in the queue the Item may fill.
    Reserved,
}

/// The renewal check in Koha's words: `ok`, or the reason `CanBookBeRenewed` gives.
///
/// Koha: `C4::Circulation::CanBookBeRenewed`.
#[must_use]
pub const fn renewal_answer(check: Result<(), &RenewalRefusal>) -> &'static str {
    match check {
        Ok(()) => "ok",
        Err(RenewalRefusal::OnSite) => "onsite_checkout",
        Err(RenewalRefusal::TooMany) => "too_many",
        Err(RenewalRefusal::Overdue) => "overdue",
        Err(RenewalRefusal::TooMuchOwing { .. }) => "too_much_owing",
        Err(RenewalRefusal::OnHold) => "on_reserve",
        Err(RenewalRefusal::TooSoon { .. }) => "too_soon",
    }
}

/// The hold of a returned Item in Koha's words: `none`, or the status `CheckReserves`
/// gives and the `borrowernumber` of the hold (`?` when the patron did not come from Koha).
///
/// Koha: `C4::Reserves::CheckReserves`.
#[must_use]
pub fn hold_answer(hold: Option<(ReturnHold, Option<&str>)>) -> String {
    let Some((state, borrowernumber)) = hold else {
        return "none".to_owned();
    };
    let status = match state {
        ReturnHold::Waiting => "Waiting",
        ReturnHold::InTransit => "Transferred",
        ReturnHold::Reserved => "Reserved",
    };
    format!("{status} borrowernumber {}", borrowernumber.unwrap_or("?"))
}

/// A question asked of both systems.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Question {
    /// The due date of a loan.
    DueDate,
    /// The fine of an overdue loan.
    Fine,
    /// Whether a loan may be renewed.
    Renewal,
    /// The hold a returned Item fills.
    HoldOnReturn,
}

/// The comparison of one question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionCheck {
    /// The question.
    pub question: Question,
    /// Koha's answers.
    pub answers: usize,
    /// Answers the Elzevir gives the same.
    pub same: usize,
    /// Answers without anything to compare, by `issue_id`.
    pub gaps: BTreeMap<ParityGap, Vec<u64>>,
    /// Each different answer: `issue_id`, Koha's value, the Elzevir's.
    pub divergences: Vec<(u64, String, String)>,
}

impl QuestionCheck {
    fn new(question: Question) -> Self {
        Self {
            question,
            answers: 0,
            same: 0,
            gaps: BTreeMap::new(),
            divergences: Vec::new(),
        }
    }

    fn compare(&mut self, issue_id: u64, koha: String, elzevir: Result<String, ParityGap>) {
        self.answers += 1;
        match elzevir {
            Ok(elzevir) if elzevir == koha => self.same += 1,
            Ok(elzevir) => self.divergences.push((issue_id, koha, elzevir)),
            Err(gap) => self.gaps.entry(gap).or_default().push(issue_id),
        }
    }

    /// Whether every answer was compared and is the same.
    #[must_use]
    pub fn agrees(&self) -> bool {
        self.same == self.answers
    }
}

/// Cents as a decimal amount, the way both reports show money.
#[must_use]
pub fn amount(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let cents = cents.unsigned_abs();
    format!("{sign}{}.{:02}", cents / 100, cents % 100)
}

/// Compares Koha's answers with the Elzevir's, found by `itemnumber`.
#[must_use]
pub fn compare(
    koha: &KohaAnswers,
    elzevir: &BTreeMap<u64, Result<LoanAnswers, ParityGap>>,
) -> Vec<QuestionCheck> {
    let mut due_dates = QuestionCheck::new(Question::DueDate);
    for answer in &koha.due_dates {
        let found = elzevir
            .get(&answer.itemnumber)
            .cloned()
            .unwrap_or(Err(ParityGap::ItemMissing))
            .and_then(|loan| loan.due);
        due_dates.compare(answer.issue_id, answer.due.clone(), found);
    }
    let mut fines = QuestionCheck::new(Question::Fine);
    for answer in &koha.fines {
        let found = elzevir
            .get(&answer.itemnumber)
            .cloned()
            .unwrap_or(Err(ParityGap::ItemMissing))
            .map(|loan| amount(loan.fine_cents));
        fines.compare(answer.issue_id, amount(answer.amount_cents), found);
    }
    let renewals = text_check(Question::Renewal, &koha.renewals, elzevir, |loan| {
        loan.renewal.clone()
    });
    let holds = text_check(
        Question::HoldOnReturn,
        &koha.holds_on_return,
        elzevir,
        |loan| loan.hold_on_return.clone(),
    );
    vec![due_dates, fines, renewals, holds]
}

fn text_check(
    question: Question,
    answers: &[KohaAnswer],
    elzevir: &BTreeMap<u64, Result<LoanAnswers, ParityGap>>,
    value: impl Fn(&LoanAnswers) -> String,
) -> QuestionCheck {
    let mut check = QuestionCheck::new(question);
    for answer in answers {
        let found = elzevir
            .get(&answer.itemnumber)
            .map_or(Err(ParityGap::ItemMissing), |loan| {
                loan.as_ref().map(&value).map_err(|gap| *gap)
            });
        check.compare(answer.issue_id, answer.answer.clone(), found);
    }
    check
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loan(due: &str, fine_cents: i64) -> LoanAnswers {
        LoanAnswers {
            due: Ok(due.to_owned()),
            fine_cents,
            renewal: "ok".to_owned(),
            hold_on_return: "none".to_owned(),
        }
    }

    #[test]
    fn each_answer_is_the_same_different_or_without_a_counterpart() {
        let koha: KohaAnswers = serde_json::from_str(
            r#"{"koha_version":"26.05.05.000","now":"2026-11-27 10:00",
                "due_dates":[{"issue_id":1,"itemnumber":100,"due":"2026-12-10 23:59"},
                             {"issue_id":2,"itemnumber":101,"due":"2026-12-11 23:59"},
                             {"issue_id":3,"itemnumber":102,"due":"2026-12-12 23:59"},
                             {"issue_id":4,"itemnumber":103,"due":"2026-12-13 23:59"},
                             {"issue_id":5,"itemnumber":104,"due":"2026-12-14 23:59"}],
                "fines":[{"issue_id":1,"itemnumber":100,"amount_cents":150},
                         {"issue_id":2,"itemnumber":101,"amount_cents":75}]}"#,
        )
        .unwrap();
        let elzevir = BTreeMap::from([
            (100, Ok(loan("2026-12-10 23:59", 150))),
            (101, Ok(loan("2026-12-12 23:59", 50))),
            (102, Err(ParityGap::NotOnLoan)),
            (
                104,
                Ok(LoanAnswers {
                    due: Err(ParityGap::NoDueDate),
                    fine_cents: 0,
                    renewal: "too_soon".to_owned(),
                    hold_on_return: "Reserved borrowernumber 7".to_owned(),
                }),
            ),
        ]);
        let checks = compare(&koha, &elzevir);
        let [due_dates, fines, renewals, holds] = checks.as_slice() else {
            panic!("four questions");
        };
        // A file of part 1 asks nothing about renewals and holds.
        assert_eq!((renewals.answers, holds.answers), (0, 0));
        assert!(renewals.agrees() && holds.agrees());
        assert_eq!((due_dates.answers, due_dates.same), (5, 1));
        assert_eq!(
            due_dates.gaps,
            BTreeMap::from([
                (ParityGap::ItemMissing, vec![4]),
                (ParityGap::NotOnLoan, vec![3]),
                (ParityGap::NoDueDate, vec![5]),
            ])
        );
        assert_eq!(
            due_dates.divergences,
            vec![(
                2,
                "2026-12-11 23:59".to_owned(),
                "2026-12-12 23:59".to_owned()
            )]
        );
        assert!(!due_dates.agrees());
        assert_eq!((fines.answers, fines.same), (2, 1));
        assert_eq!(
            fines.divergences,
            vec![(2, "0.75".to_owned(), "0.50".to_owned())]
        );
    }

    #[test]
    fn renewals_and_holds_are_compared_in_kohas_words() {
        let koha: KohaAnswers = serde_json::from_str(
            r#"{"now":"2026-11-27 10:00","renewals_at":"2026-11-27 10:05",
                "renewals":[{"issue_id":1,"itemnumber":100,"answer":"ok"},
                            {"issue_id":2,"itemnumber":101,"answer":"too_many"},
                            {"issue_id":3,"itemnumber":102,"answer":"on_reserve"}],
                "holds_on_return":[{"issue_id":1,"itemnumber":100,"answer":"none"},
                                   {"issue_id":2,"itemnumber":101,"answer":"Reserved borrowernumber 7"},
                                   {"issue_id":3,"itemnumber":102,"answer":"Waiting borrowernumber 8"}]}"#,
        )
        .unwrap();
        assert_eq!(koha.renewals_at.as_deref(), Some("2026-11-27 10:05"));
        let answers = |renewal: &str, hold: &str| {
            Ok(LoanAnswers {
                due: Ok("2026-12-10 23:59".to_owned()),
                fine_cents: 0,
                renewal: renewal.to_owned(),
                hold_on_return: hold.to_owned(),
            })
        };
        let elzevir = BTreeMap::from([
            (100, answers("ok", "none")),
            (101, answers("too_many", "Reserved borrowernumber 7")),
            (102, answers("too_soon", "Reserved borrowernumber 8")),
        ]);
        let checks = compare(&koha, &elzevir);
        let [_, _, renewals, holds] = checks.as_slice() else {
            panic!("four questions");
        };
        assert_eq!((renewals.answers, renewals.same), (3, 2));
        assert_eq!(
            renewals.divergences,
            vec![(3, "on_reserve".to_owned(), "too_soon".to_owned())]
        );
        assert_eq!((holds.answers, holds.same), (3, 2));
        assert_eq!(
            holds.divergences,
            vec![(
                3,
                "Waiting borrowernumber 8".to_owned(),
                "Reserved borrowernumber 8".to_owned()
            )]
        );
    }

    #[test]
    fn the_elzevir_names_its_answers_as_koha_does() {
        let soonest: jiff::Zoned = "2026-12-01T00:00[America/Sao_Paulo]".parse().unwrap();
        for (refusal, word) in [
            (RenewalRefusal::OnSite, "onsite_checkout"),
            (RenewalRefusal::TooMany, "too_many"),
            (RenewalRefusal::Overdue, "overdue"),
            (
                RenewalRefusal::TooMuchOwing {
                    owed: elzevir_domain::catalog::Money(10_001),
                },
                "too_much_owing",
            ),
            (RenewalRefusal::OnHold, "on_reserve"),
            (RenewalRefusal::TooSoon { soonest }, "too_soon"),
        ] {
            assert_eq!(renewal_answer(Err(&refusal)), word);
        }
        assert_eq!(renewal_answer(Ok(())), "ok");
        assert_eq!(hold_answer(None), "none");
        assert_eq!(
            hold_answer(Some((ReturnHold::Waiting, Some("5034")))),
            "Waiting borrowernumber 5034"
        );
        assert_eq!(
            hold_answer(Some((ReturnHold::InTransit, None))),
            "Transferred borrowernumber ?"
        );
        assert_eq!(
            hold_answer(Some((ReturnHold::Reserved, Some("2")))),
            "Reserved borrowernumber 2"
        );
    }

    #[test]
    fn amounts_read_as_decimals() {
        assert_eq!(amount(0), "0.00");
        assert_eq!(amount(5), "0.05");
        assert_eq!(amount(12345), "123.45");
        assert_eq!(amount(-250), "-2.50");
    }
}
