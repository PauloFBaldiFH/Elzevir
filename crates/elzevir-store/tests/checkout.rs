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
    CatalogCommand, CatalogOutput, CatalogReader, ItemData, ItemStatus, Movement, NewInstance,
    NewItem, NewRecord, RecordItems,
};
use elzevir_app::circulation_rules::{
    CirculationCommand, CirculationOutput, CirculationRefusal, LongOverdueMarked, OverduesCharged,
    RuleChange,
};
use elzevir_app::codes::{CodeTable, NamedCode};
use elzevir_app::commands::{WriteCommand, WriteOutput, WriteRefusal};
use elzevir_app::holds::{
    CheckinHold, HoldAside, HoldCancellation, HoldId, HoldLevel, HoldPlaced, HoldRefused,
    HoldRequest, HoldSuspensionRequest, SetAside, Suspension, SuspensionChange,
};
use elzevir_app::item_status::{
    ItemStatusChange, ItemStatusChanged, ItemStatusDesk, ItemStatusRefusal, LostFound, StatusKind,
    StatusName, StatusNameChange,
};
use elzevir_app::loans::{
    CheckedIn, CheckedOut, CheckinRefusal, CheckinRequest, CheckoutRefusal, CheckoutRequest,
    LoanDesk, RenewRefusal, RenewRequest, Renewed,
};
use elzevir_app::patrons::{
    CategoryChange, PatronCommand, PatronFields, PatronOutput, ReadingHistory,
};
use elzevir_app::pending::PendingReader;
use elzevir_domain::account::{DebitType, PaymentKind, PaymentRefusal};
use elzevir_domain::catalog::{BranchId, InstanceId, ItemId, Money};
use elzevir_domain::checkin::{Destination, LoanOutcome, LostOutcome, TransferReason};
use elzevir_domain::checkout::{Alert, Confirm, HeldFor, Impossible, TooManyReason};
use elzevir_domain::circulation::RenewalBase;
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
            extra: Vec::new(),
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
                    issue_statement: None,
                    enumeration: None,
                    chronology: None,
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
        self.renew_from(barcode, at, override_limit, RenewalBase::DueDate)
    }

    /// A renewal whose new period starts at `base` (`ManualRenewalPeriodBase`).
    fn renew_from(
        &self,
        barcode: &str,
        at: &str,
        override_limit: bool,
        base: RenewalBase,
    ) -> Result<Renewed, RenewRefusal> {
        let at: Zoned = format!("{at}[America/Sao_Paulo]").parse().unwrap();
        match submit(
            &self.store,
            WriteCommand::Circulation(CirculationCommand::Renew(RenewRequest {
                barcode: barcode.to_owned(),
                override_limit,
                at,
                by_patron: None,
                base,
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
                extra: Vec::new(),
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
        self.hold_asked(patron, barcode, (level, pickup), false)
    }

    /// A hold the staff place, or the patron asks for on the OPAC (`by_patron`).
    fn hold_asked(
        &self,
        patron: PatronId,
        barcode: &str,
        (level, pickup): (HoldLevel, BranchId),
        by_patron: bool,
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
                by_patron,
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
            found: LostFound::default(),
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
fn a_renewal_from_now_starts_its_period_at_the_renewal() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(&dir);
    library.item("0001", ItemStatus::default());
    library.rules(&[
        ("maxissueqty", "5"),
        ("issuelength", "7"),
        ("renewalperiod", "7"),
        ("renewalsallowed", "2"),
    ]);
    library
        .lend("0001", "2026-11-19T10:00:00-03:00", false)
        .unwrap();
    let due = |text: &str| -> Zoned { format!("{text}[America/Sao_Paulo]").parse().unwrap() };
    // Due on the 26th; renewed on the 24th from that moment, not from the due date.
    let renewed = library
        .renew_from("0001", "2026-11-24T10:00:37-03:00", false, RenewalBase::Now)
        .unwrap();
    assert_eq!(renewed.due, due("2026-12-01T23:59:00-03:00"));
    // From now, a renewal on an overdue loan also starts at the renewal.
    let late = library
        .renew_from("0001", "2026-12-05T09:00:00-03:00", false, RenewalBase::Now)
        .unwrap();
    assert_eq!(late.due, due("2026-12-12T23:59:00-03:00"));
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
    kind: PaymentKind,
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
        pay(&library, PaymentKind::Payment, 0, &[]),
        Err(PayRefusal::Amount(PaymentRefusal::NotPositive))
    );
    assert_eq!(
        pay(&library, PaymentKind::Payment, 801, &[]),
        Err(PayRefusal::Amount(PaymentRefusal::OverDue {
            due: Money(800)
        }))
    );
    // 3.00 with nothing chosen: the older fine first.
    let paid = pay(&library, PaymentKind::Payment, 300, &[]).unwrap();
    assert_eq!(paid.owed, Money(500));
    assert_eq!(outstanding(), "0 500");
    // Line 1 owes nothing now, so it cannot be chosen.
    assert_eq!(
        pay(&library, PaymentKind::Payment, 100, &[1]),
        Err(PayRefusal::UnknownLine)
    );
    assert_eq!(
        pay(&library, PaymentKind::Writeoff, 600, &[2]),
        Err(PayRefusal::Amount(PaymentRefusal::OverDue {
            due: Money(500)
        }))
    );
    let written = pay(&library, PaymentKind::Writeoff, 500, &[2]).unwrap();
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
                by_patron: false,
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
    let now: jiff::Timestamp = "2026-11-22T15:00:00Z".parse().unwrap();
    assert_eq!(library.store.pending(now).unwrap().waiting_holds, 2);
    let shown = library.store.holds(bia).unwrap();
    assert_eq!(
        (shown[0].found, shown[0].expires_on),
        (Some(Found::Waiting), Some(date(2026, 11, 25)))
    );
}

#[test]
fn a_checkout_asks_before_taking_an_item_held_for_another_and_fills_the_holders_hold() {
    let dir = tempfile::tempdir().unwrap();
    let (library, bia, first, _) = two_holds(&dir);
    library.rules(&[("maxissueqty", "5"), ("issuelength", "7")]);
    library
        .set_aside(
            first.hold,
            "0001",
            (library.annex, "2026-11-20T11:00:00-03:00"),
        )
        .unwrap();
    // Ana has no hold: the Item waits for Bia, and the next one may fill Caio's hold.
    assert_eq!(
        library.lend("0001", "2026-11-20T12:00:00-03:00", false),
        Err(CheckoutRefusal::NeedsConfirmation(vec![Confirm::Held(
            HeldFor::Waiting
        )]))
    );
    assert_eq!(
        library.lend("0002", "2026-11-20T12:00:00-03:00", false),
        Err(CheckoutRefusal::NeedsConfirmation(vec![Confirm::Held(
            HeldFor::Reserved
        )]))
    );
    // Bia takes her Item: her hold is filled and leaves the queue.
    library
        .lend_to(bia, "0001", "2026-11-20T12:00:00-03:00", false)
        .unwrap();
    assert_eq!(queue(&library), "1--");
    assert_eq!(
        library.read::<String>(
            "SELECT coalesce(patron_id, '-') || ' ' || item_id || ' ' || outcome FROM ended_holds"
        ),
        "- 1 FILLED"
    );
    // Ana borrows the other Item after confirming; Caio's hold stays first in the queue.
    library
        .lend("0002", "2026-11-20T12:00:00-03:00", true)
        .unwrap();
    assert_eq!(queue(&library), "1--");
}

#[test]
fn a_record_hold_is_filled_by_any_item_of_the_record_unless_a_rule_says_no() {
    let dir = tempfile::tempdir().unwrap();
    let (library, bia, first, _) = two_holds(&dir);
    library.rules(&[("maxissueqty", "5"), ("issuelength", "7")]);
    // Item 0002 can fill Caio's hold, so the staff confirm; Bia's own hold on the record,
    // set aside with 0001, is filled by 0002 only when the rule lets it.
    library
        .set_aside(
            first.hold,
            "0001",
            (library.annex, "2026-11-20T11:00:00-03:00"),
        )
        .unwrap();
    library.rules(&[("fill_other_biblio_holds_policy", "0")]);
    library
        .lend_to(bia, "0002", "2026-11-20T12:00:00-03:00", true)
        .unwrap();
    assert_eq!(queue(&library), "0W1 1--", "the rule keeps Bia's hold");
    library
        .give_back("0002", library.annex, "2026-11-20T13:00:00-03:00")
        .unwrap();
    library.rules(&[("fill_other_biblio_holds_policy", "1")]);
    library
        .lend_to(bia, "0002", "2026-11-20T14:00:00-03:00", true)
        .unwrap();
    assert_eq!(queue(&library), "1--");
}

#[test]
fn a_hold_waiting_past_its_last_day_is_cancelled() {
    let dir = tempfile::tempdir().unwrap();
    let (library, _, first, _) = two_holds(&dir);
    library
        .set_aside(
            first.hold,
            "0001",
            (library.annex, "2026-11-20T11:00:00-03:00"),
        )
        .unwrap();
    let expire = |at: &str| match submit(
        &library.store,
        WriteCommand::Circulation(CirculationCommand::ExpireHolds {
            at: format!("{at}[America/Sao_Paulo]").parse().unwrap(),
        }),
    ) {
        Ok(WriteOutput::Circulation(CirculationOutput::HoldsExpired { cancelled })) => cancelled,
        other => panic!("{other:?}"),
    };
    // The last day is 2026-11-23: the hold still waits on it.
    assert_eq!(expire("2026-11-23T23:00:00-03:00"), 0);
    assert_eq!(queue(&library), "0W1 1--");
    assert_eq!(expire("2026-11-24T00:30:00-03:00"), 1);
    assert_eq!(queue(&library), "1--");
    assert_eq!(
        library.read::<String>("SELECT outcome || ' ' || item_id FROM ended_holds"),
        "CANCELLED 1"
    );
    assert_eq!(expire("2026-11-25T00:30:00-03:00"), 0);
}

/// Suspends or resumes a hold of `patron`.
fn suspend(
    library: &Library,
    patron: PatronId,
    hold: HoldId,
    change: SuspensionChange,
) -> Result<WriteOutput, CommandFailed<WriteRefusal>> {
    submit(
        &library.store,
        WriteCommand::Circulation(CirculationCommand::SuspendHold(HoldSuspensionRequest {
            patron,
            hold,
            change,
        })),
    )
}

/// Each hold as suspended (`S`, with its day) or not (`-`).
fn suspensions(library: &Library) -> String {
    library.read::<String>(
        "SELECT group_concat(CASE suspended WHEN 1 THEN 'S' || coalesce(suspended_until, '')
                                            ELSE '-' END, ' ')
         FROM (SELECT * FROM holds ORDER BY id)",
    )
}

#[test]
fn a_suspended_hold_keeps_its_place_and_the_returned_item_goes_to_the_next() {
    let dir = tempfile::tempdir().unwrap();
    let (library, bia, first, second) = two_holds(&dir);
    suspend(
        &library,
        bia,
        first.hold,
        SuspensionChange::Suspend(Suspension::Indefinite),
    )
    .unwrap();
    assert_eq!(suspensions(&library), "S -");
    assert_eq!(queue(&library), "1-- 2--", "the places stay");
    // Back at the central branch, the Item answers Caio, behind Bia (`CheckReserves`).
    let back = library
        .give_back("0001", library.central, "2026-11-20T10:00:00-03:00")
        .unwrap();
    assert_eq!(
        back.hold,
        Some(CheckinHold {
            found: HoldMatch::Reserved(second.hold),
            pickup: library.central
        })
    );
    // Only Bia's hold left, still suspended: the Item answers no one.
    let caio = library.read::<i64>(&format!(
        "SELECT patron_id FROM holds WHERE id = {}",
        second.hold.0
    ));
    submit(
        &library.store,
        WriteCommand::Circulation(CirculationCommand::CancelHold(HoldCancellation {
            patron: PatronId(caio),
            hold: second.hold,
            at: "2026-11-20T10:05:00-03:00[America/Sao_Paulo]"
                .parse()
                .unwrap(),
            by_patron: false,
        })),
    )
    .unwrap();
    let back = library
        .give_back("0002", library.central, "2026-11-20T10:10:00-03:00")
        .unwrap();
    assert_eq!(back.hold, None);
    // Resumed, it is the first in the queue again.
    suspend(&library, bia, first.hold, SuspensionChange::Resume).unwrap();
    assert_eq!(suspensions(&library), "-");
    let back = library
        .give_back("0002", library.central, "2026-11-20T10:20:00-03:00")
        .unwrap();
    assert_eq!(
        back.hold.map(|hold| hold.found),
        Some(HoldMatch::Reserved(first.hold))
    );
}

#[test]
fn a_hold_set_aside_or_of_another_patron_is_not_suspended() {
    let dir = tempfile::tempdir().unwrap();
    let (library, bia, first, second) = two_holds(&dir);
    let refusal = |result: Result<WriteOutput, CommandFailed<WriteRefusal>>| match result {
        Err(CommandFailed::Rejected(WriteRefusal::Circulation(CirculationRefusal::Hold(
            refusal,
        )))) => refusal,
        other => panic!("{other:?}"),
    };
    let indefinite = SuspensionChange::Suspend(Suspension::Indefinite);
    // Caio's hold, asked as Bia's.
    assert_eq!(
        refusal(suspend(&library, bia, second.hold, indefinite)),
        HoldRefused::UnknownHold
    );
    library
        .set_aside(
            first.hold,
            "0001",
            (library.annex, "2026-11-20T11:00:00-03:00"),
        )
        .unwrap();
    // Koha: `CannotSuspendFound`.
    assert_eq!(
        refusal(suspend(&library, bia, first.hold, indefinite)),
        HoldRefused::SuspendSetAside
    );
    assert_eq!(suspensions(&library), "- -");
    // Resuming a hold that is not suspended changes nothing.
    suspend(&library, bia, first.hold, SuspensionChange::Resume).unwrap();
    assert_eq!(queue(&library), "0W1 1--");
}

#[test]
fn a_hold_suspended_until_a_day_comes_back_on_that_day() {
    let dir = tempfile::tempdir().unwrap();
    let (library, bia, first, _) = two_holds(&dir);
    suspend(
        &library,
        bia,
        first.hold,
        SuspensionChange::Suspend(Suspension::Until(date(2026, 11, 25))),
    )
    .unwrap();
    assert_eq!(suspensions(&library), "S2026-11-25 -");
    assert_eq!(
        LoanDesk::holds(&library.store, bia).unwrap()[0].suspended,
        Some(Suspension::Until(date(2026, 11, 25)))
    );
    let resume = |at: &str| match submit(
        &library.store,
        WriteCommand::Circulation(CirculationCommand::ResumeSuspendedHolds {
            at: format!("{at}[America/Sao_Paulo]").parse().unwrap(),
        }),
    ) {
        Ok(WriteOutput::Circulation(CirculationOutput::SuspendedHoldsResumed { resumed })) => {
            resumed
        }
        other => panic!("{other:?}"),
    };
    // The day before, late in the evening, it stays suspended (Koha: `AutoUnsuspendReserves`
    // resumes on `suspend_until <= today`).
    assert_eq!(resume("2026-11-24T23:30:00-03:00"), 0);
    assert_eq!(suspensions(&library), "S2026-11-25 -");
    assert_eq!(resume("2026-11-25T00:10:00-03:00"), 1);
    assert_eq!(suspensions(&library), "- -");
    assert_eq!(queue(&library), "1-- 2--");
    // An indefinite suspension waits for the desk.
    suspend(
        &library,
        bia,
        first.hold,
        SuspensionChange::Suspend(Suspension::Indefinite),
    )
    .unwrap();
    assert_eq!(resume("2027-01-01T10:00:00-03:00"), 0);
    assert_eq!(suspensions(&library), "S -");
}

#[test]
fn a_suspended_item_hold_does_not_block_a_renewal_but_is_filled_by_its_own_checkout() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(&dir);
    library.item("0001", ItemStatus::default());
    library.rules(&[
        ("maxissueqty", "5"),
        ("issuelength", "7"),
        ("renewalsallowed", "3"),
        ("reservesallowed", "5"),
    ]);
    library
        .lend("0001", "2026-11-19T10:00:00-03:00", false)
        .unwrap();
    let bruno = library.another_patron("000200", library.central);
    let held = library
        .hold(bruno, "0001", HoldLevel::Item, library.central)
        .unwrap();
    let at = "2026-11-20T10:00:00-03:00";
    assert_eq!(
        library.renew("0001", at, false),
        Err(RenewRefusal::Refused(RenewalRefusal::OnHold))
    );
    suspend(
        &library,
        bruno,
        held.hold,
        SuspensionChange::Suspend(Suspension::Indefinite),
    )
    .unwrap();
    // Koha: `current_holds` and `CheckReserves` skip a suspended hold.
    assert_eq!(library.renew("0001", at, false).unwrap().renewals, 1);
    // Bruno takes the Item himself: his own hold is filled, suspended or not
    // (Koha: `MoveReserve` looks at the patron's holds without `suspend`).
    library
        .give_back("0001", library.central, "2026-11-21T10:00:00-03:00")
        .unwrap();
    assert_eq!(suspensions(&library), "S");
    library
        .lend_to(bruno, "0001", "2026-11-21T11:00:00-03:00", false)
        .unwrap();
    assert_eq!(
        library.read::<String>("SELECT outcome FROM ended_holds"),
        "FILLED"
    );
}

#[test]
fn a_patron_cancels_only_a_hold_not_yet_set_aside() {
    let dir = tempfile::tempdir().unwrap();
    let (library, bia, first, _) = two_holds(&dir);
    library
        .set_aside(
            first.hold,
            "0001",
            (library.annex, "2026-11-20T11:00:00-03:00"),
        )
        .unwrap();
    let cancel = |by_patron| {
        submit(
            &library.store,
            WriteCommand::Circulation(CirculationCommand::CancelHold(HoldCancellation {
                patron: bia,
                hold: first.hold,
                at: "2026-11-21T09:00:00-03:00[America/Sao_Paulo]"
                    .parse()
                    .unwrap(),
                by_patron,
            })),
        )
    };
    // From the OPAC the waiting hold stays; the desk may still cancel it.
    assert!(matches!(
        cancel(true),
        Err(CommandFailed::Rejected(WriteRefusal::Circulation(
            CirculationRefusal::Hold(HoldRefused::SetAside)
        )))
    ));
    assert_eq!(queue(&library), "0W1 1--");
    cancel(false).unwrap();
    assert_eq!(queue(&library), "1--");
}

#[test]
fn the_public_catalog_sees_loans_transfers_and_the_hold_shelf() {
    let dir = tempfile::tempdir().unwrap();
    let (library, _, first, second) = two_holds(&dir);
    library.rules(&[("maxissueqty", "5"), ("issuelength", "7")]);
    let states = || {
        library
            .store
            .item_statuses(&[library.instance])
            .unwrap()
            .into_iter()
            .map(|(_, state)| (state.on_loan, state.movement, state.is_available()))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        states(),
        [
            (false, Movement::Shelved, true),
            (false, Movement::Shelved, true)
        ]
    );
    // 0001 waits at the annex; 0002 goes in transit to the central branch for Caio.
    library
        .set_aside(
            first.hold,
            "0001",
            (library.annex, "2026-11-20T11:00:00-03:00"),
        )
        .unwrap();
    library
        .set_aside(
            second.hold,
            "0002",
            (library.annex, "2026-11-20T11:00:00-03:00"),
        )
        .unwrap();
    assert_eq!(
        states(),
        [
            (false, Movement::OnHoldShelf, false),
            (false, Movement::InTransit, false)
        ]
    );
    let items = library.store.items(library.instance).unwrap();
    assert_eq!(
        items.iter().map(|item| item.movement).collect::<Vec<_>>(),
        [Movement::OnHoldShelf, Movement::InTransit]
    );
    library
        .lend("0001", "2026-11-20T12:00:00-03:00", true)
        .unwrap();
    assert_eq!(states()[0], (true, Movement::OnHoldShelf, false));
}

#[test]
fn a_patron_who_owes_too_much_places_no_hold_but_the_desk_places_it() {
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
    let asked = (HoldLevel::Record, library.central);
    assert_eq!(
        library.hold_asked(library.patron, "0001", asked, true),
        Err(HoldRefused::Rules(HoldRefusal::DebtLimit {
            owed: Money(600),
            limit: Money(500)
        }))
    );
    // Koha's staff desk does not look at the debt (`reserve/placerequest.pl`).
    let placed = library.hold_asked(library.patron, "0001", asked, false);
    assert_eq!(placed.map(|placed| placed.priority), Ok(1));
}

/// Each notice as kind, transport, address, last day and the barcodes it lists.
fn notices(library: &Library) -> String {
    library
        .read::<Option<String>>(
            "SELECT group_concat(line, ' | ') FROM (
             SELECT n.kind || ' ' || n.transport || ' ' || coalesce(n.to_address, '-') || ' '
                    || coalesce(n.hold_until, '-') || ' '
                    || (SELECT group_concat(i.barcode) FROM notice_items x
                        JOIN items i ON i.id = x.item_id WHERE x.notice_id = n.id) AS line
             FROM notices n ORDER BY n.id)",
        )
        .unwrap_or_default()
}

#[test]
fn a_hold_starting_to_wait_queues_its_notice_once_by_the_category_preference() {
    let dir = tempfile::tempdir().unwrap();
    let (library, _, first, second) = two_holds(&dir);
    let category = elzevir_domain::patron::PatronCategoryId(
        library.read::<i64>("SELECT category_id FROM patrons LIMIT 1"),
    );
    // No preference: no notice.
    library
        .set_aside(
            second.hold,
            "0002",
            (library.central, "2026-11-20T11:00:00-03:00"),
        )
        .unwrap();
    assert_eq!(notices(&library), "");
    submit(
        &library.store,
        WriteCommand::Circulation(CirculationCommand::SetHoldNotice {
            category,
            transports: elzevir_domain::notice::Transports {
                email: true,
                print: false,
            },
        }),
    )
    .unwrap();
    // In transit to its pickup branch: no notice yet.
    library
        .set_aside(
            first.hold,
            "0001",
            (library.central, "2026-11-20T11:00:00-03:00"),
        )
        .unwrap();
    assert_eq!(notices(&library), "");
    // Arrived: it waits, and Bia, without an e-mail, gets a paper notice with the last day.
    library
        .set_aside(
            first.hold,
            "0001",
            (library.annex, "2026-11-21T09:00:00-03:00"),
        )
        .unwrap();
    assert_eq!(notices(&library), "HOLD print - 2026-11-24 0001");
    // Set aside again while it waits: no second notice.
    library
        .set_aside(
            first.hold,
            "0001",
            (library.annex, "2026-11-21T10:00:00-03:00"),
        )
        .unwrap();
    assert_eq!(notices(&library), "HOLD print - 2026-11-24 0001");
}

#[test]
fn fines_over_the_limit_and_holds_block_a_renewal_and_the_opac_renews_only_its_own() {
    use elzevir_app::circulation_rules::CirculationDeskError;
    use elzevir_app::loans::LoanDesk;

    let dir = tempfile::tempdir().unwrap();
    let library = library(&dir);
    for barcode in ["0001", "0002"] {
        library.item(barcode, ItemStatus::default());
    }
    library.rules(&[
        ("maxissueqty", "5"),
        ("issuelength", "7"),
        ("renewalperiod", "7"),
        ("renewalsallowed", "3"),
        ("fine", "60"),
        ("chargeperiod", "1"),
        ("finedays", "1"),
        ("reservesallowed", "5"),
    ]);
    for barcode in ["0001", "0002"] {
        library
            .lend(barcode, "2026-11-19T10:00:00-03:00", false)
            .unwrap();
    }
    let loans = library.store.loans(library.patron).unwrap();
    assert_eq!(
        loans
            .iter()
            .map(|loan| loan.renewals_left)
            .collect::<Vec<_>>(),
        [3, 3]
    );
    // Due on the 26th, back on the 28th: 120.00, over Koha's `FineNoRenewals` of 100.
    library
        .give_back("0002", library.central, "2026-11-28T10:00:00-03:00")
        .unwrap();
    let at = "2026-11-28T11:00:00-03:00";
    assert_eq!(
        library.renew("0001", at, false),
        Err(RenewRefusal::Refused(RenewalRefusal::TooMuchOwing {
            owed: Money(12_000)
        }))
    );
    assert_eq!(library.renew("0001", at, true).unwrap().renewals, 1);
    assert_eq!(
        library.store.loans(library.patron).unwrap()[0].renewals_left,
        2
    );

    // The late return suspended Ana: the OPAC sends her to the desk.
    let own = |patron| {
        library.store.renew_own(
            patron,
            RenewRequest {
                barcode: "0001".to_owned(),
                override_limit: true,
                at: format!("{at}[America/Sao_Paulo]").parse().unwrap(),
                by_patron: None,
                base: RenewalBase::DueDate,
            },
        )
    };
    let refusal = |result: Result<Renewed, CirculationDeskError>| match result {
        Err(CirculationDeskError::Refused(CirculationRefusal::Renew(refusal))) => refusal,
        other => panic!("{other:?}"),
    };
    assert_eq!(refusal(own(library.patron)), RenewRefusal::Restricted);
    // Someone else's loan is no loan at all.
    let bruno = library.another_patron("000200", library.central);
    assert_eq!(refusal(own(bruno)), RenewRefusal::NotOnLoan);

    // A hold Bruno waits on blocks even the staff overriding the limits.
    library
        .hold(bruno, "0001", HoldLevel::Record, library.central)
        .unwrap();
    assert_eq!(
        library.renew("0001", at, true),
        Err(RenewRefusal::Refused(RenewalRefusal::OnHold))
    );
    // The patron is not logged as a staff member.
    assert_eq!(
        library.read::<Option<i64>>(
            "SELECT actor FROM audit_log WHERE kind = 'circ.renew' ORDER BY seq DESC LIMIT 1"
        ),
        None
    );
}

#[test]
fn the_patron_search_counts_open_loans_and_holds() {
    use elzevir_app::patrons::{PatronDesk, PatronQuery};
    let dir = tempfile::tempdir().unwrap();
    let (library, bia, _, _) = two_holds(&dir);
    library.rules(&[("maxissueqty", "5"), ("issuelength", "7")]);
    library
        .lend_to(bia, "0002", "2026-11-20T10:00:00-03:00", true)
        .unwrap();
    let rows = library
        .store
        .search(&PatronQuery::parse("000124").unwrap())
        .unwrap();
    let [row] = rows.as_slice() else {
        panic!("{rows:?}");
    };
    // The checkout filled Bia's hold on the record; Caio still waits.
    assert_eq!((row.loans_due.len(), row.holds, row.owed), (1, 0, Money(0)));
    assert_eq!(row.loans_due[0].date(), date(2026, 11, 27));
    assert_eq!(row.branch_name, "Central");
    let rows = library
        .store
        .search(&PatronQuery::parse("000125").unwrap())
        .unwrap();
    let [caio] = rows.as_slice() else {
        panic!("{rows:?}");
    };
    assert_eq!((caio.loans_due.len(), caio.holds), (0, 1));
}

#[test]
fn the_reports_match_the_counts_read_from_the_tables() {
    use elzevir_app::reports::{CirculationLine, CollectionLine, Period, ReportReader};

    let dir = tempfile::tempdir().unwrap();
    let library = library(&dir);
    library.rules(&[("maxissueqty", "5"), ("issuelength", "7")]);
    for (barcode, call_number) in [
        ("A1", Some("869.3 A848d")),
        ("A2", Some("J 028.5 L732m")),
        ("A3", None),
    ] {
        submit(
            &library.store,
            WriteCommand::Catalog(CatalogCommand::AddItem(Box::new(NewItem {
                instance: library.instance,
                item: ItemData {
                    issue_statement: None,
                    enumeration: None,
                    chronology: None,
                    barcode: Some(barcode.to_owned()),
                    inventory_number: None,
                    home_branch: "B1".to_owned(),
                    holding_branch: "B1".to_owned(),
                    item_type: "P1".to_owned(),
                    call_number: call_number.map(str::to_owned),
                    location: None,
                    collection: None,
                    price: None,
                    replacement_price: None,
                    copy_number: None,
                    status: ItemStatus::default(),
                    acquired_on: None,
                },
            }))),
        )
        .unwrap();
    }
    let bia = library.another_patron("000456", library.central);
    library
        .lend("A1", "2026-03-02T10:00:00-03:00", false)
        .unwrap();
    library
        .lend_to(bia, "A2", "2026-03-05T10:00:00-03:00", false)
        .unwrap();
    library
        .give_back("A1", library.central, "2026-03-10T10:00:00-03:00")
        .unwrap();
    library
        .lend("A3", "2026-04-01T10:00:00-03:00", false)
        .unwrap();
    let march = Period::new(date(2026, 3, 1), date(2026, 3, 31)).unwrap();
    let circulation = library.store.circulation(march).unwrap();
    let line = |branch: &str, checkouts, returns| CirculationLine {
        branch: branch.to_owned(),
        category: Some("Adulto".to_owned()),
        checkouts,
        returns,
    };
    assert_eq!(circulation, [line("Anexo", 2, 0), line("Central", 0, 1)]);
    // The same counts straight from the tables.
    let issued: i64 = library.read(
        "SELECT (SELECT count(*) FROM loans WHERE issued_at LIKE '2026-03-%')
              + (SELECT count(*) FROM returned_loans WHERE issued_at LIKE '2026-03-%')",
    );
    let returned: i64 =
        library.read("SELECT count(*) FROM returned_loans WHERE returned_at LIKE '2026-03-%'");
    let total = |pick: fn(&CirculationLine) -> u64| circulation.iter().map(pick).sum::<u64>();
    assert_eq!(total(|line| line.checkouts), u64::try_from(issued).unwrap());
    assert_eq!(total(|line| line.returns), u64::try_from(returned).unwrap());
    // Ana keeps no history, yet her return still counts under her category.
    let kept: Option<i64> = library.read("SELECT patron_id FROM returned_loans");
    assert_eq!(kept, None);
    // Ana borrowed in April, Bia only in March.
    let patrons = library.store.patrons(date(2026, 4, 1)).unwrap();
    let [only] = patrons.as_slice() else {
        panic!("{patrons:?}");
    };
    assert_eq!((only.patrons, only.active), (2, 1));
    let since_march = library.store.patrons(date(2026, 3, 1)).unwrap();
    assert_eq!(since_march.first().map(|line| line.active), Some(2));
    let collection = library.store.collection().unwrap();
    let items: i64 = library.read("SELECT count(*) FROM items WHERE withdrawn = 0");
    assert_eq!(
        collection.iter().map(|line| line.items).sum::<u64>(),
        u64::try_from(items).unwrap()
    );
    let classes: Vec<(Option<u16>, u64)> = collection
        .iter()
        .map(|line: &CollectionLine| (line.class, line.items))
        .collect();
    assert_eq!(classes, [(Some(0), 1), (Some(800), 1), (None, 1)]);
}

fn set_status(
    library: &Library,
    barcode: &str,
    (kind, from, to): (StatusKind, i64, i64),
    at: &str,
) -> Result<ItemStatusChanged, ItemStatusRefusal> {
    let item: i64 = library.read(&format!("SELECT id FROM items WHERE barcode = '{barcode}'"));
    match submit(
        &library.store,
        WriteCommand::Circulation(CirculationCommand::SetItemStatus(ItemStatusChange {
            item: ItemId(item),
            kind,
            from,
            to,
            at: format!("{at}[America/Sao_Paulo]").parse().unwrap(),
        })),
    ) {
        Ok(WriteOutput::Circulation(CirculationOutput::ItemStatusChanged(done))) => Ok(done),
        Err(CommandFailed::Rejected(WriteRefusal::Circulation(
            CirculationRefusal::ItemStatus(refusal),
        ))) => Err(refusal),
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_item_lost_on_loan_is_charged_once_its_loan_ends_and_its_transfer_is_cancelled() {
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
    library
        .lend("0001", "2026-11-19T10:00:00-03:00", false)
        .unwrap();
    charge_overdues(&library, "2026-11-28T10:00:00-03:00");
    let lost = (StatusKind::Lost, 0, 1);
    assert_eq!(
        set_status(
            &library,
            "0001",
            (StatusKind::Lost, 1, 2),
            "2026-11-30T10:00:00-03:00"
        ),
        Err(ItemStatusRefusal::Stale)
    );
    assert_eq!(
        set_status(&library, "0001", lost, "2026-11-30T10:00:00-03:00"),
        Ok(ItemStatusChanged {
            charged: vec![
                (DebitType::Processing, Money(300)),
                (DebitType::Lost, Money(4500)),
            ],
            loan_ended: true,
            ..ItemStatusChanged::default()
        })
    );
    let state = || {
        library.read::<String>(
            "SELECT (SELECT lost || ' ' || coalesce(lost_on, '-') FROM items WHERE id = 1)
                    || ' | ' || (SELECT count(*) FROM loans) || ' out | ' ||
                    (SELECT group_concat(debit_type || ' ' || coalesce(status, '-') || ' '
                                         || amount, ', ')
                     FROM account_lines ORDER BY id)",
        )
    };
    // The fine stops as LOST, the fee and the replacement price are charged, the loan ends
    // where the Item is, and the moment of the loss is kept.
    assert_eq!(
        state(),
        "1 2026-11-30T10:00:00-03:00[America/Sao_Paulo] | 0 out | OVERDUE LOST 100, \
         PROCESSING - 300, LOST - 4500"
    );
    assert_eq!(
        library.read::<i64>("SELECT return_branch_id FROM returned_loans"),
        library.annex.0
    );
    // Another lost number charges nothing and keeps the moment; 0 clears it.
    assert_eq!(
        set_status(
            &library,
            "0001",
            (StatusKind::Lost, 1, 2),
            "2026-12-01T10:00:00-03:00"
        ),
        Ok(ItemStatusChanged::default())
    );
    assert!(state().starts_with("2 2026-11-30T10:00:00-03:00"));
    set_status(
        &library,
        "0001",
        (StatusKind::Lost, 2, 0),
        "2026-12-02T10:00:00-03:00",
    )
    .unwrap();
    assert!(state().starts_with("0 - | 0 out"));
    // Lost on the shelf, on its way home: nobody is charged and the transfer is cancelled.
    library
        .give_back("0002", library.annex, "2026-11-20T10:00:00-03:00")
        .unwrap();
    assert_eq!(
        library.read::<i64>("SELECT count(*) FROM transfers WHERE arrived_at IS NULL"),
        1
    );
    assert_eq!(
        set_status(&library, "0002", lost, "2026-11-30T10:00:00-03:00"),
        Ok(ItemStatusChanged {
            transfer_cancelled: true,
            ..ItemStatusChanged::default()
        })
    );
    assert_eq!(library.read::<i64>("SELECT count(*) FROM transfers"), 0);
    // Nobody is charged, and the debits stay as they were.
    assert_eq!(
        library.read::<i64>("SELECT count(*) FROM account_lines WHERE debit_type IS NOT NULL"),
        3
    );
}

/// The account lines of one Item: type, status, amount and what is still owed.
fn item_lines(library: &Library, barcode: &str) -> String {
    library.read(&format!(
        "SELECT group_concat(coalesce(debit_type, credit_type) || ' ' || coalesce(status, '-')
                             || ' ' || amount || ' ' || outstanding, ', ')
         FROM (SELECT * FROM account_lines
               WHERE item_id = (SELECT id FROM items WHERE barcode = '{barcode}') ORDER BY id)"
    ))
}

/// The id of the newest debit of `kind` for the Item.
fn debit_of(library: &Library, barcode: &str, kind: &str) -> i64 {
    library.read(&format!(
        "SELECT max(id) FROM account_lines WHERE debit_type = '{kind}'
           AND item_id = (SELECT id FROM items WHERE barcode = '{barcode}')"
    ))
}

/// A library whose patron borrowed each Item on 2026-11-19 and lost it on 2026-11-30, with
/// a fine of 1.00 stopped by the loss and the fee and the price charged.
fn lost_items(dir: &tempfile::TempDir, barcodes: &[&str]) -> Library {
    let library = library(dir);
    library.rules(&[
        ("maxissueqty", "9"),
        ("issuelength", "7"),
        ("fine", "0.50"),
        ("chargeperiod", "1"),
        ("lost_item_processing_fee", "3.00"),
    ]);
    for barcode in barcodes {
        library.priced_item(barcode, ItemStatus::default(), Some(Money(4500)));
        library
            .lend(barcode, "2026-11-19T10:00:00-03:00", false)
            .unwrap();
    }
    charge_overdues(&library, "2026-11-28T10:00:00-03:00");
    for barcode in barcodes {
        set_status(
            &library,
            barcode,
            (StatusKind::Lost, 0, 1),
            "2026-11-30T10:00:00-03:00",
        )
        .unwrap();
    }
    library
}

#[test]
fn a_lost_item_found_at_the_checkin_gives_back_what_was_owed_and_paid_but_not_written_off() {
    let dir = tempfile::tempdir().unwrap();
    let library = lost_items(&dir, &["0001"]);
    let lost = debit_of(&library, "0001", "LOST");
    pay(&library, PaymentKind::Payment, 1000, &[lost]).unwrap();
    pay(&library, PaymentKind::Writeoff, 500, &[lost]).unwrap();
    let found = library
        .give_back("0001", library.central, "2026-12-23T10:00:00-03:00")
        .unwrap();
    // Koha's default `refund`: the 30.00 still owed and the 10.00 paid, not the 5.00
    // written off; the patron is 10.00 in credit and the fee is given back whole.
    assert_eq!(
        found.found,
        LostFound {
            lost_refunded: Some(Money(4000)),
            processing_refunded: Some(Money(300)),
            restored: false,
            charged: None,
        }
    );
    assert_eq!(
        item_lines(&library, "0001"),
        "OVERDUE LOST 100 100, PROCESSING FOUND 300 0, LOST FOUND 4500 0, \
         LOST_FOUND - -4000 -1000, PROCESSING_FOUND - -300 0"
    );
    assert_eq!(
        library.read::<i64>("SELECT lost FROM items WHERE barcode = '0001'"),
        0
    );
    // The credit cancels what was owed, at the desk that found the Item.
    assert_eq!(
        library.read::<String>(
            "SELECT group_concat(o.kind || ' ' || o.amount || ' ' || coalesce(o.debit_id, '-')
                                 || ' ' || c.branch_id, ', ')
             FROM account_offsets o JOIN account_lines c ON c.id = o.credit_id
             WHERE c.credit_type = 'LOST_FOUND'"
        ),
        format!(
            "CREATE -4000 - {0}, APPLY -3000 {lost} {0}",
            library.central.0
        )
    );
    // Found again later, nothing more is given back.
    set_status(
        &library,
        "0001",
        (StatusKind::Lost, 0, 1),
        "2026-12-24T10:00:00-03:00",
    )
    .unwrap();
    assert_eq!(
        set_status(
            &library,
            "0001",
            (StatusKind::Lost, 1, 0),
            "2026-12-25T10:00:00-03:00"
        ),
        Ok(ItemStatusChanged::default())
    );
}

#[test]
fn the_lostreturn_rule_chooses_what_finding_the_item_gives_back() {
    let dir = tempfile::tempdir().unwrap();
    let library = lost_items(&dir, &["0002", "0003", "0004", "0005"]);
    let find = |barcode| {
        set_status(
            &library,
            barcode,
            (StatusKind::Lost, 1, 0),
            "2026-12-23T10:00:00-03:00",
        )
        .unwrap()
        .found
    };
    // `refund_unpaid`: only the 35.00 still owed; the processing fee is not given back.
    let lost = debit_of(&library, "0002", "LOST");
    pay(&library, PaymentKind::Payment, 1000, &[lost]).unwrap();
    library.rules(&[("lostreturn", "refund_unpaid"), ("processingreturn", "0")]);
    assert_eq!(
        find("0002"),
        LostFound {
            lost_refunded: Some(Money(3500)),
            ..LostFound::default()
        }
    );
    assert_eq!(
        item_lines(&library, "0002"),
        "OVERDUE LOST 100 100, PROCESSING - 300 300, LOST FOUND 4500 0, LOST_FOUND - -3500 0"
    );
    // "0": nothing is given back and the charges stay as they were.
    library.rules(&[("lostreturn", "0")]);
    assert_eq!(find("0003"), LostFound::default());
    assert_eq!(
        item_lines(&library, "0003"),
        "OVERDUE LOST 100 100, PROCESSING - 300 300, LOST - 4500 4500"
    );
    // `restore`: the refund, and the fine stopped by the loss counts again.
    library.rules(&[("lostreturn", "restore"), ("processingreturn", "refund")]);
    assert_eq!(
        find("0004"),
        LostFound {
            lost_refunded: Some(Money(4500)),
            processing_refunded: Some(Money(300)),
            restored: true,
            charged: None,
        }
    );
    assert_eq!(
        item_lines(&library, "0004"),
        "OVERDUE FOUND 100 100, PROCESSING FOUND 300 0, LOST FOUND 4500 0, \
         LOST_FOUND - -4500 0, PROCESSING_FOUND - -300 0"
    );
    // `charge` away from a checkin: only the refund.
    library.rules(&[("lostreturn", "charge")]);
    assert_eq!(
        find("0005"),
        LostFound {
            lost_refunded: Some(Money(4500)),
            processing_refunded: Some(Money(300)),
            ..LostFound::default()
        }
    );
    assert!(item_lines(&library, "0005").starts_with("OVERDUE LOST 100 100,"));
}

#[test]
fn the_charge_rule_charges_the_fine_of_the_last_loan_up_to_the_checkin() {
    let dir = tempfile::tempdir().unwrap();
    let library = lost_items(&dir, &["0001"]);
    library.rules(&[("lostreturn", "charge")]);
    let found = library
        .give_back("0001", library.annex, "2026-12-06T10:00:00-03:00")
        .unwrap();
    // Due on 2026-11-26: ten days at 0.50, as a new fine, already returned.
    assert_eq!(
        found.found,
        LostFound {
            lost_refunded: Some(Money(4500)),
            processing_refunded: Some(Money(300)),
            restored: false,
            charged: Some(Money(500)),
        }
    );
    assert_eq!(
        item_lines(&library, "0001"),
        "OVERDUE LOST 100 100, PROCESSING FOUND 300 0, LOST FOUND 4500 0, \
         LOST_FOUND - -4500 0, OVERDUE RETURNED 500 500, PROCESSING_FOUND - -300 0"
    );
}

#[test]
fn a_lost_item_lent_again_is_found_by_the_checkout() {
    let dir = tempfile::tempdir().unwrap();
    let library = lost_items(&dir, &["0001"]);
    // Another patron, since the one who lost it owes too much to borrow.
    let other = library.another_patron("000456", library.central);
    library
        .lend_to(other, "0001", "2026-12-23T10:00:00-03:00", true)
        .unwrap();
    assert_eq!(
        item_lines(&library, "0001"),
        "OVERDUE LOST 100 100, PROCESSING FOUND 300 0, LOST FOUND 4500 0, \
         LOST_FOUND - -4500 0, PROCESSING_FOUND - -300 0"
    );
    assert_eq!(
        library.read::<String>(
            "SELECT lost || ' ' || coalesce(lost_on, '-') FROM items WHERE barcode = '0001'"
        ),
        "0 -"
    );
}

#[test]
fn damaged_and_withdrawn_change_what_the_desk_accepts_and_status_names_are_kept() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(&dir);
    library.item("0001", ItemStatus::default());
    library.rules(&[("maxissueqty", "5"), ("issuelength", "7")]);
    let at = "2026-11-20T10:00:00-03:00";
    set_status(&library, "0001", (StatusKind::Withdrawn, 0, 2), at).unwrap();
    set_status(&library, "0001", (StatusKind::Damaged, 0, 1), at).unwrap();
    assert!(matches!(
        library.lend("0001", at, true),
        Err(CheckoutRefusal::Impossible(reasons)) if reasons.contains(&Impossible::Withdrawn)
    ));
    // Damaged alone does not stop a checkout, as in Koha.
    set_status(&library, "0001", (StatusKind::Withdrawn, 2, 0), at).unwrap();
    library.lend("0001", at, false).unwrap();
    for (change, refusal) in [
        ((StatusKind::NotForLoan, 0, 1), ItemStatusRefusal::BadNumber),
        ((StatusKind::Damaged, 1, 100), ItemStatusRefusal::BadNumber),
        ((StatusKind::Damaged, 0, 1), ItemStatusRefusal::Stale),
    ] {
        assert_eq!(set_status(&library, "0001", change, at), Err(refusal));
    }
    let unknown = submit(
        &library.store,
        WriteCommand::Circulation(CirculationCommand::SetItemStatus(ItemStatusChange {
            item: ItemId(999),
            kind: StatusKind::Lost,
            from: 0,
            to: 1,
            at: format!("{at}[America/Sao_Paulo]").parse().unwrap(),
        })),
    );
    assert!(matches!(
        unknown,
        Err(CommandFailed::Rejected(WriteRefusal::Circulation(
            CirculationRefusal::ItemStatus(ItemStatusRefusal::UnknownItem)
        )))
    ));
    let desk = library.store.item_at_the_desk("0001").unwrap().unwrap();
    let numbers: Vec<(StatusKind, i64, bool)> = desk
        .statuses
        .iter()
        .map(|line| (line.kind, line.number, line.since.is_some()))
        .collect();
    assert_eq!(
        numbers,
        [
            (StatusKind::Lost, 0, false),
            (StatusKind::Damaged, 1, true),
            (StatusKind::Withdrawn, 0, false),
        ]
    );
    assert!(desk.due.is_some());
    assert_eq!(library.store.item_at_the_desk("9999").unwrap(), None);

    let name = |kind, number, label: &str| {
        submit(
            &library.store,
            WriteCommand::Circulation(CirculationCommand::ChangeStatusName(StatusNameChange::Set(
                StatusName {
                    kind,
                    number,
                    label: label.to_owned(),
                },
            ))),
        )
    };
    name(StatusKind::Lost, 1, "Perdido").unwrap();
    name(StatusKind::Withdrawn, 1, "Perdido").unwrap();
    assert!(matches!(
        name(StatusKind::Lost, 2, " Perdido "),
        Err(CommandFailed::Rejected(WriteRefusal::Circulation(
            CirculationRefusal::ItemStatus(ItemStatusRefusal::LabelTaken)
        )))
    ));
    name(StatusKind::Lost, 1, "Extraviado").unwrap();
    name(StatusKind::Lost, 2, "Perdido").unwrap();
    submit(
        &library.store,
        WriteCommand::Circulation(CirculationCommand::ChangeStatusName(
            StatusNameChange::Remove {
                kind: StatusKind::Withdrawn,
                number: 1,
            },
        )),
    )
    .unwrap();
    let names: Vec<(StatusKind, i64, String)> = library
        .store
        .status_names()
        .unwrap()
        .into_iter()
        .map(|name| (name.kind, name.number, name.label))
        .collect();
    assert_eq!(
        names,
        [
            (StatusKind::Lost, 1, "Extraviado".to_owned()),
            (StatusKind::Lost, 2, "Perdido".to_owned()),
        ]
    );
}
