// SPDX-License-Identifier: GPL-3.0-or-later
//! The writes of the patron migration from Koha
//! (docs/specs/migracao/leitores-do-koha.md), with fictitious patrons.
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "integration test: helper functions fail the test on error"
)]

use elzevir_app::catalog::CatalogCommand;
use elzevir_app::codes::{CodeTable, NamedCode};
use elzevir_app::commands::WriteRefusal;
use elzevir_app::commands::{WriteCommand, WriteOutput};
use elzevir_app::import::{ImportOptions, import_marc_text};
use elzevir_app::koha_migration::{
    CirculationLeftover, KohaAccountsMigrated, KohaCirculationMigrated, KohaHistoryMigrated,
    KohaMigrated, KohaPlan, plan,
};
use elzevir_app::matching::Matching;
use elzevir_app::patron_account::log_in;
use elzevir_app::patrons::{CategoryChange, PatronCommand, PatronOutput, PatronRefusal};
use elzevir_domain::catalog::MatchRule;
use elzevir_domain::patron::Enrolment;
use elzevir_domain::patron::PatronId;
use elzevir_store::CommandFailed;
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
            passwords: 1,
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

    // Koha's password signs in once and is then Argon2id.
    let identity = log_in(&store, "A0001", "Senha fictícia 1").unwrap();
    assert_eq!(identity.greeting, "Ana Maria");
    assert_eq!(
        rows(
            &store,
            "SELECT substr(password_hash, 1, 9) FROM patrons WHERE cardnumber = 'A0001'"
        ),
        ["$argon2id"]
    );
    assert!(log_in(&store, "A0001", "Senha fictícia 1").is_ok());
    assert!(log_in(&store, "A0006", "").is_err());

    // A second run adds nobody, keeps the new password and updates the branches.
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

/// The catalog as Koha exported it: two records with their `biblionumber` and Items with
/// their `itemnumber`, one only with its barcode.
const CATALOG: &str = "=LDR  00000nam a2200000 a 4500
=245  10$aPrimeiro livro fictício
=952  \\\\$9100$aCTR$bCTR$pB100
=952  \\\\$9101$aCTR$bCTR$pB101
=942  \\\\$cLIVRO
=999  \\\\$c10$d10

=LDR  00000nam a2200000 a 4500
=245  10$aSegundo livro fictício
=952  \\\\$9110$aCTR$bCTR$pB110
=952  \\\\$aNRT$bNRT$pB120
=942  \\\\$cLIVRO
=999  \\\\$c11$d11
";

fn circulate(store: &ElzevirStore, plan: &KohaPlan) -> KohaCirculationMigrated {
    let mut done = KohaCirculationMigrated::default();
    for batch in plan.circulation_batches() {
        let PatronOutput::KohaCirculationMigrated(write) = run(
            store,
            PatronCommand::MigrateKohaCirculation(Box::new(batch)),
        ) else {
            panic!("not a circulation migration");
        };
        done.add(write);
    }
    done
}

fn accounts(store: &ElzevirStore, plan: &KohaPlan) -> KohaAccountsMigrated {
    let mut done = KohaAccountsMigrated::default();
    for batch in plan.account_batches() {
        let PatronOutput::KohaAccountsMigrated(write) =
            run(store, PatronCommand::MigrateKohaAccounts(Box::new(batch)))
        else {
            panic!("not an account migration");
        };
        done.add(write);
    }
    done
}

#[test]
fn loans_holds_and_accounts_follow_the_patrons_and_the_catalog() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(&dir);
    let plan = the_plan();
    migrate(&store, &plan);
    store
        .submit(WriteCommand::Catalog(CatalogCommand::NameCodes(vec![
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
    assert_eq!((loaded.imported, loaded.items), (2, 4), "{loaded:?}");

    let done = circulate(&store, &plan);
    assert_eq!(
        done,
        KohaCirculationMigrated {
            loans: 2,
            holds: 4,
            already_migrated: 0,
            loans_left: vec![
                (3, CirculationLeftover::Patron),
                (4, CirculationLeftover::Item),
                (5, CirculationLeftover::ItemOnLoan),
            ],
            holds_left: vec![
                (6, CirculationLeftover::Branch),
                (5, CirculationLeftover::Record),
            ],
        }
    );
    assert_eq!(
        rows(
            &store,
            "SELECT i.barcode || '|' || p.cardnumber || '|' || l.due || '|' || l.renewals
                 || '|' || l.on_site
             FROM loans l JOIN items i ON i.id = l.item_id JOIN patrons p ON p.id = l.patron_id
             ORDER BY i.barcode"
        ),
        [
            "B100|A0001|2026-12-10T23:59:00-03:00[America/Sao_Paulo]|1|0",
            // Found by its barcode, lent on site at the other branch.
            "B120|A0006|2026-11-27T18:00:00-03:00[America/Sao_Paulo]|0|1",
        ]
    );
    assert_eq!(
        rows(
            &store,
            "SELECT p.cardnumber || '|' || h.priority || '|' || coalesce(h.found, '-') || '|'
                 || coalesce(i.barcode, '-') || '|' || coalesce(h.expires_on, '-')
             FROM holds h JOIN patrons p ON p.id = h.patron_id
             LEFT JOIN items i ON i.id = h.item_id
             ORDER BY h.instance_id, h.priority"
        ),
        [
            "A0001|0|T|B101|-",
            "A0002|1|-|-|-",
            "A0006|2|-|-|-",
            "A0001|0|W|B110|2026-12-01",
        ]
    );
    // The Item set aside in transit travels to the pickup branch.
    assert_eq!(
        rows(
            &store,
            "SELECT t.reason || '|' || k.code FROM transfers t
             JOIN koha_codes k ON k.kind = 1 AND k.local_id = t.to_branch
             WHERE t.arrived_at IS NULL"
        ),
        ["Reserve|NRT"]
    );

    accounts_come_whole(&store, &plan);
    history_keeps_only_consenting_patrons(&store, &plan);

    // A second run adds nothing.
    assert_eq!(accounts(&store, &plan).already_migrated, 2);
    let again = circulate(&store, &plan);
    assert_eq!(
        (again.loans, again.holds, again.already_migrated),
        (0, 0, 6)
    );
    assert_eq!(rows(&store, "SELECT count(*) || '' FROM holds"), ["4"]);
    anonymizing_keeps_the_history_without_the_patron(&store, &dir);
}

/// The accounts, after the loans (circulacao-do-koha.md, items 9 to 12).
fn accounts_come_whole(store: &ElzevirStore, plan: &KohaPlan) {
    // The accounts, after the loans: the open fine follows its loan.
    let done = accounts(store, plan);
    assert_eq!(
        done,
        KohaAccountsMigrated {
            accounts: 2,
            lines: 5,
            offsets: 4,
            already_migrated: 0,
            accounts_left: vec![3],
        }
    );
    assert_eq!(
        rows(
            store,
            "SELECT p.cardnumber || '|' || coalesce(a.debit_type, a.credit_type) || '|'
                 || coalesce(a.status, '-') || '|' || a.amount || '|' || a.outstanding || '|'
                 || coalesce(i.barcode, '-') || '|' || (a.loan_id IS NOT NULL)
             FROM account_lines a JOIN patrons p ON p.id = a.patron_id
             LEFT JOIN items i ON i.id = a.item_id ORDER BY a.id"
        ),
        [
            "A0001|OVERDUE|UNRETURNED|200|200|B100|1",
            "A0001|PAYMENT|-|-100|0|-|0",
            "A0001|OVERDUE|RETURNED|300|200|B101|0",
            "A0006|MANUAL|-|33|33|-|0",
            "A0006|CREDIT|-|-100|-100|-|0",
        ]
    );
    assert_eq!(
        rows(
            store,
            "SELECT o.kind || '|' || coalesce(c.credit_type, '-') || '|'
                 || coalesce(d.debit_type || d.amount, '-') || '|' || o.amount
             FROM account_offsets o LEFT JOIN account_lines c ON c.id = o.credit_id
             LEFT JOIN account_lines d ON d.id = o.debit_id ORDER BY o.id"
        ),
        [
            "CREATE|-|OVERDUE200|200",
            "CREATE|PAYMENT|-|-100",
            "APPLY|PAYMENT|OVERDUE300|-100",
            "CREATE|-|OVERDUE300|300",
        ]
    );
}

fn history(store: &ElzevirStore, plan: &KohaPlan) -> KohaHistoryMigrated {
    let mut done = KohaHistoryMigrated::default();
    for batch in plan.history_batches() {
        let PatronOutput::KohaHistoryMigrated(write) =
            run(store, PatronCommand::MigrateKohaHistory(Box::new(batch)))
        else {
            panic!("not a history migration");
        };
        done.add(write);
    }
    done
}

/// The returned loans (circulacao-do-koha.md, items 14 to 16): the patron stays only when
/// they keep a reading history.
fn history_keeps_only_consenting_patrons(store: &ElzevirStore, plan: &KohaPlan) {
    assert_eq!(
        history(store, plan),
        KohaHistoryMigrated {
            returned: 4,
            with_patron: 1,
            already_migrated: 0,
            left: vec![(14, CirculationLeftover::Item)],
        }
    );
    assert_eq!(
        rows(
            store,
            "SELECT i.barcode || '|' || coalesce(p.cardnumber, '-') || '|' || r.returned_at
                 || '|' || k.code || '|' || r.renewals
             FROM returned_loans r JOIN items i ON i.id = r.item_id
             LEFT JOIN patrons p ON p.id = r.patron_id
             JOIN koha_codes k ON k.kind = 1 AND k.local_id = r.return_branch_id
             ORDER BY r.issued_at"
        ),
        [
            "B100|-|2026-08-10T10:00:00-03:00[America/Sao_Paulo]|CTR|0",
            "B101|-|2026-09-10T10:00:00-03:00[America/Sao_Paulo]|CTR|0",
            "B110|A0001|2026-10-10T16:00:00-03:00[America/Sao_Paulo]|NRT|1",
            // The school does not keep a reading history.
            "B110|-|2026-10-20T10:00:00-03:00[America/Sao_Paulo]|CTR|0",
        ]
    );
    assert_eq!(history(store, plan).already_migrated, 4);
}

/// docs/specs/circulacao/lgpd.md, items 1 to 7, on the migrated library: the patron's
/// data leaves the record, the history and the audit log; another patron's stays.
fn anonymizing_keeps_the_history_without_the_patron(store: &ElzevirStore, dir: &tempfile::TempDir) {
    let ana = PatronId(
        rows(
            store,
            "SELECT id || '' FROM patrons WHERE cardnumber = 'A0001'",
        )
        .first()
        .unwrap()
        .parse()
        .unwrap(),
    );
    let anonymize = || {
        store
            .submit(WriteCommand::Patrons(PatronCommand::Anonymize(ana)))
            .unwrap()
            .wait()
    };
    assert!(matches!(
        anonymize(),
        Err(CommandFailed::Rejected(WriteRefusal::Patrons(
            PatronRefusal::HasLoans
        )))
    ));
    // The desk takes the Item back and cancels the holds.
    rusqlite::Connection::open(dir.path().join("elzevir.db"))
        .unwrap()
        .execute_batch(&format!(
            "DELETE FROM loans WHERE patron_id = {0}; DELETE FROM holds WHERE patron_id = {0};",
            ana.0
        ))
        .unwrap();
    assert!(matches!(
        anonymize(),
        Ok(WriteOutput::Patrons(PatronOutput::Anonymized(_)))
    ));
    assert_eq!(
        rows(
            store,
            &format!(
                "SELECT cardnumber || '|' || surname || '|' || coalesce(first_name, '-') || '|'
                     || coalesce(email, '-') || '|' || coalesce(password_hash, '-') || '|'
                     || (anonymized_at IS NOT NULL)
                 FROM patrons WHERE id = {}",
                ana.0
            )
        ),
        [format!("anon-{}||-|-|-|1", ana.0)]
    );
    // The returned loan and the account stay, without a name.
    assert_eq!(
        rows(
            store,
            "SELECT count(*) || '|' || count(patron_id) FROM returned_loans"
        ),
        ["4|0"]
    );
    assert_eq!(
        rows(
            store,
            &format!(
                "SELECT count(*) || '' FROM account_lines WHERE patron_id = {}",
                ana.0
            )
        ),
        ["3"]
    );
    let log = rows(store, "SELECT command FROM audit_log").join("\n");
    for private in [
        "A0001",
        "Ana Maria",
        "Acácias",
        "ana@example.org",
        "$2a$08$",
    ] {
        assert!(!log.contains(private), "{private} still in the audit log");
    }
    // Another patron's data is untouched.
    assert!(log.contains("A0006") && log.contains("elisa@example.org"));
    assert!(matches!(
        anonymize(),
        Err(CommandFailed::Rejected(WriteRefusal::Patrons(
            PatronRefusal::UnknownPatron
        )))
    ));
}
