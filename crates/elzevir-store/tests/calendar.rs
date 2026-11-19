// SPDX-License-Identifier: GPL-3.0-or-later
//! Branch calendars set and read through the real store
//! (docs/specs/circulacao/calendario.md).
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "integration test: helper functions fail the test on error"
)]

use elzevir_app::calendar::{
    CalendarAction, CalendarBook, CalendarChange, Holiday, HolidayText, HolidayWhen,
};
use elzevir_app::catalog::{CatalogCommand, CatalogOutput};
use elzevir_app::circulation_rules::{CirculationCommand, CirculationOutput, CirculationRefusal};
use elzevir_app::commands::{WriteCommand, WriteOutput, WriteRefusal};
use elzevir_app::registries::{RegistryChange, RegistryEntry};
use elzevir_domain::calendar::{AnnualHoliday, Calendar, DatedEntry};
use elzevir_domain::catalog::BranchId;
use elzevir_store::{CommandFailed, ElzevirHandler, ElzevirStore, StoreConfig};
use jiff::civil::{Weekday, date};

fn open(dir: &tempfile::TempDir) -> ElzevirStore {
    ElzevirStore::open(
        &dir.path().join("elzevir.db"),
        &StoreConfig::default(),
        ElzevirHandler,
    )
    .unwrap()
}

fn branch(store: &ElzevirStore, name: &str) -> BranchId {
    let WriteOutput::Catalog(CatalogOutput::Branch(branch)) = store
        .submit(WriteCommand::Catalog(CatalogCommand::CreateBranch {
            name: name.to_owned(),
        }))
        .unwrap()
        .wait()
        .unwrap()
    else {
        panic!("no branch");
    };
    branch
}

fn text(title: &str) -> HolidayText {
    HolidayText {
        title: title.to_owned(),
        description: None,
    }
}

fn change(
    store: &ElzevirStore,
    branch: BranchId,
    action: CalendarAction,
) -> Result<usize, CirculationRefusal> {
    match store
        .submit(WriteCommand::Circulation(
            CirculationCommand::ChangeCalendar(CalendarChange { branch, action }),
        ))
        .unwrap()
        .wait()
    {
        Ok(WriteOutput::Circulation(CirculationOutput::CalendarChanged { entries })) => Ok(entries),
        Err(CommandFailed::Rejected(WriteRefusal::Circulation(refusal))) => Err(refusal),
        other => panic!("{other:?}"),
    }
}

/// Central with weekends closed, Christmas, a year-end recess and a Saturday open as an
/// exception; the last day of the recess retitled.
fn december(store: &ElzevirStore) -> BranchId {
    let central = branch(store, "Central");
    let close = |weekday| CalendarAction::CloseWeekday {
        weekday,
        text: text("Fim de semana"),
    };
    assert_eq!(change(store, central, close(0)), Ok(1));
    assert_eq!(change(store, central, close(6)), Ok(1));
    let christmas = AnnualHoliday { month: 12, day: 25 };
    assert_eq!(
        change(
            store,
            central,
            CalendarAction::CloseAnnually {
                holiday: christmas,
                text: text("Natal"),
            }
        ),
        Ok(1)
    );
    let set = |first, last, entry, title| CalendarAction::SetDates {
        first,
        last,
        entry,
        text: text(title),
    };
    assert_eq!(
        change(
            store,
            central,
            set(
                date(2026, 12, 28),
                date(2026, 12, 31),
                DatedEntry::Closed,
                "Recesso"
            )
        ),
        Ok(4)
    );
    // An exception on a Saturday, and a date set again takes the new title.
    assert_eq!(
        change(
            store,
            central,
            set(
                date(2026, 12, 5),
                date(2026, 12, 5),
                DatedEntry::Open,
                "Mutirão"
            )
        ),
        Ok(1)
    );
    change(
        store,
        central,
        set(
            date(2026, 12, 31),
            date(2026, 12, 31),
            DatedEntry::Closed,
            "Réveillon",
        ),
    )
    .unwrap();
    central
}

#[test]
fn a_branch_calendar_is_stored_and_retitled() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(&dir);
    let central = december(&store);
    let other = branch(&store, "Bairro");
    let christmas = AnnualHoliday { month: 12, day: 25 };
    let expected = Calendar::always_open()
        .closed_every(Weekday::Sunday)
        .closed_every(Weekday::Saturday)
        .closed_annually(christmas)
        .on(date(2026, 12, 5), DatedEntry::Open)
        .on(date(2026, 12, 28), DatedEntry::Closed)
        .on(date(2026, 12, 29), DatedEntry::Closed)
        .on(date(2026, 12, 30), DatedEntry::Closed)
        .on(date(2026, 12, 31), DatedEntry::Closed);
    assert_eq!(store.calendar(central).unwrap(), expected);
    assert_eq!(store.calendar(other).unwrap(), Calendar::always_open());
    let holidays = store.holidays(central).unwrap();
    assert_eq!(
        holidays.first(),
        Some(&Holiday {
            when: HolidayWhen::Weekly(0),
            text: text("Fim de semana"),
        })
    );
    assert_eq!(
        holidays.last(),
        Some(&Holiday {
            when: HolidayWhen::Dated(date(2026, 12, 31), DatedEntry::Closed),
            text: text("Réveillon"),
        })
    );
}

#[test]
fn entries_are_removed_and_go_with_their_branch() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(&dir);
    let central = december(&store);
    let christmas = AnnualHoliday { month: 12, day: 25 };
    assert_eq!(
        change(
            &store,
            central,
            CalendarAction::RemoveDates {
                first: date(2026, 12, 29),
                last: date(2026, 12, 31),
            }
        ),
        Ok(3)
    );
    assert_eq!(
        change(&store, central, CalendarAction::ReopenWeekday(6)),
        Ok(1)
    );
    assert_eq!(
        change(&store, central, CalendarAction::RemoveAnnual(christmas)),
        Ok(1)
    );
    assert_eq!(
        store.calendar(central).unwrap(),
        Calendar::always_open()
            .closed_every(Weekday::Sunday)
            .on(date(2026, 12, 5), DatedEntry::Open)
            .on(date(2026, 12, 28), DatedEntry::Closed)
    );

    // A deleted branch takes its calendar with it.
    store
        .submit(WriteCommand::Catalog(CatalogCommand::ChangeRegistry(
            RegistryChange::Delete(RegistryEntry::Branch(central)),
        )))
        .unwrap()
        .wait()
        .unwrap();
    assert!(store.holidays(central).unwrap().is_empty());
}

#[test]
fn a_change_that_does_not_fit_is_refused_and_stores_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(&dir);
    let central = branch(&store, "Central");
    let refusals = [
        (
            CalendarAction::CloseWeekday {
                weekday: 7,
                text: text("Nunca"),
            },
            CirculationRefusal::BadWeekday,
        ),
        (
            CalendarAction::CloseWeekday {
                weekday: 1,
                text: text("  "),
            },
            CirculationRefusal::BadHolidayTitle,
        ),
        (
            CalendarAction::CloseAnnually {
                holiday: AnnualHoliday { month: 4, day: 31 },
                text: text("Nunca"),
            },
            CirculationRefusal::BadDates,
        ),
        (
            CalendarAction::SetDates {
                first: date(2026, 12, 31),
                last: date(2026, 12, 1),
                entry: DatedEntry::Closed,
                text: text("Ao contrário"),
            },
            CirculationRefusal::BadDates,
        ),
        (
            CalendarAction::SetDates {
                first: date(2026, 1, 1),
                last: date(2027, 6, 1),
                entry: DatedEntry::Closed,
                text: text("Longo demais"),
            },
            CirculationRefusal::BadDates,
        ),
    ];
    for (action, refusal) in refusals {
        assert_eq!(change(&store, central, action), Err(refusal));
    }
    assert_eq!(
        change(
            &store,
            BranchId(999),
            CalendarAction::CloseWeekday {
                weekday: 0,
                text: text("Domingo"),
            }
        ),
        Err(CirculationRefusal::UnknownBranch)
    );
    assert!(store.holidays(central).unwrap().is_empty());
}
