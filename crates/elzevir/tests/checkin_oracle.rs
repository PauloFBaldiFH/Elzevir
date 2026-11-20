// SPDX-License-Identifier: GPL-3.0-or-later
//! A loan renewed and returned late on the Koha oracle is renewed, refused and suspended
//! the same way by `circ.renew`, `circ.checkin` and `circ.checkout`
//! (docs/specs/circulacao/renovacao.md, docs/specs/circulacao/suspensao.md;
//! `scenarios/circulacao/devolucao-renovacao.koha.json`); and two late returns are fined,
//! hold back the next checkout and are paid and written off the same way
//! (docs/specs/circulacao/conta.md; `scenarios/circulacao/multa-conta.koha.json`).

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "tests: a missing code or field fails the test"
)]

mod oracle;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use elzevir_app::accounts::{AccountLineId, LineKind, PaymentRequest};
use elzevir_app::calendar::{CalendarAction, CalendarChange, HolidayText};
use elzevir_app::catalog::ItemStatus;
use elzevir_app::circulation_rules::{CirculationCommand, CirculationOutput, CirculationRefusal};
use elzevir_app::commands::{WriteCommand, WriteOutput, WriteRefusal};
use elzevir_app::loans::{
    CheckinRequest, CheckoutRefusal, CheckoutRequest, LoanDesk, RenewRefusal, RenewRequest,
};
use elzevir_domain::account::{CreditType, rule_money};
use elzevir_domain::calendar::{AnnualHoliday, DatedEntry};
use elzevir_domain::catalog::Money;
use elzevir_domain::checkout::{Confirm, Impossible};
use elzevir_domain::renewal::RenewalRefusal;
use elzevir_domain::suspension::SuspensionEnd;
use elzevir_store::CommandFailed;
use jiff::Zoned;
use jiff::civil::Date;
use oracle::{Library, RecordedRules, submit};
use serde::Deserialize;
use serde_json::Value;

const TIME_ZONE: &str = "America/Sao_Paulo";

#[derive(Deserialize)]
struct Recording {
    patron: RecordedPatron,
    item: RecordedItem,
    other_item: RecordedItem,
    rules: Vec<RecordedRules>,
    renewal_rules: Vec<RecordedRules>,
    calendar: RecordedCalendar,
    checkout: Moment,
    too_soon: Asked,
    renewal: Moment,
    too_many: Asked,
    checkin: Moment,
    suspended: Asked,
}

#[derive(Deserialize)]
struct RecordedPatron {
    category: String,
    library: String,
}

#[derive(Deserialize)]
struct RecordedItem {
    barcode: String,
    item_type: String,
    home_library: String,
    holding_library: String,
}

#[derive(Deserialize)]
struct RecordedCalendar {
    weekly_closed: Vec<u8>,
    annual: Vec<[i8; 2]>,
    closed: Vec<Date>,
    open: Vec<Date>,
}

/// When something happened, and the due date it gave.
#[derive(Deserialize)]
struct Moment {
    at: String,
    due: Option<String>,
}

/// When the oracle was asked, and its answer.
#[derive(Deserialize)]
struct Asked {
    at: String,
    answer: Value,
}

/// A recorded time in the library's time zone.
fn zoned(text: &str) -> Zoned {
    text.parse::<jiff::Timestamp>()
        .unwrap()
        .in_tz(TIME_ZONE)
        .unwrap()
}

fn library(
    dir: &tempfile::TempDir,
    patron: &RecordedPatron,
    items: &[&RecordedItem],
    rules: &[&RecordedRules],
) -> Library {
    let mut codes = BTreeSet::new();
    codes.insert((true, patron.library.clone()));
    for item in items {
        codes.insert((true, item.home_library.clone()));
        codes.insert((true, item.holding_library.clone()));
        codes.insert((false, item.item_type.clone()));
    }
    for rules in rules {
        for (is_branch, code) in [
            (true, &rules.context.library),
            (false, &rules.context.item_type),
        ] {
            if code != "*" {
                codes.insert((is_branch, code.clone()));
            }
        }
    }
    let library = Library::new(dir, &codes, &patron.library, &patron.category);
    for item in items {
        library.add_item(
            &item.barcode,
            (&item.home_library, &item.holding_library),
            &item.item_type,
            ItemStatus::default(),
        );
    }
    library
}

/// The branch's calendar as the oracle's calendar screen listed it.
fn set_calendar(library: &Library, branch: &str, calendar: &RecordedCalendar) {
    let text = || HolidayText {
        title: "Feriado".to_owned(),
        description: None,
    };
    let mut actions = Vec::new();
    for &weekday in &calendar.weekly_closed {
        actions.push(CalendarAction::CloseWeekday {
            weekday,
            text: text(),
        });
    }
    for &[month, day] in &calendar.annual {
        actions.push(CalendarAction::CloseAnnually {
            holiday: AnnualHoliday::new(month, day).unwrap(),
            text: text(),
        });
    }
    for (dates, entry) in [
        (&calendar.closed, DatedEntry::Closed),
        (&calendar.open, DatedEntry::Open),
    ] {
        for &day in dates {
            actions.push(CalendarAction::SetDates {
                first: day,
                last: day,
                entry,
                text: text(),
            });
        }
    }
    for action in actions {
        submit(
            &library.store,
            WriteCommand::Circulation(CirculationCommand::ChangeCalendar(CalendarChange {
                branch: library.branches[branch],
                action,
            })),
        )
        .unwrap();
    }
}

fn circulate(
    library: &Library,
    command: CirculationCommand,
) -> Result<WriteOutput, CirculationRefusal> {
    match submit(&library.store, WriteCommand::Circulation(command)) {
        Ok(output) => Ok(output),
        Err(CommandFailed::Rejected(WriteRefusal::Circulation(refusal))) => Err(refusal),
        other => panic!("{other:?}"),
    }
}

fn checkout(
    library: &Library,
    barcode: &str,
    desk: &str,
    at: &str,
) -> Result<WriteOutput, CirculationRefusal> {
    circulate(
        library,
        CirculationCommand::Checkout(CheckoutRequest {
            patron: library.patron,
            barcode: barcode.to_owned(),
            desk: library.branches[desk],
            on_site: false,
            confirmed: false,
            at: zoned(at),
        }),
    )
}

fn renew(library: &Library, barcode: &str, at: &str) -> Result<(Zoned, u32), RenewRefusal> {
    match circulate(
        library,
        CirculationCommand::Renew(RenewRequest {
            barcode: barcode.to_owned(),
            override_limit: false,
            at: zoned(at),
        }),
    ) {
        Ok(WriteOutput::Circulation(CirculationOutput::Renewed(done))) => {
            Ok((done.due, done.renewals))
        }
        Err(CirculationRefusal::Renew(refusal)) => Err(refusal),
        other => panic!("{other:?}"),
    }
}

/// Koha's `allows_renewal` answer for a refusal.
fn renewal_error(refusal: &RenewRefusal) -> &'static str {
    match refusal {
        RenewRefusal::Refused(RenewalRefusal::TooSoon { .. }) => "too_soon",
        RenewRefusal::Refused(RenewalRefusal::TooMany) => "too_many",
        RenewRefusal::Refused(RenewalRefusal::Overdue) => "overdue",
        RenewRefusal::Refused(RenewalRefusal::OnSite) => "onsite_checkout",
        RenewRefusal::UnknownBarcode | RenewRefusal::NotOnLoan => "no_checkout",
        RenewRefusal::NoDueDate => "no_due_date",
    }
}

fn same_due(ours: &Zoned, koha: &str) -> bool {
    ours.timestamp() == koha.parse::<jiff::Timestamp>().unwrap()
}

#[test]
fn a_renewed_late_loan_is_answered_as_the_oracle_answered() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scenarios/circulacao/devolucao-renovacao.koha.json");
    let recording: Recording =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let library = library(
        &dir,
        &recording.patron,
        &[&recording.item, &recording.other_item],
        &recording
            .rules
            .iter()
            .chain(&recording.renewal_rules)
            .collect::<Vec<_>>(),
    );
    let desk = &recording.patron.library;
    set_calendar(&library, desk, &recording.calendar);
    library.set_rules(&recording.rules).unwrap();
    let item = &recording.item.barcode;

    let Ok(WriteOutput::Circulation(CirculationOutput::CheckedOut(lent))) =
        checkout(&library, item, desk, &recording.checkout.at)
    else {
        panic!("the checkout failed");
    };
    let koha_due = recording.checkout.due.as_deref().unwrap();
    assert!(
        same_due(&lent.due, koha_due),
        "due {}, Koha {koha_due}",
        lent.due
    );

    let mut answers = BTreeMap::new();
    let refusal = renew(&library, item, &recording.too_soon.at).unwrap_err();
    answers.insert(
        "too_soon",
        (
            renewal_error(&refusal),
            recording.too_soon.answer["error"].clone(),
        ),
    );
    library.set_rules(&recording.renewal_rules).unwrap();
    let (due, renewals) = renew(&library, item, &recording.renewal.at).unwrap();
    let koha_due = recording.renewal.due.as_deref().unwrap();
    assert!(
        same_due(&due, koha_due),
        "renewed until {due}, Koha {koha_due}"
    );
    assert_eq!(renewals, 1);
    let refusal = renew(&library, item, &recording.too_many.at).unwrap_err();
    answers.insert(
        "too_many",
        (
            renewal_error(&refusal),
            recording.too_many.answer["error"].clone(),
        ),
    );
    for (name, (ours, koha)) in &answers {
        assert_eq!(Some(*ours), koha.as_str(), "{name}");
    }

    let Ok(WriteOutput::Circulation(CirculationOutput::CheckedIn(back))) = circulate(
        &library,
        CirculationCommand::Checkin(CheckinRequest {
            barcode: item.clone(),
            desk: library.branches[desk],
            at: zoned(&recording.checkin.at),
        }),
    ) else {
        panic!("the checkin failed");
    };
    let blockers = &recording.suspended.answer["blockers"];
    let koha_until: Date = blockers["USERBLOCKEDWITHENDDATE"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(back.suspended_until, Some(koha_until));

    let refused = checkout(
        &library,
        &recording.other_item.barcode,
        desk,
        &recording.suspended.at,
    );
    assert_eq!(
        refused.unwrap_err(),
        CirculationRefusal::Checkout(CheckoutRefusal::Impossible(vec![Impossible::Suspended(
            SuspensionEnd::Day(koha_until)
        )]))
    );
    let koha_keys: BTreeSet<&str> = blockers
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        koha_keys,
        BTreeSet::from(["DEBARRED", "USERBLOCKEDWITHENDDATE"])
    );
}

#[derive(Deserialize)]
struct FinesRecording {
    patron: RecordedPatron,
    items: Vec<RecordedItem>,
    rules: Vec<RecordedRules>,
    calendar: RecordedCalendar,
    checkouts: Vec<RecordedLoan>,
    fines: Vec<RecordedLine>,
    debt: Asked,
    payment: RecordedCredit,
    after_payment: Vec<RecordedLine>,
    writeoff: RecordedCredit,
    after_writeoff: Vec<RecordedLine>,
    credits: Vec<RecordedLine>,
}

#[derive(Deserialize)]
struct RecordedLoan {
    at: String,
    due: String,
    returned: String,
}

#[derive(Deserialize)]
struct RecordedLine {
    status: Option<String>,
    amount: Value,
    amount_outstanding: Value,
}

#[derive(Deserialize)]
struct RecordedCredit {
    amount: Value,
}

/// A recorded amount in cents, read from its decimal text as a rule amount is.
fn cents(amount: &Value) -> Money {
    let text = match amount {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    };
    rule_money(&text).unwrap()
}

/// Our lines of one side, oldest first, as `(id, amount, outstanding)`.
fn lines(library: &Library, debit: bool) -> Vec<(AccountLineId, Money, Money)> {
    let mut lines: Vec<_> = library
        .store
        .account(library.patron)
        .unwrap()
        .into_iter()
        .filter(|line| matches!(line.kind, LineKind::Debit(_)) == debit)
        .map(|line| (line.id, line.amount, line.outstanding))
        .collect();
    lines.sort();
    lines
}

fn debits(library: &Library) -> Vec<(AccountLineId, Money, Money)> {
    lines(library, true)
}

fn same_lines(ours: &[(AccountLineId, Money, Money)], koha: &[RecordedLine], when: &str) {
    let ours: Vec<_> = ours
        .iter()
        .map(|&(_, amount, left)| (amount, left))
        .collect();
    let koha: Vec<_> = koha
        .iter()
        .map(|line| (cents(&line.amount), cents(&line.amount_outstanding)))
        .collect();
    assert_eq!(ours, koha, "{when}");
}

fn credit(library: &Library, kind: CreditType, amount: Money, lines: Vec<AccountLineId>, at: &str) {
    let Ok(WriteOutput::Circulation(CirculationOutput::Paid(_))) = circulate(
        library,
        CirculationCommand::Pay(PaymentRequest {
            patron: library.patron,
            kind,
            amount,
            lines,
            desk: library.branches["MAIN"],
            at: zoned(at),
        }),
    ) else {
        panic!("the {kind:?} failed");
    };
}

#[test]
fn late_returns_are_fined_and_settled_as_the_oracle_did() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scenarios/circulacao/multa-conta.koha.json");
    let recording: FinesRecording =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let library = library(
        &dir,
        &recording.patron,
        &recording.items.iter().collect::<Vec<_>>(),
        &recording.rules.iter().collect::<Vec<_>>(),
    );
    let desk = &recording.patron.library;
    set_calendar(&library, desk, &recording.calendar);
    library.set_rules(&recording.rules).unwrap();

    for (item, loan) in recording.items.iter().zip(&recording.checkouts) {
        let Ok(WriteOutput::Circulation(CirculationOutput::CheckedOut(lent))) =
            checkout(&library, &item.barcode, desk, &loan.at)
        else {
            panic!("the checkout failed");
        };
        assert!(
            same_due(&lent.due, &loan.due),
            "due {}, Koha {}",
            lent.due,
            loan.due
        );
    }
    for ((item, loan), koha) in recording
        .items
        .iter()
        .zip(&recording.checkouts)
        .zip(&recording.fines)
    {
        let Ok(WriteOutput::Circulation(CirculationOutput::CheckedIn(back))) = circulate(
            &library,
            CirculationCommand::Checkin(CheckinRequest {
                barcode: item.barcode.clone(),
                desk: library.branches[desk],
                at: zoned(&loan.returned),
            }),
        ) else {
            panic!("the checkin failed");
        };
        assert_eq!(back.fine, Some(cents(&koha.amount)), "{}", item.barcode);
        assert_eq!(koha.status.as_deref(), Some("RETURNED"));
    }
    same_lines(&debits(&library), &recording.fines, "fines");

    // The debt asks for confirmation (`DEBT` among Koha's confirmations).
    let koha_debt = &recording.debt.answer["confirms"]["DEBT"];
    let asked = checkout(
        &library,
        &recording.items[2].barcode,
        desk,
        &recording.debt.at,
    );
    assert_eq!(
        asked.unwrap_err(),
        CirculationRefusal::Checkout(CheckoutRefusal::NeedsConfirmation(vec![Confirm::Debt(
            cents(koha_debt)
        )]))
    );
    assert!(
        recording.debt.answer["blockers"]
            .as_object()
            .unwrap()
            .is_empty()
    );

    // A payment without lines settles the oldest fine first.
    let at = &recording.debt.at;
    credit(
        &library,
        CreditType::Payment,
        cents(&recording.payment.amount),
        Vec::new(),
        at,
    );
    let after_payment = debits(&library);
    same_lines(
        &after_payment,
        &recording.after_payment,
        "after the payment",
    );
    // The write-off of the line Koha chose, the second fine.
    let second = after_payment.last().unwrap().0;
    credit(
        &library,
        CreditType::Writeoff,
        cents(&recording.writeoff.amount),
        vec![second],
        at,
    );
    same_lines(
        &debits(&library),
        &recording.after_writeoff,
        "after the write-off",
    );

    same_lines(&lines(&library, false), &recording.credits, "credits");
}
