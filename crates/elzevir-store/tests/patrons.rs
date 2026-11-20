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
    PatronUpdate, ReadingHistory,
};
use elzevir_app::registries::{RegistryChange, RegistryEntry};
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
