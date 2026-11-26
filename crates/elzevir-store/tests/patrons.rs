// SPDX-License-Identifier: GPL-3.0-or-later
//! Patrons and patron categories through the real command handler
//! (docs/specs/circulacao/leitores.md).
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "integration test: helper functions fail the test on error"
)]

use elzevir_app::catalog::CatalogCommand;
use elzevir_app::commands::{WriteCommand, WriteOutput, WriteRefusal};
use elzevir_app::patron_account::PatronAccounts;
use elzevir_app::patrons::{
    CategoryChange, PatronCommand, PatronField, PatronFields, PatronOutput, PatronRefusal,
    PatronUpdate, ReadingHistory, Retention, RetentionApplied,
};
use elzevir_app::registries::{RegistryChange, RegistryEntry};
use elzevir_app::staff::hash_password;
use elzevir_domain::catalog::BranchId;
use elzevir_domain::patron::{Cpf, Enrolment, PatronCategoryId, PatronId};
use elzevir_store::{CommandFailed, ElzevirHandler, ElzevirStore, StoreConfig};
use jiff::civil::date;

type Outcome = Result<PatronOutput, CommandFailed<WriteRefusal>>;

fn open(dir: &tempfile::TempDir) -> ElzevirStore {
    ElzevirStore::open(
        &dir.path().join("elzevir.db"),
        &StoreConfig::default(),
        ElzevirHandler,
    )
    .unwrap()
}

fn run(store: &ElzevirStore, command: PatronCommand) -> Outcome {
    store
        .submit_as(7, WriteCommand::Patrons(command))
        .unwrap()
        .wait()
        .map(|output| {
            let WriteOutput::Patrons(output) = output else {
                panic!("unexpected output {output:?}");
            };
            output
        })
}

fn refusal(outcome: Outcome) -> PatronRefusal {
    match outcome {
        Err(CommandFailed::Rejected(WriteRefusal::Patrons(refusal))) => refusal,
        other => panic!("not a refusal: {other:?}"),
    }
}

fn read<T: rusqlite::types::FromSql>(store: &ElzevirStore, sql: &str) -> T {
    store
        .readers()
        .read(|c| c.query_row(sql, [], |row| row.get(0)))
        .unwrap()
}

/// A store with one branch and a category of 12 months.
fn library(dir: &tempfile::TempDir) -> (ElzevirStore, BranchId, PatronCategoryId) {
    let store = open(dir);
    let branch = store
        .submit(WriteCommand::Catalog(CatalogCommand::CreateBranch {
            name: "Biblioteca Central".to_owned(),
        }))
        .unwrap()
        .wait()
        .unwrap();
    let WriteOutput::Catalog(elzevir_app::catalog::CatalogOutput::Branch(branch)) = branch else {
        panic!("no branch: {branch:?}");
    };
    let category = category(&store, "Estudante", Enrolment::Months(12));
    (store, branch, category)
}

fn category(store: &ElzevirStore, name: &str, enrolment: Enrolment) -> PatronCategoryId {
    let PatronOutput::CategoryChanged(Some(category)) = run(
        store,
        PatronCommand::ChangeCategory(CategoryChange::Create {
            name: name.to_owned(),
            enrolment,
        }),
    )
    .unwrap() else {
        panic!("no category");
    };
    category
}

fn fields(branch: BranchId, category: PatronCategoryId) -> PatronFields {
    PatronFields {
        cardnumber: "000123".to_owned(),
        surname: "Silva".to_owned(),
        first_name: Some("Ana".to_owned()),
        cpf: Some(Cpf::parse("529.982.247-25").unwrap()),
        date_of_birth: Some(date(2010, 5, 1)),
        email: None,
        phone: None,
        address: None,
        branch,
        category,
        enrolled: date(2026, 1, 31),
        expires: None,
        staff_note: None,
        reading_history: ReadingHistory::NotKept,
    }
}

fn add(store: &ElzevirStore, fields: PatronFields) -> Outcome {
    run(store, PatronCommand::Add(Box::new(fields)))
}

#[test]
fn patron_password_changes_audit_the_actor_action_and_target_without_the_hash() {
    let dir = tempfile::tempdir().unwrap();
    let (store, branch, category) = library(&dir);
    let PatronOutput::Added(patron) = add(&store, fields(branch, category)).unwrap() else {
        panic!("not added");
    };
    let hash = hash_password("test-patron-password").unwrap();
    for password_hash in [Some(hash.clone()), None] {
        let access_enabled = password_hash.is_some();
        assert_eq!(
            run(
                &store,
                PatronCommand::SetPassword {
                    patron,
                    password_hash
                },
            ),
            Ok(PatronOutput::PasswordSet(patron))
        );
        let (actor, payload): (i64, String) = store
            .readers()
            .read(|c| {
                c.query_row(
                    "SELECT actor, command FROM audit_log WHERE kind = 'patrons.set_password'
                     ORDER BY seq DESC LIMIT 1",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
            })
            .unwrap();
        assert_eq!(actor, 7);
        assert!(!payload.contains(&hash));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&payload).unwrap(),
            serde_json::json!({"target": patron.0, "access_enabled": access_enabled})
        );
    }
}

#[test]
fn a_patron_is_enrolled_until_the_end_of_the_categorys_period() {
    let dir = tempfile::tempdir().unwrap();
    let (store, branch, _) = library(&dir);
    let monthly = category(&store, "Visitante", Enrolment::Months(1));
    let PatronOutput::Added(patron) = add(&store, fields(branch, monthly)).unwrap() else {
        panic!("not added");
    };
    let row = format!("FROM patrons WHERE id = {}", patron.0);
    assert_eq!(
        read::<String>(&store, &format!("SELECT expires {row}")),
        "2026-02-28"
    );
    assert_eq!(
        read::<String>(&store, &format!("SELECT cpf {row}")),
        "52998224725"
    );
    // A fixed date and a date typed in win over the period.
    let fixed = category(&store, "Projeto", Enrolment::Until(date(2026, 12, 31)));
    let mut other = fields(branch, fixed);
    other.cardnumber = "000124".to_owned();
    other.cpf = None;
    let PatronOutput::Added(second) = add(&store, other.clone()).unwrap() else {
        panic!("not added");
    };
    assert_eq!(
        read::<String>(
            &store,
            &format!("SELECT expires FROM patrons WHERE id = {}", second.0)
        ),
        "2026-12-31"
    );
    other.cardnumber = "000125".to_owned();
    other.expires = Some(date(2027, 6, 30));
    add(&store, other).unwrap();
    assert_eq!(
        read::<String>(&store, "SELECT max(expires) FROM patrons"),
        "2027-06-30"
    );
    // Who enrolled them is in the audit log.
    assert_eq!(
        read::<i64>(
            &store,
            "SELECT count(*) FROM audit_log WHERE kind = 'patrons.add' AND actor = 7"
        ),
        3
    );
}

#[test]
fn card_numbers_and_cpfs_are_unique_and_references_must_exist() {
    let dir = tempfile::tempdir().unwrap();
    let (store, branch, category) = library(&dir);
    add(&store, fields(branch, category)).unwrap();
    let mut same_card = fields(branch, category);
    same_card.cardnumber = " 000123 ".to_owned();
    same_card.cpf = None;
    assert_eq!(
        refusal(add(&store, same_card)),
        PatronRefusal::CardnumberTaken
    );
    let mut same_cpf = fields(branch, category);
    same_cpf.cardnumber = "999".to_owned();
    assert_eq!(refusal(add(&store, same_cpf)), PatronRefusal::CpfTaken);
    let mut nowhere = fields(BranchId(99), category);
    nowhere.cardnumber = "999".to_owned();
    assert_eq!(refusal(add(&store, nowhere)), PatronRefusal::UnknownBranch);
    let mut no_category = fields(branch, PatronCategoryId(99));
    no_category.cardnumber = "999".to_owned();
    assert_eq!(
        refusal(add(&store, no_category)),
        PatronRefusal::UnknownCategory
    );
    let mut no_surname = fields(branch, category);
    no_surname.surname = " ".to_owned();
    assert_eq!(
        refusal(add(&store, no_surname)),
        PatronRefusal::BadField(PatronField::Surname)
    );
    assert_eq!(read::<i64>(&store, "SELECT count(*) FROM patrons"), 1);
}

#[test]
fn an_edit_needs_the_version_it_read() {
    let dir = tempfile::tempdir().unwrap();
    let (store, branch, category) = library(&dir);
    let PatronOutput::Added(patron) = add(&store, fields(branch, category)).unwrap() else {
        panic!("not added");
    };
    let mut changed = fields(branch, category);
    changed.email = Some("ana@example.org".to_owned());
    changed.reading_history = ReadingHistory::Kept;
    let update = |row_version, fields: PatronFields| {
        run(
            &store,
            PatronCommand::Update(Box::new(PatronUpdate {
                patron,
                row_version,
                fields,
            })),
        )
    };
    assert_eq!(
        update(1, changed.clone()).unwrap(),
        PatronOutput::Updated(patron)
    );
    assert_eq!(
        read::<String>(&store, "SELECT email FROM patrons"),
        "ana@example.org"
    );
    assert_eq!(
        read::<i64>(&store, "SELECT reading_history FROM patrons"),
        1
    );
    assert_eq!(read::<i64>(&store, "SELECT row_version FROM patrons"), 2);
    // Keeping its own card number and CPF is no conflict; reading an old version is.
    assert_eq!(refusal(update(1, changed.clone())), PatronRefusal::Stale);
    assert_eq!(
        refusal(run(
            &store,
            PatronCommand::Update(Box::new(PatronUpdate {
                patron: PatronId(99),
                row_version: 1,
                fields: changed,
            })),
        )),
        PatronRefusal::UnknownPatron
    );
}

#[test]
fn the_patron_chooses_their_reading_history_with_no_staff_actor() {
    let dir = tempfile::tempdir().unwrap();
    let (store, branch, category) = library(&dir);
    let PatronOutput::Added(patron) = add(&store, fields(branch, category)).unwrap() else {
        panic!("not added");
    };
    store
        .set_reading_history(patron, ReadingHistory::Kept)
        .unwrap();
    assert_eq!(
        store.account(patron).unwrap().unwrap().reading_history,
        ReadingHistory::Kept
    );
    assert_eq!(
        read::<String>(
            &store,
            "SELECT reading_history || ' ' || row_version FROM patrons"
        ),
        "1 2"
    );
    assert_eq!(
        read::<i64>(
            &store,
            "SELECT count(*) FROM audit_log
             WHERE kind = 'patrons.set_reading_history' AND actor IS NULL"
        ),
        1
    );
    store
        .set_reading_history(patron, ReadingHistory::NotKept)
        .unwrap();
    assert_eq!(
        read::<i64>(&store, "SELECT reading_history FROM patrons"),
        0
    );
    assert!(
        store
            .set_reading_history(PatronId(99), ReadingHistory::Kept)
            .is_err()
    );
}

#[test]
fn a_category_with_patrons_or_a_branch_with_patrons_stays() {
    let dir = tempfile::tempdir().unwrap();
    let (store, branch, category) = library(&dir);
    let other = category_named(&store, "Professor");
    assert_eq!(
        refusal(run(
            &store,
            PatronCommand::ChangeCategory(CategoryChange::Update {
                category: other,
                name: "Estudante".to_owned(),
                enrolment: Enrolment::Months(24),
            }),
        )),
        PatronRefusal::CategoryNameTaken
    );
    add(&store, fields(branch, category)).unwrap();
    assert_eq!(
        refusal(run(
            &store,
            PatronCommand::ChangeCategory(CategoryChange::Delete(category)),
        )),
        PatronRefusal::CategoryInUse
    );
    assert_eq!(
        run(
            &store,
            PatronCommand::ChangeCategory(CategoryChange::Delete(other)),
        )
        .unwrap(),
        PatronOutput::CategoryChanged(None)
    );
    let branch_deleted = store
        .submit(WriteCommand::Catalog(CatalogCommand::ChangeRegistry(
            RegistryChange::Delete(RegistryEntry::Branch(branch)),
        )))
        .unwrap()
        .wait();
    assert!(
        matches!(branch_deleted, Err(CommandFailed::Rejected(_))),
        "{branch_deleted:?}"
    );
}

fn category_named(store: &ElzevirStore, name: &str) -> PatronCategoryId {
    category(store, name, Enrolment::Months(12))
}

#[test]
fn a_search_finds_card_numbers_cpfs_and_parts_of_names_and_views_are_logged() {
    use elzevir_app::patrons::{PatronDesk, PatronQuery};
    use elzevir_app::staff::StaffId;
    let dir = tempfile::tempdir().unwrap();
    let (store, branch, category) = library(&dir);
    let PatronOutput::Added(ana) = add(&store, fields(branch, category)).unwrap() else {
        panic!("not added");
    };
    let mut bruno = fields(branch, category);
    bruno.cardnumber = "000200".to_owned();
    bruno.surname = "Souza".to_owned();
    bruno.first_name = Some("Bruno_".to_owned());
    bruno.cpf = None;
    add(&store, bruno).unwrap();
    let found = |text: &str| -> Vec<String> {
        store
            .search(&PatronQuery::parse(text).unwrap())
            .unwrap()
            .into_iter()
            .map(|row| row.cardnumber)
            .collect()
    };
    assert_eq!(found("000123"), ["000123"]);
    assert_eq!(found("529.982.247-25"), ["000123"]);
    assert_eq!(found("ana silva"), ["000123"]);
    assert_eq!(found("silva ana"), ["000123"]);
    assert_eq!(found("S"), Vec::<String>::new());
    assert_eq!(found("so"), ["000200"]);
    // `_` is a letter here, not a wildcard.
    assert_eq!(found("o_"), ["000200"]);
    let record = store.patron(ana).unwrap().unwrap();
    assert_eq!(record.category_name, "Estudante");
    assert_eq!(record.fields.expires, Some(date(2027, 1, 31)));
    assert_eq!(record.fields.cpf.unwrap().masked(), "***.982.247-**");
    assert_eq!(store.categories().unwrap()[0].patrons, 2);
    assert_eq!(
        store.apply(StaffId(3), PatronCommand::View(ana)).unwrap(),
        PatronOutput::Viewed(ana)
    );
    assert_eq!(
        read::<i64>(
            &store,
            "SELECT count(*) FROM audit_log WHERE kind = 'patrons.view' AND actor = 3"
        ),
        1
    );
}

#[test]
fn anonymizing_follows_a_changed_card_number_and_spares_a_namesake() {
    // docs/specs/circulacao/lgpd.md, item 5.
    let dir = tempfile::tempdir().unwrap();
    let (store, branch, category) = library(&dir);
    let PatronOutput::Added(ana) = add(&store, fields(branch, category)).unwrap() else {
        panic!("not added");
    };
    let mut namesake = fields(branch, category);
    namesake.cardnumber = "000200".to_owned();
    namesake.cpf = None;
    namesake.first_name = Some("Bruno".to_owned());
    add(&store, namesake).unwrap();
    // A new card number: the first enrolment is found through the CPF both share.
    let mut changed = fields(branch, category);
    changed.cardnumber = "000124".to_owned();
    changed.email = Some("ana@example.org".to_owned());
    run(
        &store,
        PatronCommand::Update(Box::new(PatronUpdate {
            patron: ana,
            row_version: 1,
            fields: changed,
        })),
    )
    .unwrap();
    assert_eq!(
        run(&store, PatronCommand::Anonymize(ana)).unwrap(),
        PatronOutput::Anonymized(ana)
    );
    let log: String = read(
        &store,
        "SELECT group_concat(command, char(10)) FROM audit_log",
    );
    for private in ["000123", "000124", "52998224725", "Ana", "ana@example.org"] {
        assert!(!log.contains(private), "{private} still in the audit log");
    }
    assert!(log.contains("000200") && log.contains("Bruno") && log.contains("Silva"));
    assert!(log.contains("patrons.anonymize") || log.contains("Anonymize"));
    assert_eq!(
        refusal(run(&store, PatronCommand::Anonymize(ana))),
        PatronRefusal::UnknownPatron
    );
}

#[test]
fn retention_drops_old_history_and_anonymizes_long_expired_patrons_once_a_day() {
    // docs/specs/circulacao/lgpd.md, items 12 to 16.
    let dir = tempfile::tempdir().unwrap();
    let (store, branch, category) = library(&dir);
    let patron = |card: &str, expires: Option<jiff::civil::Date>| {
        let mut fields = fields(branch, category);
        fields.cardnumber = card.to_owned();
        fields.cpf = None;
        fields.enrolled = date(2023, 1, 31);
        fields.expires = expires;
        fields.reading_history = ReadingHistory::Kept;
        let PatronOutput::Added(id) = add(&store, fields).unwrap() else {
            panic!("not added");
        };
        id.0
    };
    let ana = patron("000301", Some(date(2027, 12, 31)));
    let long_ago = Some(date(2024, 1, 31));
    let bruno = patron("000302", long_ago);
    let carla = patron("000303", long_ago);
    let elisa = patron("000304", long_ago);
    // Expired, but not long enough.
    patron("000305", Some(date(2026, 6, 30)));
    // Rows put straight in, without their Items: only the patron and the dates matter here.
    rusqlite::Connection::open(dir.path().join("elzevir.db"))
        .unwrap()
        .execute_batch(&format!(
            "PRAGMA foreign_keys = OFF;
             INSERT INTO returned_loans (item_id, patron_id, branch_id, issued_at, due, on_site,
                                         returned_at, return_branch_id)
             VALUES (1, {ana}, {b}, '2024-01-02', '2024-01-16', 0,
                     '2024-01-10T10:00:00-03:00[America/Sao_Paulo]', {b}),
                    (2, {ana}, {b}, '2026-11-02', '2026-11-16', 0,
                     '2026-11-20T10:00:00-03:00[America/Sao_Paulo]', {b});
             INSERT INTO ended_holds (patron_id, instance_id, pickup_id, placed_on, ended_at,
                                      outcome)
             VALUES ({ana}, 1, {b}, '2024-01-01', '2024-02-01T09:00:00-03:00[America/Sao_Paulo]',
                     'CANCELLED');
             INSERT INTO account_lines (patron_id, debit_type, amount, outstanding, branch_id,
                                        created_at, updated_at)
             VALUES ({carla}, 'OVERDUE', 500, 500, {b}, '2024-01-01', '2024-01-01');
             INSERT INTO loans (item_id, patron_id, branch_id, issued_at, due, on_site)
             VALUES (3, {elisa}, {b}, '2024-01-02', '2024-01-16', 0);",
            b = branch.0
        ))
        .unwrap();
    let retention = |today| {
        run(
            &store,
            PatronCommand::ApplyRetention {
                today,
                retention: Retention {
                    reading_history_days: Some(365),
                    anonymize_expired_days: Some(365),
                },
            },
        )
        .unwrap()
    };
    assert_eq!(
        retention(date(2026, 11, 26)),
        PatronOutput::RetentionApplied(RetentionApplied {
            ran: true,
            history: 2,
            anonymized: 1,
            kept: 2,
        })
    );
    let kept: String = read(
        &store,
        "SELECT group_concat(returned_at, '|') FROM returned_loans WHERE patron_id IS NOT NULL",
    );
    assert!(kept.starts_with("2026-11-20"));
    let anonymized: String = read(
        &store,
        "SELECT group_concat(id, ',') FROM patrons WHERE anonymized_at IS NOT NULL",
    );
    assert_eq!(anonymized, bruno.to_string());
    // The same day again does nothing; the next day runs again.
    assert_eq!(
        retention(date(2026, 11, 26)),
        PatronOutput::RetentionApplied(RetentionApplied::default())
    );
    assert_eq!(
        retention(date(2026, 11, 27)),
        PatronOutput::RetentionApplied(RetentionApplied {
            ran: true,
            history: 0,
            anonymized: 0,
            kept: 2,
        })
    );
}
