// SPDX-License-Identifier: GPL-3.0-or-later
//! The writes of the patron migration from Koha
//! (docs/specs/migracao/leitores-do-koha.md), with fictitious patrons.
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "integration test: helper functions fail the test on error"
)]

use elzevir_app::commands::{WriteCommand, WriteOutput};
use elzevir_app::koha_migration::{KohaMigrated, KohaPlan, plan};
use elzevir_app::patrons::{CategoryChange, PatronCommand, PatronOutput};
use elzevir_domain::patron::Enrolment;
use elzevir_store::{ElzevirHandler, ElzevirStore, StoreConfig};
use jiff::civil::date;
use jiff::tz::TimeZone;

const DUMP: &[u8] = include_bytes!("../../elzevir-app/tests/data/koha-patrons.sql");

fn open(dir: &tempfile::TempDir) -> ElzevirStore {
    ElzevirStore::open(
        &dir.path().join("elzevir.db"),
        &StoreConfig::default(),
        ElzevirHandler,
    )
    .unwrap()
}

fn run(store: &ElzevirStore, command: PatronCommand) -> PatronOutput {
    match store
        .submit(WriteCommand::Patrons(command))
        .unwrap()
        .wait()
        .unwrap()
    {
        WriteOutput::Patrons(output) => output,
        other => panic!("unexpected output {other:?}"),
    }
}

fn the_plan() -> KohaPlan {
    plan(
        DUMP,
        date(2026, 11, 26),
        &TimeZone::get("America/Sao_Paulo").unwrap(),
    )
    .unwrap()
}

fn migrate(store: &ElzevirStore, plan: &KohaPlan) -> KohaMigrated {
    let mut done = KohaMigrated::default();
    for batch in plan.batches() {
        let PatronOutput::KohaMigrated(write) =
            run(store, PatronCommand::MigrateKoha(Box::new(batch)))
        else {
            panic!("not a migration");
        };
        done.add(write);
    }
    done
}

fn rows(store: &ElzevirStore, sql: &str) -> Vec<String> {
    store
        .readers()
        .read(|c| {
            let mut statement = c.prepare(sql)?;
            let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
        })
        .unwrap()
}

#[test]
fn migrates_and_runs_again_without_doubling() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(&dir);
    // A category the library already has, by the name Koha uses.
    run(
        &store,
        PatronCommand::ChangeCategory(CategoryChange::Create {
            name: "Adulto".to_owned(),
            enrolment: Enrolment::Months(24),
        }),
    );
    let plan = the_plan();
    let done = migrate(&store, &plan);
    assert_eq!(
        done,
        KohaMigrated {
            branches_created: 2,
            branches_updated: 0,
            categories_created: 2,
            categories_linked: 1,
            patrons_added: 3,
            already_migrated: 0,
            restrictions: 2,
            cardnumber_taken: Vec::new(),
        }
    );
    assert_eq!(
        rows(
            &store,
            "SELECT b.name || '|' || coalesce(b.address, '') FROM branches b
             JOIN koha_codes k ON k.kind = 1 AND k.local_id = b.id ORDER BY k.code"
        ),
        [
            "Biblioteca Central|Rua das Flores, 100, Cidade Exemplo, PR, 85950-000, Brasil",
            "Ramal Norte|"
        ]
    );
    assert_eq!(
        rows(
            &store,
            "SELECT p.cardnumber || '|' || c.name || '|' || p.enrolled || '|' || p.expires
                 || '|' || p.reading_history
             FROM patrons p JOIN patron_categories c ON c.id = p.category_id ORDER BY p.cardnumber"
        ),
        [
            "A0001|Adulto|2020-01-15|2030-01-15|1",
            // Enrolled today; the expiry comes from the category's 12 months.
            "A0002|INS|2026-11-26|2027-11-26|0",
            // The expiry before the enrolment went; the category's fixed day stands.
            "A0006|Estudante|2025-01-10|2027-12-31|0",
        ]
    );
    assert_eq!(
        rows(
            &store,
            "SELECT kind || '|' || coalesce(expires, '-') || '|' || created_at
             FROM patron_restrictions ORDER BY kind"
        ),
        [
            "OVERDUES|-|2024-04-01T08:00:00-03:00[America/Sao_Paulo]",
            "SUSPENSION|-|2024-02-01T09:30:00-03:00[America/Sao_Paulo]",
        ]
    );
    assert_eq!(
        rows(
            &store,
            "SELECT koha_key FROM koha_keys WHERE kind = 2 ORDER BY koha_key"
        ),
        ["1", "2", "6"]
    );
    assert_eq!(
        rows(
            &store,
            "SELECT kind FROM audit_log WHERE kind LIKE 'patrons.migrate%'"
        ),
        ["patrons.migrate_koha"]
    );

    // A second run adds nobody and updates the branches from Koha again.
    let again = migrate(&store, &plan);
    assert_eq!(
        (
            again.branches_updated,
            again.patrons_added,
            again.already_migrated
        ),
        (2, 0, 3)
    );
    assert_eq!(rows(&store, "SELECT count(*) || '' FROM patrons"), ["3"]);
}

#[test]
fn a_taken_card_leaves_the_patron_out() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(&dir);
    let mut plan = the_plan();
    let first = migrate(&store, &plan);
    assert_eq!(first.patrons_added, 3);
    // Another Koha patron with a card the Elzevir already has.
    let mut twin = plan.patrons[0].clone();
    twin.borrowernumber = 99;
    plan.patrons = vec![twin];
    let done = migrate(&store, &plan);
    assert_eq!((done.patrons_added, done.cardnumber_taken), (0, vec![99]));
}
