// SPDX-License-Identifier: GPL-3.0-or-later
//! Koha's status names and item status dates through the real store, and their check
//! (docs/specs/circulacao/lost-items.md, items 17 and 18).
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "integration test: helper functions fail the test on error"
)]

use elzevir_app::catalog::CatalogCommand;
use elzevir_app::codes::{CodeTable, NamedCode};
use elzevir_app::commands::{WriteCommand, WriteOutput};
use elzevir_app::import::{ImportOptions, import_marc_text};
use elzevir_app::item_status::{ItemStatusDesk, StatusKind};
use elzevir_app::koha_migration::{KohaPlan, KohaStatusesMigrated, plan};
use elzevir_app::koha_verify::{Cause, Table, Unexplained, verify};
use elzevir_app::matching::Matching;
use elzevir_app::patrons::{PatronCommand, PatronOutput};
use elzevir_domain::catalog::MatchRule;
use elzevir_store::{ElzevirHandler, ElzevirStore, StoreConfig};
use jiff::civil::date;
use jiff::tz::TimeZone;

const DUMP: &str = include_str!("../../elzevir-app/tests/data/koha-statuses.sql");

/// Items 100 lost, 101 damaged and withdrawn, 110 with no status.
const CATALOG: &str = "=LDR  00000nam a2200000 a 4500
=245  10$aPrimeiro livro fictício
=952  \\\\$9100$aCTR$bCTR$pB100$12
=952  \\\\$9101$aCTR$bCTR$pB101$41$01
=942  \\\\$cLIVRO
=999  \\\\$c10$d10

=LDR  00000nam a2200000 a 4500
=245  10$aSegundo livro fictício
=952  \\\\$9110$aCTR$bCTR$pB110
=942  \\\\$cLIVRO
=999  \\\\$c11$d11
";

fn the_plan() -> KohaPlan {
    plan(
        DUMP.as_bytes(),
        date(2026, 11, 28),
        &TimeZone::get("America/Sao_Paulo").unwrap(),
    )
    .unwrap()
}

fn statuses(store: &ElzevirStore, plan: &KohaPlan) -> KohaStatusesMigrated {
    let mut done = KohaStatusesMigrated::default();
    for batch in plan.status_batches() {
        let answer = store
            .submit(WriteCommand::Patrons(PatronCommand::MigrateKohaStatuses(
                Box::new(batch),
            )))
            .unwrap()
            .wait()
            .unwrap();
        let WriteOutput::Patrons(PatronOutput::KohaStatusesMigrated(write)) = answer else {
            panic!("not a status migration");
        };
        done.add(&write);
    }
    done
}

/// A store with the catalog of [`CATALOG`] imported.
fn with_catalog(dir: &tempfile::TempDir) -> ElzevirStore {
    let store = ElzevirStore::open(
        &dir.path().join("elzevir.db"),
        &StoreConfig::default(),
        ElzevirHandler,
    )
    .unwrap();
    store
        .submit(WriteCommand::Catalog(CatalogCommand::NameCodes(vec![
            NamedCode {
                table: CodeTable::Branch,
                code: "CTR".to_owned(),
                name: "Centro".to_owned(),
            },
            NamedCode {
                table: CodeTable::ItemType,
                code: "LIVRO".to_owned(),
                name: "Livro".to_owned(),
            },
        ])))
        .unwrap()
        .wait()
        .unwrap();
    let options = ImportOptions {
        origin: Some("koha".into()),
        matching: Some(Matching::new(MatchRule::KohaBiblionumber)),
        ..ImportOptions::default()
    };
    let loaded = import_marc_text(CATALOG.as_bytes(), &store, &options).unwrap();
    assert_eq!(loaded.items, 3, "{loaded:?}");

    store
}

#[test]
fn status_names_and_dates_come_with_their_items() {
    let dir = tempfile::tempdir().unwrap();
    let store = with_catalog(&dir);
    let plan = the_plan();
    // 100 and "x" are not status numbers, -1 keeps its generic name, and a name cannot be
    // empty.
    assert_eq!(plan.status_names_left, 4);
    // 101's lost date is left behind with lost 0, and 110 has no date.
    let items: Vec<u64> = plan
        .status_dates
        .iter()
        .map(|dates| dates.item.itemnumber)
        .collect();
    assert_eq!(items, [100, 101, 999]);
    let done = statuses(&store, &plan);
    assert_eq!(
        done,
        KohaStatusesMigrated {
            names: 6,
            names_numbered: 1,
            names_existing: 0,
            names_taken: 0,
            dates: 2,
            items_left: 1,
        }
    );
    let names: Vec<(StatusKind, i64, String)> = store
        .status_names()
        .unwrap()
        .into_iter()
        .map(|name| (name.kind, name.number, name.label))
        .collect();
    assert_eq!(
        names,
        [
            (StatusKind::Lost, 1, "Perdido".to_owned()),
            (StatusKind::Lost, 2, "Extraviado".to_owned()),
            (StatusKind::Lost, 3, "Perdido (3)".to_owned()),
            (StatusKind::Damaged, 1, "Danificado".to_owned()),
            (StatusKind::Withdrawn, 1, "Baixa".to_owned()),
            (StatusKind::NotForLoan, 1, "Referência".to_owned()),
        ]
    );
    let since = |barcode: &str| -> Vec<(StatusKind, i64, Option<String>)> {
        store
            .item_at_the_desk(barcode)
            .unwrap()
            .unwrap()
            .statuses
            .into_iter()
            .map(|line| (line.kind, line.number, line.since.map(|at| at.to_string())))
            .collect()
    };
    assert_eq!(
        since("B100"),
        [
            (
                StatusKind::Lost,
                2,
                Some("2026-11-02T09:30:00-03:00[America/Sao_Paulo]".to_owned())
            ),
            (StatusKind::Damaged, 0, None),
            (StatusKind::Withdrawn, 0, None),
        ]
    );
    assert_eq!(
        since("B101"),
        [
            (StatusKind::Lost, 0, None),
            (
                StatusKind::Damaged,
                1,
                Some("2026-10-02T08:00:00-03:00[America/Sao_Paulo]".to_owned())
            ),
            (
                StatusKind::Withdrawn,
                1,
                Some("2026-10-01T08:00:00-03:00[America/Sao_Paulo]".to_owned())
            ),
        ]
    );

    // A second run finds every name; the dates are the same.
    let again = statuses(&store, &plan);
    assert_eq!(
        (again.names, again.names_existing, again.names_taken),
        (0, 6, 0)
    );
}

#[test]
fn the_check_finds_the_names_and_dates_and_their_differences() {
    let dir = tempfile::tempdir().unwrap();
    let store = with_catalog(&dir);
    let plan = the_plan();
    statuses(&store, &plan);
    let found = store.koha_footprint().unwrap();
    let checks = verify(&plan, &[], &found);
    let check = |table| checks.iter().find(|check| check.table == table).unwrap();
    let names = check(Table::StatusNames);
    // Lost 3 came with its number, since lost 1 has its name.
    assert_eq!((names.in_dump, names.arrived), (6, 6));
    assert!(names.explained.is_empty(), "{names:?}");
    assert!(names.unexplained.is_empty(), "{names:?}");
    let dates = check(Table::StatusDates);
    assert_eq!((dates.in_dump, dates.arrived), (3, 2));
    assert_eq!(
        dates.explained.get(&Cause::ItemLeftOut),
        Some(&vec!["999".to_owned()])
    );
    assert!(dates.unexplained.is_empty(), "{dates:?}");

    // A name changed afterwards, and a date lost, are differences without a cause.
    let mut changed = found;
    changed
        .status_names
        .insert((StatusKind::Lost, 2), "Sumido".to_owned());
    changed
        .status_names
        .insert((StatusKind::Lost, 9), "Novo".to_owned());
    if let Some(item) = changed
        .items
        .get_mut("100")
        .and_then(|items| items.first_mut())
    {
        item.lost_on = None;
    }
    let checks = verify(&plan, &[], &changed);
    let check = |table| checks.iter().find(|check| check.table == table).unwrap();
    assert_eq!(
        check(Table::StatusNames).unexplained,
        [
            ("lost 2".to_owned(), Unexplained::Field("lib")),
            ("lost 9".to_owned(), Unexplained::Extra),
        ]
    );
    assert_eq!(
        check(Table::StatusDates).unexplained,
        [("100".to_owned(), Unexplained::Field("itemlost_on"))]
    );
}
