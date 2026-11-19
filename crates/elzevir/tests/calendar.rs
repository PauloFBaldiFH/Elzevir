// SPDX-License-Identifier: GPL-3.0-or-later
//! Koha's due date and fine tests hold with each calendar saved by the store and read back
//! (docs/specs/circulacao/calendario.md; doc 03, task 3.3).

use std::path::{Path, PathBuf};

use elzevir_app::calendar::{CalendarAction, CalendarBook, CalendarChange, HolidayText};
use elzevir_app::catalog::{CatalogCommand, CatalogOutput};
use elzevir_app::circulation_rules::CirculationCommand;
use elzevir_app::commands::{WriteCommand, WriteOutput};
use elzevir_domain::calendar::Calendar;
use elzevir_domain::catalog::BranchId;
use elzevir_scenarios::{find_koha_tests, load_koha_test, run_koha_test_through};
use elzevir_store::{ElzevirHandler, ElzevirStore, StoreConfig};

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn submit(store: &ElzevirStore, command: WriteCommand) -> Result<WriteOutput, String> {
    store
        .submit(command)
        .map_err(|error| error.to_string())?
        .wait()
        .map_err(|error| error.to_string())
}

/// Saves `calendar` as a new branch's, entry by entry, and reads it back.
fn saved(store: &ElzevirStore, calendar: &Calendar, number: usize) -> Result<Calendar, String> {
    let Ok(WriteOutput::Catalog(CatalogOutput::Branch(branch))) = submit(
        store,
        WriteCommand::Catalog(CatalogCommand::CreateBranch {
            name: format!("Unidade {number}"),
        }),
    ) else {
        return Err("no branch".to_owned());
    };
    let text = || HolidayText {
        title: "Fechado".to_owned(),
        description: None,
    };
    let mut actions = Vec::new();
    for weekday in calendar.closed_weekdays() {
        actions.push(CalendarAction::CloseWeekday {
            weekday: weekday.to_sunday_zero_offset().unsigned_abs(),
            text: text(),
        });
    }
    for holiday in calendar.annual_holidays() {
        actions.push(CalendarAction::CloseAnnually {
            holiday,
            text: text(),
        });
    }
    for (day, entry) in calendar.dated_entries() {
        actions.push(CalendarAction::SetDates {
            first: day,
            last: day,
            entry,
            text: text(),
        });
    }
    for action in actions {
        submit(
            store,
            WriteCommand::Circulation(CirculationCommand::ChangeCalendar(CalendarChange {
                branch,
                action,
            })),
        )?;
    }
    store
        .calendar(BranchId(branch.0))
        .map_err(|error| error.to_string())
}

#[test]
fn koha_due_date_and_fine_tests_hold_with_stored_calendars() {
    let dir = tempfile::tempdir().unwrap();
    let store = ElzevirStore::open(
        &dir.path().join("elzevir.db"),
        &StoreConfig::default(),
        ElzevirHandler,
    )
    .unwrap();
    let files = find_koha_tests(&repository().join("scenarios")).unwrap();
    assert!(files.len() >= 2, "expected CalcDateDue.t and CalcFine.t");
    let mut failures = Vec::new();
    let mut routed = 0;
    let branches = std::cell::Cell::new(0);
    for path in files {
        let scenario = load_koha_test(&path).unwrap();
        let route = |calendar: Calendar| {
            branches.set(branches.get() + 1);
            let read = saved(&store, &calendar, branches.get())?;
            if read != calendar {
                return Err(format!("{read:?} came back for {calendar:?}"));
            }
            Ok(read)
        };
        for (name, error) in run_koha_test_through(&scenario, &route) {
            failures.push(format!("{} / {name}: {error}", path.display()));
        }
        routed += scenario.cases.len();
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(routed > 30, "{routed} cases");
}
