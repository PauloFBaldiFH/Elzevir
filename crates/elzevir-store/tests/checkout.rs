// SPDX-License-Identifier: GPL-3.0-or-later
//! Checkouts and checkins through the real store (docs/specs/circulacao/emprestimo.md,
//! docs/specs/circulacao/devolucao.md, docs/specs/circulacao/suspensao.md).
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "integration test: helper functions fail the test on error"
)]

use elzevir_app::accounts::{AccountLineId, Paid, PayRefusal, PaymentRequest};
use elzevir_app::calendar::{CalendarAction, CalendarChange, HolidayText};
use elzevir_app::catalog::{
    CatalogCommand, CatalogOutput, CatalogReader, ItemData, ItemStatus, NewInstance, NewItem,
    NewRecord, RecordItems,
};
use elzevir_app::circulation_rules::{
    CirculationCommand, CirculationOutput, CirculationRefusal, LongOverdueMarked, OverduesCharged,
    RuleChange,
};
use elzevir_app::codes::{CodeTable, NamedCode};
use elzevir_app::commands::{WriteCommand, WriteOutput, WriteRefusal};
use elzevir_app::holds::{
    CheckinHold, HoldAside, HoldCancellation, HoldId, HoldLevel, HoldPlaced, HoldRefused,
    HoldRequest, SetAside,
};
use elzevir_app::loans::{
    CheckedIn, CheckedOut, CheckinRefusal, CheckinRequest, CheckoutRefusal, CheckoutRequest,
    LoanDesk, RenewRefusal, RenewRequest, Renewed,
};
use elzevir_app::patrons::{
    CategoryChange, PatronCommand, PatronFields, PatronOutput, ReadingHistory,
};
use elzevir_domain::account::{CreditType, PaymentRefusal};
use elzevir_domain::catalog::{BranchId, InstanceId, Money};
use elzevir_domain::checkin::{Destination, LoanOutcome, LostOutcome, TransferReason};
use elzevir_domain::checkout::{Alert, Confirm, Impossible, TooManyReason};
use elzevir_domain::circulation_rules::RuleScopeKey;
use elzevir_domain::hold::{Found, HoldMatch, HoldRefusal};
use elzevir_domain::long_overdue::{DEFAULT_MAX_DAYS, LongOverdueSettings};
use elzevir_domain::patron::{Enrolment, PatronId};
use elzevir_domain::renewal::RenewalRefusal;
use elzevir_domain::suspension::SuspensionEnd;
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
        self.priced_item(barcode, status, None);
    }

    fn priced_item(&self, barcode: &str, status: ItemStatus, replacement_price: Option<Money>) {
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
                    replacement_price,
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

    fn give_back(
        &self,
        barcode: &str,
        desk: BranchId,
        at: &str,
    ) -> Result<CheckedIn, CheckinRefusal> {
        let at: Zoned = format!("{at}[America/Sao_Paulo]").parse().unwrap();
        match submit(
            &self.store,
            WriteCommand::Circulation(CirculationCommand::Checkin(CheckinRequest {
                barcode: barcode.to_owned(),
                desk,
                at,
            })),
        ) {
            Ok(WriteOutput::Circulation(CirculationOutput::CheckedIn(done))) => Ok(done),
            Err(CommandFailed::Rejected(WriteRefusal::Circulation(
                CirculationRefusal::Checkin(refusal),
            ))) => Err(refusal),
            other => panic!("{other:?}"),
        }
    }

    fn renew(
        &self,
        barcode: &str,
        at: &str,
        override_limit: bool,
    ) -> Result<Renewed, RenewRefusal> {
        let at: Zoned = format!("{at}[America/Sao_Paulo]").parse().unwrap();
        match submit(
            &self.store,
            WriteCommand::Circulation(CirculationCommand::Renew(RenewRequest {
                barcode: barcode.to_owned(),
                override_limit,
                at,
            })),
        ) {
            Ok(WriteOutput::Circulation(CirculationOutput::Renewed(done))) => Ok(done),
            Err(CommandFailed::Rejected(WriteRefusal::Circulation(CirculationRefusal::Renew(
                refusal,
            )))) => Err(refusal),
            other => panic!("{other:?}"),
        }
    }

    /// Another patron of the same category, at `branch`.
    fn another_patron(&self, cardnumber: &str, branch: BranchId) -> PatronId {
        let category: i64 = self.read("SELECT category_id FROM patrons LIMIT 1");
        let Ok(WriteOutput::Patrons(PatronOutput::Added(patron))) = submit(
            &self.store,
            WriteCommand::Patrons(PatronCommand::Add(Box::new(PatronFields {
                cardnumber: cardnumber.to_owned(),
                surname: "Souza".to_owned(),
                first_name: None,
                cpf: None,
                date_of_birth: None,
                email: None,
                phone: None,
                address: None,
                branch,
                category: elzevir_domain::patron::PatronCategoryId(category),
                enrolled: date(2026, 1, 31),
                expires: None,
                staff_note: None,
                reading_history: ReadingHistory::NotKept,
            }))),
        ) else {
            panic!("no patron");
        };
        patron
    }

    fn hold(
        &self,
        patron: PatronId,
        barcode: &str,
        level: HoldLevel,
        pickup: BranchId,
    ) -> Result<HoldPlaced, HoldRefused> {
        let at: Zoned = "2026-11-20T10:00:00-03:00[America/Sao_Paulo]"
            .parse()
            .unwrap();
        match submit(
            &self.store,
            WriteCommand::Circulation(CirculationCommand::PlaceHold(HoldRequest {
                patron,
                barcode: barcode.to_owned(),
                level,
                pickup,
                at,
            })),
        ) {
            Ok(WriteOutput::Circulation(CirculationOutput::HoldPlaced(placed))) => Ok(placed),
            Err(CommandFailed::Rejected(WriteRefusal::Circulation(CirculationRefusal::Hold(
                refusal,
            )))) => Err(refusal),
            other => panic!("{other:?}"),
        }
    }

    fn set_aside(
        &self,
        hold: HoldId,
        barcode: &str,
        (desk, at): (BranchId, &str),
    ) -> Result<SetAside, HoldRefused> {
        match submit(
            &self.store,
            WriteCommand::Circulation(CirculationCommand::SetHoldAside(HoldAside {
                hold,
                barcode: barcode.to_owned(),
                desk,
                at: format!("{at}[America/Sao_Paulo]").parse().unwrap(),
            })),
        ) {
            Ok(WriteOutput::Circulation(CirculationOutput::HoldSetAside(done))) => Ok(done),
            Err(CommandFailed::Rejected(WriteRefusal::Circulation(CirculationRefusal::Hold(
                refusal,
            )))) => Err(refusal),
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

#[test]
fn a_returned_item_goes_home_and_its_transfer_arrives() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(&dir);
    library.item("0001", ItemStatus::default());
    library.rules(&[("maxissueqty", "5"), ("issuelength", "7")]);
    // Lent at the annex desk, so the Item is held there.
    library
        .lend("0001", "2026-11-19T10:00:00-03:00", false)
        .unwrap();
    let back = library
        .give_back("0001", library.annex, "2026-11-20T10:00:00-03:00")
        .unwrap();
    assert_eq!(
        back,
        CheckedIn {
            patron: Some(library.patron),
            loan: LoanOutcome::Ended { overdue: false },
            lost: LostOutcome::NotLost,
            destination: Destination::Sent {
                to: library.central,
                reason: TransferReason::ReturnToHome,
            },
            suspended_until: None,
            fine: None,
            hold: None,
        }
    );
    assert_eq!(library.read::<i64>("SELECT count(*) FROM loans"), 0);
    // The patron keeps no reading history, so the ended loan forgets who borrowed.
    assert_eq!(
        library.read::<i64>("SELECT count(*) FROM returned_loans WHERE patron_id IS NULL"),
        1
    );
    let arrived = library
        .give_back("0001", library.central, "2026-11-21T10:00:00-03:00")
        .unwrap();
    assert_eq!(arrived.loan, LoanOutcome::NotOnLoan);
    assert_eq!(
        arrived.destination,
        Destination::Arrived {
            from: library.annex
        }
    );
    assert_eq!(
        library.read::<i64>("SELECT count(*) FROM transfers WHERE arrived_at IS NULL"),
        0
    );
    assert_eq!(
        library.read::<i64>("SELECT holding_branch FROM items WHERE barcode = '0001'"),
        library.central.0
    );
}

#[test]
fn checkins_follow_the_return_rule_and_the_item_status() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(&dir);
    library.rules(&[
        ("maxissueqty", "5"),
        ("issuelength", "7"),
        ("returnbranch", "noreturn"),
    ]);
    library.item("0001", ItemStatus::default());
    library.item(
        "0002",
        ItemStatus {
            withdrawn: 1,
            ..ItemStatus::default()
        },
    );
    library.item(
        "0003",
        ItemStatus {
            lost: 2,
            ..ItemStatus::default()
        },
    );
    library
        .lend("0001", "2026-11-19T10:00:00-03:00", false)
        .unwrap();
    // Due on 2026-11-26 at 23:59; returned a week later, where it stays.
    let late = library
        .give_back("0001", library.annex, "2026-12-03T10:00:00-03:00")
        .unwrap();
    assert_eq!(late.loan, LoanOutcome::Ended { overdue: true });
    assert_eq!(late.destination, Destination::Stays);
    assert_eq!(
        library.give_back("0002", library.central, "2026-12-03T10:00:00-03:00"),
        Err(CheckinRefusal::Withdrawn)
    );
    assert_eq!(
        library.give_back("9999", library.central, "2026-12-03T10:00:00-03:00"),
        Err(CheckinRefusal::UnknownBarcode)
    );
    let found = library
        .give_back("0003", library.central, "2026-12-03T10:00:00-03:00")
        .unwrap();
    assert_eq!(
        (found.loan, found.lost),
        (LoanOutcome::NotOnLoan, LostOutcome::Found)
    );
    assert_eq!(
        library.read::<i64>("SELECT lost FROM items WHERE barcode = '0003'"),
        0
    );
}

#[test]
fn a_loan_is_renewed_within_its_count_and_not_too_soon() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(&dir);
    library.item("0001", ItemStatus::default());
    library.item("0002", ItemStatus::default());
    library.rules(&[
        ("maxissueqty", "5"),
        ("issuelength", "7"),
        ("renewalperiod", "7"),
        ("renewalsallowed", "1"),
        ("norenewalbefore", "3"),
    ]);
    let lent = library
        .lend("0001", "2026-11-19T10:00:00-03:00", false)
        .unwrap();
    let due = |text: &str| -> Zoned { format!("{text}[America/Sao_Paulo]").parse().unwrap() };
    assert_eq!(lent.due, due("2026-11-26T23:59:00-03:00"));
    assert_eq!(
        library.renew("0001", "2026-11-20T10:00:00-03:00", false),
        Err(RenewRefusal::Refused(RenewalRefusal::TooSoon {
            soonest: due("2026-11-23T23:59:00-03:00")
        }))
    );
    // The new period starts at the due date.
    let renewed = library
        .renew("0001", "2026-11-24T10:00:00-03:00", false)
        .unwrap();
    assert_eq!(
        renewed,
        Renewed {
            patron: library.patron,
            due: due("2026-12-03T23:59:00-03:00"),
            renewals: 1,
        }
    );
    assert_eq!(
        library.renew("0001", "2026-12-01T10:00:00-03:00", false),
        Err(RenewRefusal::Refused(RenewalRefusal::TooMany))
    );
    let forced = library
        .renew("0001", "2026-12-01T10:00:00-03:00", true)
        .unwrap();
    assert_eq!(
        (forced.due, forced.renewals),
        (due("2026-12-10T23:59:00-03:00"), 2)
    );
    assert_eq!(
        library.renew("0002", "2026-12-01T10:00:00-03:00", false),
        Err(RenewRefusal::NotOnLoan)
    );
    assert_eq!(
        library.renew("9999", "2026-12-01T10:00:00-03:00", false),
        Err(RenewRefusal::UnknownBarcode)
    );
    library
        .give_back("0001", library.annex, "2026-12-02T10:00:00-03:00")
        .unwrap();
    assert_eq!(
        library.read::<i64>("SELECT renewals FROM returned_loans"),
        2
    );
}

#[test]
fn a_late_return_suspends_the_patron_from_borrowing() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(&dir);
    for barcode in ["0001", "0002", "0003"] {
        library.item(barcode, ItemStatus::default());
    }
    library.rules(&[
        ("maxissueqty", "5"),
        ("issuelength", "7"),
        ("finedays", "2"),
    ]);
    library
        .lend("0001", "2026-11-19T10:00:00-03:00", false)
        .unwrap();
    // Due on the 26th, back on the 30th: four days late, two days each.
    let back = library
        .give_back("0001", library.central, "2026-11-30T10:00:00-03:00")
        .unwrap();
    assert_eq!(back.loan, LoanOutcome::Ended { overdue: true });
    assert_eq!(back.suspended_until, Some(date(2026, 12, 8)));
    assert_eq!(
        library.lend("0002", "2026-12-01T10:00:00-03:00", true),
        Err(CheckoutRefusal::Impossible(vec![Impossible::Suspended(
            SuspensionEnd::Day(date(2026, 12, 8))
        )]))
    );
    // On the day it ends the patron borrows again.
    library
        .lend("0002", "2026-12-08T10:00:00-03:00", false)
        .unwrap();
    library
        .lend("0003", "2026-12-10T10:00:00-03:00", false)
        .unwrap();
    // Five days late: ten days. Then three days late: six days, which do not shorten the
    // suspension the patron already has.
    let first = library
        .give_back("0002", library.central, "2026-12-20T10:00:00-03:00")
        .unwrap();
    assert_eq!(first.suspended_until, Some(date(2026, 12, 30)));
    let second = library
        .give_back("0003", library.central, "2026-12-20T11:00:00-03:00")
        .unwrap();
    assert_eq!(second.suspended_until, Some(date(2026, 12, 30)));
    assert_eq!(
        library.read::<i64>("SELECT count(*) FROM patron_restrictions"),
        1
    );
    assert_eq!(
        library.read::<String>("SELECT expires FROM patron_restrictions"),
        "2026-12-30"
    );
}

#[test]
fn a_late_return_charges_its_fine_and_debts_hold_back_checkouts() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(&dir);
    for barcode in ["0001", "0002", "0003"] {
        library.item(barcode, ItemStatus::default());
    }
    library.rules(&[
        ("maxissueqty", "5"),
        ("issuelength", "7"),
        ("fine", "0.50"),
        ("chargeperiod", "1"),
    ]);
    library
        .lend("0001", "2026-11-19T10:00:00-03:00", false)
        .unwrap();
    // Due on the 26th, back on the 30th: four days at 0.50.
    let back = library
        .give_back("0001", library.central, "2026-11-30T10:00:00-03:00")
        .unwrap();
    assert_eq!(back.fine, Some(Money(200)));
    assert_eq!(
        library.read::<String>(
            "SELECT debit_type || ' ' || status || ' ' || amount || ' ' || outstanding
             FROM account_lines"
        ),
        "OVERDUE RETURNED 200 200"
    );
    assert_eq!(
        library.read::<String>("SELECT kind || ' ' || amount FROM account_offsets"),
        "CREATE 200"
    );
    // Any debt asks the staff first (`AllFinesNeedOverride`).
    assert_eq!(
        library.lend("0002", "2026-12-01T10:00:00-03:00", false),
        Err(CheckoutRefusal::NeedsConfirmation(vec![Confirm::Debt(
            Money(200)
        )]))
    );
    library
        .lend("0002", "2026-12-01T10:00:00-03:00", true)
        .unwrap();
    // Due on the 8th, back on the 20th: twelve days, 6.00; 8.00 is over the limit of 5.00.
    let back = library
        .give_back("0002", library.central, "2026-12-20T10:00:00-03:00")
        .unwrap();
    assert_eq!(back.fine, Some(Money(600)));
    assert_eq!(
        library.lend("0003", "2026-12-21T10:00:00-03:00", true),
        Err(CheckoutRefusal::Impossible(vec![Impossible::Debt(Money(
            800
        ))]))
    );
    // One fine per late return.
    assert_eq!(library.read::<i64>("SELECT count(*) FROM account_lines"), 2);
}

fn pay(
    library: &Library,
    kind: CreditType,
    amount: i64,
    lines: &[i64],
) -> Result<Paid, PayRefusal> {
    let at: Zoned = "2026-12-22T10:00:00-03:00[America/Sao_Paulo]"
        .parse()
        .unwrap();
    match submit(
        &library.store,
        WriteCommand::Circulation(CirculationCommand::Pay(PaymentRequest {
            patron: library.patron,
            kind,
            amount: Money(amount),
            lines: lines.iter().copied().map(AccountLineId).collect(),
            desk: library.central,
            at,
        })),
    ) {
        Ok(WriteOutput::Circulation(CirculationOutput::Paid(done))) => Ok(done),
        Err(CommandFailed::Rejected(WriteRefusal::Circulation(CirculationRefusal::Pay(
            refusal,
        )))) => Err(refusal),
        other => panic!("{other:?}"),
    }
}

#[test]
fn payments_settle_the_oldest_debits_first_and_write_offs_the_chosen_ones() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(&dir);
    for barcode in ["0001", "0002"] {
        library.item(barcode, ItemStatus::default());
    }
    library.rules(&[
        ("maxissueqty", "5"),
        ("issuelength", "7"),
        ("fine", "0.50"),
        ("chargeperiod", "1"),
    ]);
    // Two fines: 2.00 (line 1), then 6.00 (line 2).
    library
        .lend("0001", "2026-11-19T10:00:00-03:00", false)
        .unwrap();
    library
        .give_back("0001", library.central, "2026-11-30T10:00:00-03:00")
        .unwrap();
    library
        .lend("0002", "2026-12-01T10:00:00-03:00", true)
        .unwrap();
    library
        .give_back("0002", library.central, "2026-12-20T10:00:00-03:00")
        .unwrap();
    let outstanding = || {
        library.read::<String>(
            "SELECT group_concat(outstanding, ' ') FROM account_lines
             WHERE debit_type IS NOT NULL ORDER BY id",
        )
    };
    assert_eq!(
        pay(&library, CreditType::Payment, 0, &[]),
        Err(PayRefusal::Amount(PaymentRefusal::NotPositive))
    );
    assert_eq!(
        pay(&library, CreditType::Payment, 801, &[]),
        Err(PayRefusal::Amount(PaymentRefusal::OverDue {
            due: Money(800)
        }))
    );
    // 3.00 with nothing chosen: the older fine first.
    let paid = pay(&library, CreditType::Payment, 300, &[]).unwrap();
    assert_eq!(paid.owed, Money(500));
    assert_eq!(outstanding(), "0 500");
    // Line 1 owes nothing now, so it cannot be chosen.
    assert_eq!(
        pay(&library, CreditType::Payment, 100, &[1]),
        Err(PayRefusal::UnknownLine)
    );
    assert_eq!(
        pay(&library, CreditType::Writeoff, 600, &[2]),
        Err(PayRefusal::Amount(PaymentRefusal::OverDue {
            due: Money(500)
        }))
    );
    let written = pay(&library, CreditType::Writeoff, 500, &[2]).unwrap();
    assert_eq!(written.owed, Money(0));
    assert_eq!(outstanding(), "0 0");
    assert_eq!(
        library.read::<String>(
            "SELECT group_concat(credit_type || ' ' || amount || ' ' || outstanding, ', ')
             FROM account_lines WHERE credit_type IS NOT NULL ORDER BY id"
        ),
        "PAYMENT -300 0, WRITEOFF -500 0"
    );
    assert_eq!(
        library.read::<String>(
            "SELECT group_concat(credit_id || '>' || debit_id || ' ' || amount, ', ')
             FROM account_offsets WHERE kind = 'APPLY' ORDER BY id"
        ),
        format!(
            "{}>1 -200, {}>2 -100, {}>2 -500",
            paid.credit.0, paid.credit.0, written.credit.0
        )
    );
    // Paid up, the patron borrows without being asked.
    library
        .lend("0001", "2026-12-22T10:00:00-03:00", false)
        .unwrap();
}

fn charge_overdues(library: &Library, at: &str) -> OverduesCharged {
    let at: Zoned = format!("{at}[America/Sao_Paulo]").parse().unwrap();
    match submit(
        &library.store,
        WriteCommand::Circulation(CirculationCommand::ChargeOverdues { at }),
    ) {
        Ok(WriteOutput::Circulation(CirculationOutput::OverduesCharged(done))) => done,
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_daily_run_grows_the_fine_of_a_loan_still_out_except_on_closed_days() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(&dir);
    library.item("0001", ItemStatus::default());
    library.rules(&[
        ("maxissueqty", "5"),
        ("issuelength", "7"),
        ("fine", "0.50"),
        ("chargeperiod", "1"),
    ]);
    // Lent at the annex, which holds the Item while it is out; due on Thursday the 26th.
    library
        .lend("0001", "2026-11-19T10:00:00-03:00", false)
        .unwrap();
    let fine = || {
        library.read::<String>(
            "SELECT status || ' ' || amount || ' ' || outstanding FROM account_lines",
        )
    };
    assert_eq!(
        charge_overdues(&library, "2026-11-26T22:00:00-03:00"),
        OverduesCharged::default()
    );
    // Saturday: two days late.
    assert_eq!(
        charge_overdues(&library, "2026-11-28T10:00:00-03:00"),
        OverduesCharged {
            overdue: 1,
            charged: 1
        }
    );
    assert_eq!(fine(), "UNRETURNED 100 100");
    submit(
        &library.store,
        WriteCommand::Circulation(CirculationCommand::ChangeCalendar(CalendarChange {
            branch: library.annex,
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
    // Sunday the annex is closed: nothing changes (`ChargeFinesOnClosedDays` off).
    assert_eq!(
        charge_overdues(&library, "2026-11-29T10:00:00-03:00"),
        OverduesCharged {
            overdue: 1,
            charged: 0
        }
    );
    assert_eq!(fine(), "UNRETURNED 100 100");
    // Monday: the 27th, 28th and 30th count, Sunday does not.
    charge_overdues(&library, "2026-11-30T10:00:00-03:00");
    assert_eq!(fine(), "UNRETURNED 150 150");
    // Twice the same day changes nothing.
    charge_overdues(&library, "2026-11-30T18:00:00-03:00");
    let back = library
        .give_back("0001", library.central, "2026-12-01T10:00:00-03:00")
        .unwrap();
    assert_eq!(back.fine, Some(Money(200)));
    assert_eq!(fine(), "RETURNED 200 200");
    assert_eq!(
        library.read::<String>(
            "SELECT group_concat(kind || ' ' || amount, ', ') FROM account_offsets ORDER BY id"
        ),
        "CREATE 100, OVERDUE_INCREASE 50, OVERDUE_INCREASE 50"
    );
    assert_eq!(
        charge_overdues(&library, "2026-12-02T10:00:00-03:00"),
        OverduesCharged::default()
    );
}

fn mark_long_overdue(
    library: &Library,
    at: &str,
    settings: LongOverdueSettings,
) -> LongOverdueMarked {
    let at: Zoned = format!("{at}[America/Sao_Paulo]").parse().unwrap();
    match submit(
        &library.store,
        WriteCommand::Circulation(CirculationCommand::MarkLongOverdue { at, settings }),
    ) {
        Ok(WriteOutput::Circulation(CirculationOutput::LongOverdueMarked(done))) => done,
        other => panic!("{other:?}"),
    }
}

#[test]
fn long_overdue_items_are_declared_lost_and_charged_once() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(&dir);
    library.priced_item("0001", ItemStatus::default(), Some(Money(4500)));
    library.item("0002", ItemStatus::default());
    library.rules(&[
        ("maxissueqty", "5"),
        ("issuelength", "7"),
        ("fine", "0.50"),
        ("chargeperiod", "1"),
        ("lost_item_processing_fee", "3.00"),
    ]);
    // Both due on Thursday the 26th of November, and fined two days later.
    for barcode in ["0001", "0002"] {
        library
            .lend(barcode, "2026-11-19T10:00:00-03:00", false)
            .unwrap();
    }
    charge_overdues(&library, "2026-11-28T10:00:00-03:00");
    let flagged = LongOverdueSettings {
        days: 30,
        max_days: DEFAULT_MAX_DAYS,
        lost_status: 2,
        charge: false,
        mark_returned: false,
    };
    // Thirty days after midnight of the due day is not yet "more than" thirty days.
    assert_eq!(
        mark_long_overdue(&library, "2026-12-26T10:00:00-03:00", flagged),
        LongOverdueMarked::default()
    );
    assert_eq!(
        mark_long_overdue(&library, "2026-12-27T10:00:00-03:00", flagged),
        LongOverdueMarked {
            lost: 2,
            charged: 0
        }
    );
    let state = || {
        library.read::<String>(
            "SELECT (SELECT group_concat(lost, ' ') FROM items ORDER BY id) || ' | ' ||
                    (SELECT count(*) FROM loans) || ' out | ' ||
                    (SELECT group_concat(debit_type || ' ' || coalesce(status, '-') || ' '
                                         || amount, ', ')
                     FROM account_lines ORDER BY id)",
        )
    };
    // Lost without a charge: the loans go on, and the fines stop growing.
    assert_eq!(
        state(),
        "2 2 | 2 out | OVERDUE UNRETURNED 100, OVERDUE UNRETURNED 100"
    );
    assert_eq!(
        charge_overdues(&library, "2026-12-28T10:00:00-03:00"),
        OverduesCharged::default()
    );
    // Charging with another lost status: the fines stop as LOST, the fee and the
    // replacement price are charged, and the loans end.
    let charged = LongOverdueSettings {
        lost_status: 1,
        charge: true,
        ..flagged
    };
    assert_eq!(
        mark_long_overdue(&library, "2026-12-28T10:00:00-03:00", charged),
        LongOverdueMarked {
            lost: 2,
            charged: 2
        }
    );
    assert_eq!(
        state(),
        "1 1 | 0 out | OVERDUE LOST 100, OVERDUE LOST 100, PROCESSING - 300, \
         LOST - 4500, PROCESSING - 300"
    );
    assert_eq!(
        library.read::<String>(
            "SELECT group_concat(return_branch_id || ' ' || returned_at, ', ') FROM returned_loans"
        ),
        format!(
            "{0} 2026-12-28T10:00:00-03:00[America/Sao_Paulo], \
             {0} 2026-12-28T10:00:00-03:00[America/Sao_Paulo]",
            library.annex.0
        )
    );
    assert_eq!(
        mark_long_overdue(&library, "2026-12-29T10:00:00-03:00", charged),
        LongOverdueMarked::default()
    );
}

#[test]
fn holds_queue_up_per_record_and_close_up_when_one_is_cancelled() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(&dir);
    library.item("0001", ItemStatus::default());
    library.item("0002", ItemStatus::default());
    let ana = library.patron;
    let bia = library.another_patron("000124", library.central);
    let caio = library.another_patron("000125", library.annex);
    // Koha holds nothing without a `reservesallowed` rule.
    assert_eq!(
        library.hold(ana, "0001", HoldLevel::Record, library.central),
        Err(HoldRefused::Rules(HoldRefusal::NoReservesAllowed))
    );
    library.rules(&[("reservesallowed", "5")]);
    let first = library
        .hold(ana, "0001", HoldLevel::Record, library.central)
        .unwrap();
    assert_eq!(first.priority, 1);
    // `holds_per_record` is 1 when no rule says otherwise.
    assert_eq!(
        library.hold(ana, "0002", HoldLevel::Item, library.central),
        Err(HoldRefused::Rules(HoldRefusal::TooManyHoldsForThisRecord {
            limit: 1
        }))
    );
    let second = library
        .hold(bia, "0002", HoldLevel::Item, library.annex)
        .unwrap();
    let third = library
        .hold(caio, "0001", HoldLevel::Record, library.central)
        .unwrap();
    assert_eq!((second.priority, third.priority), (2, 3));
    assert_eq!(
        library.hold(bia, "0002", HoldLevel::Item, BranchId(99)),
        Err(HoldRefused::UnknownBranch)
    );
    // Only the patron's own hold can be cancelled.
    let cancel = |patron, hold: HoldPlaced| {
        submit(
            &library.store,
            WriteCommand::Circulation(CirculationCommand::CancelHold(HoldCancellation {
                patron,
                hold: hold.hold,
                at: "2026-11-21T09:00:00-03:00[America/Sao_Paulo]"
                    .parse()
                    .unwrap(),
            })),
        )
    };
    assert!(matches!(
        cancel(ana, second),
        Err(CommandFailed::Rejected(WriteRefusal::Circulation(
            CirculationRefusal::Hold(HoldRefused::UnknownHold)
        )))
    ));
    cancel(ana, first).unwrap();
    assert_eq!(
        library.read::<String>(
            "SELECT group_concat(patron_id || ':' || priority, ' ') FROM holds ORDER BY priority"
        ),
        format!("{}:1 {}:2", bia.0, caio.0)
    );
    // The ended hold forgets the patron, who keeps no reading history.
    assert_eq!(
        library.read::<String>(
            "SELECT coalesce(patron_id, '-') || ' ' || outcome || ' ' || ended_at FROM ended_holds"
        ),
        "- CANCELLED 2026-11-21T09:00:00-03:00[America/Sao_Paulo]"
    );
    let holds = library.store.holds(bia).unwrap();
    assert_eq!(
        (holds.len(), holds[0].barcode.as_deref(), holds[0].priority),
        (1, Some("0002"), 1)
    );
}

/// Two Items of one record, and two record holds on it: Bia picks up at the annex, Caio at
/// the central branch. Holds wait three days.
fn two_holds(dir: &tempfile::TempDir) -> (Library, PatronId, HoldPlaced, HoldPlaced) {
    let library = library(dir);
    library.item("0001", ItemStatus::default());
    library.item("0002", ItemStatus::default());
    library.rules(&[("reservesallowed", "5"), ("holds_pickup_period", "3")]);
    let bia = library.another_patron("000124", library.central);
    let caio = library.another_patron("000125", library.central);
    let first = library
        .hold(bia, "0001", HoldLevel::Record, library.annex)
        .unwrap();
    let second = library
        .hold(caio, "0001", HoldLevel::Record, library.central)
        .unwrap();
    (library, bia, first, second)
}

/// Each hold as priority, found and Item.
fn queue(library: &Library) -> String {
    library.read::<String>(
        "SELECT group_concat(priority || coalesce(found, '-') || coalesce(item_id, '-'), ' ')
         FROM (SELECT * FROM holds ORDER BY id)",
    )
}

#[test]
fn a_returned_item_is_set_aside_for_the_first_hold_it_can_fill() {
    let dir = tempfile::tempdir().unwrap();
    let (library, _, first, second) = two_holds(&dir);
    // Returned at the annex, where Bia picks it up: it stays there instead of going home.
    let back = library
        .give_back("0001", library.annex, "2026-11-20T10:00:00-03:00")
        .unwrap();
    assert_eq!(back.destination, Destination::Stays);
    assert_eq!(
        back.hold,
        Some(CheckinHold {
            found: HoldMatch::Reserved(first.hold),
            pickup: library.annex
        })
    );
    assert_eq!(
        library.set_aside(
            second.hold,
            "0002",
            (library.central, "2026-11-20T11:00:00-03:00")
        ),
        Ok(SetAside {
            found: Found::Waiting,
            pickup: library.central,
            expires_on: Some(date(2026, 11, 23)),
        }),
        "a record hold takes any Item of its record"
    );
    assert_eq!(
        library.set_aside(
            HoldId(99),
            "0001",
            (library.annex, "2026-11-20T11:00:00-03:00")
        ),
        Err(HoldRefused::UnknownHold)
    );
    let aside = library
        .set_aside(
            first.hold,
            "0001",
            (library.annex, "2026-11-20T11:00:00-03:00"),
        )
        .unwrap();
    assert_eq!(
        (aside.found, aside.expires_on),
        (Found::Waiting, Some(date(2026, 11, 23)))
    );
    assert_eq!(queue(&library), "0W1 0W2");
}

#[test]
fn a_hold_waiting_elsewhere_goes_back_to_the_queue_and_its_item_in_transit() {
    let dir = tempfile::tempdir().unwrap();
    let (library, bia, first, second) = two_holds(&dir);
    for (hold, barcode, desk) in [
        (first, "0001", library.annex),
        (second, "0002", library.central),
    ] {
        library
            .set_aside(hold.hold, barcode, (desk, "2026-11-20T11:00:00-03:00"))
            .unwrap();
    }
    // Waiting at the annex and returned at the central branch: the hold goes back to the
    // head of the queue, forgets the Item, and the Item stays for it to be sent.
    let at_central = library
        .give_back("0001", library.central, "2026-11-21T10:00:00-03:00")
        .unwrap();
    assert_eq!(
        at_central.hold.map(|found| found.found),
        Some(HoldMatch::Reserved(first.hold))
    );
    assert_eq!(queue(&library), "1-- 0W2");
    let sent = library
        .set_aside(
            first.hold,
            "0001",
            (library.central, "2026-11-21T11:00:00-03:00"),
        )
        .unwrap();
    assert_eq!((sent.found, sent.expires_on), (Found::InTransit, None));
    assert_eq!(
        library.read::<String>(
            "SELECT from_branch || '>' || to_branch || ' ' || reason FROM transfers
             WHERE arrived_at IS NULL"
        ),
        format!("{}>{} Reserve", library.central.0, library.annex.0)
    );
    // At the annex the transfer arrives and the hold waits again.
    let arrived = library
        .give_back("0001", library.annex, "2026-11-22T10:00:00-03:00")
        .unwrap();
    assert_eq!(
        arrived.destination,
        Destination::Arrived {
            from: library.central
        }
    );
    assert_eq!(
        arrived.hold.map(|found| found.found),
        Some(HoldMatch::InTransit(first.hold))
    );
    library
        .set_aside(
            first.hold,
            "0001",
            (library.annex, "2026-11-22T11:00:00-03:00"),
        )
        .unwrap();
    assert_eq!(queue(&library), "0W1 0W2");
    let shown = library.store.holds(bia).unwrap();
    assert_eq!(
        (shown[0].found, shown[0].expires_on),
        (Some(Found::Waiting), Some(date(2026, 11, 25)))
    );
}

#[test]
fn a_patron_who_owes_too_much_places_no_hold() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(&dir);
    library.item("0001", ItemStatus::default());
    library.item("0002", ItemStatus::default());
    library.rules(&[
        ("reservesallowed", ""),
        ("maxissueqty", "5"),
        ("issuelength", "7"),
        ("fine", "3.00"),
        ("chargeperiod", "1"),
    ]);
    library
        .lend("0002", "2026-11-01T10:00:00-03:00", false)
        .unwrap();
    // Two days late: 6.00 owed, above `maxoutstanding` 5.00.
    library
        .give_back("0002", library.central, "2026-11-10T10:00:00-03:00")
        .unwrap();
    assert_eq!(
        library.hold(library.patron, "0001", HoldLevel::Record, library.central),
        Err(HoldRefused::Rules(HoldRefusal::DebtLimit {
            owed: Money(600),
            limit: Money(500)
        }))
    );
}
