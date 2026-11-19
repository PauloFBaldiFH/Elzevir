// SPDX-License-Identifier: GPL-3.0-or-later
//! A loan renewed and returned late on the Koha oracle is renewed, refused and suspended
//! the same way by `circ.renew`, `circ.checkin` and `circ.checkout`
//! (docs/specs/circulacao/renovacao.md, docs/specs/circulacao/suspensao.md;
//! `scenarios/circulacao/devolucao-renovacao.koha.json`).

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "tests: a missing code or field fails the test"
)]

mod oracle;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use elzevir_app::calendar::{CalendarAction, CalendarChange, HolidayText};
use elzevir_app::catalog::ItemStatus;
use elzevir_app::circulation_rules::{CirculationCommand, CirculationOutput, CirculationRefusal};
use elzevir_app::commands::{WriteCommand, WriteOutput, WriteRefusal};
use elzevir_app::loans::{
    CheckinRequest, CheckoutRefusal, CheckoutRequest, RenewRefusal, RenewRequest,
};
use elzevir_domain::calendar::{AnnualHoliday, DatedEntry};
use elzevir_domain::checkout::Impossible;
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

fn library(dir: &tempfile::TempDir, recording: &Recording) -> Library {
    let mut codes = BTreeSet::new();
    codes.insert((true, recording.patron.library.clone()));
    for item in [&recording.item, &recording.other_item] {
        codes.insert((true, item.home_library.clone()));
        codes.insert((true, item.holding_library.clone()));
        codes.insert((false, item.item_type.clone()));
    }
    for rules in recording.rules.iter().chain(&recording.renewal_rules) {
        for (is_branch, code) in [
            (true, &rules.context.library),
            (false, &rules.context.item_type),
        ] {
            if code != "*" {
                codes.insert((is_branch, code.clone()));
            }
        }
    }
    let library = Library::new(
        dir,
        &codes,
        &recording.patron.library,
        &recording.patron.category,
    );
    for item in [&recording.item, &recording.other_item] {
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
    let library = library(&dir, &recording);
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
