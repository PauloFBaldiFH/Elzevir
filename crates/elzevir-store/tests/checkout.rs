// SPDX-License-Identifier: GPL-3.0-or-later
//! Checkouts through the real store (docs/specs/circulacao/emprestimo.md).
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "integration test: helper functions fail the test on error"
)]

use elzevir_app::calendar::{CalendarAction, CalendarChange, HolidayText};
use elzevir_app::catalog::{
    CatalogCommand, CatalogOutput, CatalogReader, ItemData, ItemStatus, NewInstance, NewItem,
    NewRecord, RecordItems,
};
use elzevir_app::circulation_rules::{
    CirculationCommand, CirculationOutput, CirculationRefusal, RuleChange,
};
use elzevir_app::codes::{CodeTable, NamedCode};
use elzevir_app::commands::{WriteCommand, WriteOutput, WriteRefusal};
use elzevir_app::loans::{CheckedOut, CheckoutRefusal, CheckoutRequest};
use elzevir_app::patrons::{
    CategoryChange, PatronCommand, PatronFields, PatronOutput, ReadingHistory,
};
use elzevir_domain::catalog::{BranchId, InstanceId};
use elzevir_domain::checkout::{Alert, Confirm, Impossible, TooManyReason};
use elzevir_domain::circulation_rules::RuleScopeKey;
use elzevir_domain::patron::{Enrolment, PatronId};
use elzevir_store::{CommandFailed, ElzevirHandler, ElzevirStore, StoreConfig};
use jiff::Zoned;
use jiff::civil::date;

fn submit(
    store: &ElzevirStore,
    command: WriteCommand,
) -> Result<WriteOutput, CommandFailed<WriteRefusal>> {
    store.submit(command).unwrap().wait()
}

struct Library {
    store: ElzevirStore,
    central: BranchId,
    annex: BranchId,
    patron: PatronId,
    instance: InstanceId,
}

fn library(dir: &tempfile::TempDir) -> Library {
    let store = ElzevirStore::open(
        &dir.path().join("elzevir.db"),
        &StoreConfig::default(),
        ElzevirHandler,
    )
    .unwrap();
    for (table, code, name) in [
        (CodeTable::Branch, "B1", "Central"),
        (CodeTable::Branch, "B2", "Anexo"),
        (CodeTable::ItemType, "P1", "Livros"),
    ] {
        submit(
            &store,
            WriteCommand::Catalog(CatalogCommand::NameCodes(vec![NamedCode {
                table,
                code: code.to_owned(),
                name: name.to_owned(),
            }])),
        )
        .unwrap();
    }
    let codes = store.codes().unwrap();
    let branch = |name: &str| {
        codes
            .branches
            .iter()
            .find(|branch| branch.name == name)
            .unwrap()
            .id
    };
    let (central, annex) = (branch("Central"), branch("Anexo"));
    let Ok(WriteOutput::Patrons(PatronOutput::CategoryChanged(Some(category)))) = submit(
        &store,
        WriteCommand::Patrons(PatronCommand::ChangeCategory(CategoryChange::Create {
            name: "Adulto".to_owned(),
            enrolment: Enrolment::Until(date(2026, 12, 31)),
        })),
    ) else {
        panic!("no category");
    };
    let Ok(WriteOutput::Patrons(PatronOutput::Added(patron))) = submit(
        &store,
        WriteCommand::Patrons(PatronCommand::Add(Box::new(PatronFields {
            cardnumber: "000123".to_owned(),
            surname: "Silva".to_owned(),
            first_name: Some("Ana".to_owned()),
            cpf: None,
            date_of_birth: None,
            email: None,
            phone: None,
            address: None,
            branch: central,
            category,
            enrolled: date(2026, 1, 31),
            expires: None,
            staff_note: None,
            reading_history: ReadingHistory::NotKept,
        }))),
    ) else {
        panic!("no patron");
    };
    let Ok(WriteOutput::Catalog(CatalogOutput::Record { instance, .. })) = submit(
        &store,
        WriteCommand::Catalog(CatalogCommand::AddRecord(Box::new(NewRecord {
            work_title: "Dom Casmurro".to_owned(),
            work_description: elzevir_domain::statement::Description::default(),
            hub: None,
            relations: Vec::new(),
            classification: None,
            classification_scheme: None,
            summary: None,
            audience: None,
            languages: Vec::new(),
            contributions: Vec::new(),
            subjects: Vec::new(),
            items: RecordItems::default(),
            source: None,
            instance: NewInstance {
                title: "Dom Casmurro".to_owned(),
                ..NewInstance::default()
            },
        }))),
    ) else {
        panic!("no record");
    };
    Library {
        store,
        central,
        annex,
        patron,
        instance,
    }
}

impl Library {
    fn item(&self, barcode: &str, status: ItemStatus) {
        submit(
            &self.store,
            WriteCommand::Catalog(CatalogCommand::AddItem(Box::new(NewItem {
                instance: self.instance,
                item: ItemData {
                    barcode: Some(barcode.to_owned()),
                    inventory_number: None,
                    home_branch: "B1".to_owned(),
                    holding_branch: "B1".to_owned(),
                    item_type: "P1".to_owned(),
                    call_number: None,
                    location: None,
                    collection: None,
                    price: None,
                    replacement_price: None,
                    copy_number: None,
                    status,
                    acquired_on: None,
                },
            }))),
        )
        .unwrap();
    }

    fn rules(&self, values: &[(&str, &str)]) {
        let values = values
            .iter()
            .map(|(rule, value)| ((*rule).to_owned(), Some((*value).to_owned())))
            .collect();
        submit(
            &self.store,
            WriteCommand::Circulation(CirculationCommand::SetRules(RuleChange {
                scope: RuleScopeKey::default(),
                values,
            })),
        )
        .unwrap();
    }

    fn lend(
        &self,
        barcode: &str,
        at: &str,
        confirmed: bool,
    ) -> Result<CheckedOut, CheckoutRefusal> {
        self.lend_to(self.patron, barcode, at, confirmed)
    }

    fn lend_to(
        &self,
        patron: PatronId,
        barcode: &str,
        at: &str,
        confirmed: bool,
    ) -> Result<CheckedOut, CheckoutRefusal> {
        let at: Zoned = format!("{at}[America/Sao_Paulo]").parse().unwrap();
        match submit(
            &self.store,
            WriteCommand::Circulation(CirculationCommand::Checkout(CheckoutRequest {
                patron,
                barcode: barcode.to_owned(),
                desk: self.annex,
                on_site: false,
                confirmed,
                at,
            })),
        ) {
            Ok(WriteOutput::Circulation(CirculationOutput::CheckedOut(done))) => Ok(done),
            Err(CommandFailed::Rejected(WriteRefusal::Circulation(
                CirculationRefusal::Checkout(refusal),
            ))) => Err(refusal),
            other => panic!("{other:?}"),
        }
    }

    fn read<T: rusqlite::types::FromSql>(&self, sql: &str) -> T {
        self.store
            .readers()
            .read(|c| c.query_row(sql, [], |row| row.get(0)))
            .unwrap()
    }
}

#[test]
fn an_item_is_lent_until_the_due_date_of_its_rules_and_calendar() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(&dir);
    library.item("0001", ItemStatus::default());
    library.rules(&[("maxissueqty", "5"), ("issuelength", "14")]);
    // Sundays closed at the branch that holds the Item; the desk's calendar does not count.
    submit(
        &library.store,
        WriteCommand::Circulation(CirculationCommand::ChangeCalendar(CalendarChange {
            branch: library.central,
            action: CalendarAction::CloseWeekday {
                weekday: 0,
                text: HolidayText {
                    title: "Domingo".to_owned(),
                    description: None,
                },
            },
        })),
    )
    .unwrap();

    let done = library
        .lend("  0001 ", "2026-11-19T10:00:00-03:00", false)
        .unwrap();
    // Fourteen open days, Sundays skipped (`useDaysMode` Calendar), at 23:59.
    assert_eq!(
        done.due.to_string(),
        "2026-12-05T23:59:00-03:00[America/Sao_Paulo]"
    );
    assert!(done.alerts.is_empty());
    assert_eq!(
        library.read::<i64>("SELECT count(*) FROM loans"),
        1,
        "one loan"
    );
    // The Item is now at the desk's branch.
    assert_eq!(
        library.read::<i64>("SELECT holding_branch FROM items WHERE barcode = '0001'"),
        library.annex.0
    );
}

#[test]
fn a_lent_item_asks_to_renew_and_is_not_lent_again() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(&dir);
    library.item("0001", ItemStatus::default());
    library.rules(&[("maxissueqty", "5"), ("issuelength", "7")]);
    let at = "2026-11-19T10:00:00-03:00";
    library.lend("0001", at, false).unwrap();
    assert_eq!(
        library.lend("0001", at, false),
        Err(CheckoutRefusal::NeedsConfirmation(vec![Confirm::Renew]))
    );
    assert_eq!(library.lend("0001", at, true), Err(CheckoutRefusal::OnLoan));
}

#[test]
fn the_loan_limit_asks_for_confirmation() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(&dir);
    library.item("0001", ItemStatus::default());
    library.item("0002", ItemStatus::default());
    library.rules(&[("maxissueqty", "1"), ("issuelength", "7")]);
    let at = "2026-11-19T10:00:00-03:00";
    library.lend("0001", at, false).unwrap();
    // The first loan counts at its Item's new holding branch, the desk's, and the rule
    // for any branch counts every loan.
    let Err(CheckoutRefusal::NeedsConfirmation(confirm)) = library.lend("0002", at, false) else {
        panic!("no confirmation asked");
    };
    let [Confirm::TooMany(too_many)] = confirm.as_slice() else {
        panic!("{confirm:?}");
    };
    assert_eq!(too_many.reason, TooManyReason::Checkouts);
    assert_eq!((too_many.count, too_many.max_allowed.as_str()), (1, "1"));
    assert!(library.lend("0002", at, true).is_ok());
}

#[test]
fn without_rules_every_checkout_needs_confirmation() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(&dir);
    library.item("0001", ItemStatus::default());
    let Err(CheckoutRefusal::NeedsConfirmation(confirm)) =
        library.lend("0001", "2026-11-19T10:00:00-03:00", false)
    else {
        panic!("no confirmation asked");
    };
    assert_eq!(confirm.first(), Some(&Confirm::PatronCannotBorrow));
    assert!(matches!(
        confirm.get(1),
        Some(Confirm::TooMany(too_many)) if too_many.reason == TooManyReason::NoRuleDefined
    ));
}

#[test]
fn the_patron_and_the_item_can_stop_a_checkout() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(&dir);
    library.rules(&[("maxissueqty", "5"), ("issuelength", "7")]);
    library.item(
        "0001",
        ItemStatus {
            withdrawn: 1,
            not_for_loan: 2,
            ..ItemStatus::default()
        },
    );
    library.item(
        "0002",
        ItemStatus {
            lost: 1,
            ..ItemStatus::default()
        },
    );
    library.item("0003", ItemStatus::default());
    assert_eq!(
        library.lend("0001", "2026-11-19T10:00:00-03:00", true),
        Err(CheckoutRefusal::Impossible(vec![
            Impossible::NotForLoan(2),
            Impossible::Withdrawn
        ]))
    );
    // The enrolment ends on 2026-12-31: on the next day the patron has expired.
    assert_eq!(
        library.lend("0003", "2027-01-01T10:00:00-03:00", true),
        Err(CheckoutRefusal::Impossible(vec![Impossible::PatronExpired]))
    );
    assert_eq!(
        library.lend("9999", "2026-11-19T10:00:00-03:00", true),
        Err(CheckoutRefusal::UnknownBarcode)
    );
    assert_eq!(
        library.lend_to(PatronId(999), "0003", "2026-11-19T10:00:00-03:00", true),
        Err(CheckoutRefusal::UnknownPatron)
    );
    assert_eq!(library.read::<i64>("SELECT count(*) FROM loans"), 0);
    // A lost Item is lent with an alert, and is found.
    let done = library
        .lend("0002", "2026-11-19T10:00:00-03:00", false)
        .unwrap();
    assert_eq!(done.alerts, [Alert::Lost(1)]);
    assert_eq!(
        library.read::<i64>("SELECT lost FROM items WHERE barcode = '0002'"),
        0
    );
}
