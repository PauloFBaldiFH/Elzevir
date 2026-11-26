// SPDX-License-Identifier: GPL-3.0-or-later
//! Reading a whole Koha dump for the patron migration
//! (docs/specs/migracao/leitores-do-koha.md, circulacao-do-koha.md), with fictitious
//! patrons.
#![expect(clippy::unwrap_used, reason = "fixed synthetic dump")]

use elzevir_app::accounts::LineKind;
use elzevir_app::import::koha_sql::DumpProblem;
use elzevir_app::koha_migration::{
    CirculationLeftover, KohaHoldState, KohaItem, KohaPlan, Leftover, PatronFieldName, plan,
};
use elzevir_app::patrons::ReadingHistory;
use elzevir_domain::account::{CreditType, DebitType};
use elzevir_domain::catalog::Money;
use elzevir_domain::patron::Enrolment;
use elzevir_domain::suspension::RestrictionKind;
use jiff::civil::date;
use jiff::tz::TimeZone;

const DUMP: &[u8] = include_bytes!("data/koha-patrons.sql");

fn read(dump: &[u8]) -> KohaPlan {
    plan(
        dump,
        date(2026, 11, 26),
        &TimeZone::get("America/Sao_Paulo").unwrap(),
    )
    .unwrap()
}

#[test]
fn branches_and_categories() {
    let plan = read(DUMP);
    let central = &plan.branches[0];
    assert_eq!(central.code, "CTR");
    assert_eq!(central.name, "Biblioteca Central");
    assert_eq!(
        central.address.as_deref(),
        Some("Rua das Flores, 100, Cidade Exemplo, PR, 85950-000, Brasil")
    );
    assert_eq!(
        central.url.as_deref(),
        Some("https://biblioteca.example.org")
    );
    assert_eq!(plan.branches[1].address, None);
    let periods: Vec<_> = plan
        .categories
        .iter()
        .map(|category| (category.name.as_str(), category.enrolment))
        .collect();
    assert_eq!(
        periods,
        [
            ("Adulto", Enrolment::Months(12)),
            ("Estudante", Enrolment::Until(date(2027, 12, 31))),
            ("INS", Enrolment::Months(12)),
        ]
    );
    assert_eq!(plan.default_periods, 1);
}

#[test]
fn patrons_follow_the_table() {
    let plan = read(DUMP);
    let numbers: Vec<u64> = plan.patrons.iter().map(|p| p.borrowernumber).collect();
    assert_eq!(numbers, [1, 2, 6]);
    let ana = &plan.patrons[0];
    assert_eq!(ana.cardnumber, "A0001");
    assert_eq!(
        (ana.surname.as_str(), ana.first_name.as_deref()),
        ("Silva", Some("Ana Maria"))
    );
    assert_eq!(
        ana.address.as_deref(),
        Some("Rua das Acácias, 100, apto 2, Cidade Exemplo, PR, 85950-000, Brasil")
    );
    assert_eq!(ana.date_of_birth, Some(date(1990, 5, 10)));
    assert_eq!((ana.branch.as_str(), ana.category.as_str()), ("CTR", "ADU"));
    assert_eq!(
        (ana.enrolled, ana.expires),
        (date(2020, 1, 15), Some(date(2030, 1, 15)))
    );
    assert_eq!(ana.staff_note.as_deref(), Some("Leitora fictícia"));
    assert_eq!(ana.reading_history, ReadingHistory::Kept);

    // An organisation: the first name is the surname; the bad e-mail goes, the mobile
    // stands in for the phone, and enrolment without a date is today.
    let school = &plan.patrons[1];
    assert_eq!(
        (school.surname.as_str(), school.first_name.as_deref()),
        ("Escola Municipal Exemplo", None)
    );
    assert_eq!(school.email, None);
    assert_eq!(school.phone.as_deref(), Some("(44) 3000-0002"));
    assert_eq!(school.date_of_birth, None);
    assert_eq!(
        (school.enrolled, school.expires),
        (date(2026, 11, 26), None)
    );
    assert_eq!(school.reading_history, ReadingHistory::NotKept);

    // Born after enrolling and expiring before it: both emptied.
    let elisa = &plan.patrons[2];
    assert_eq!((elisa.date_of_birth, elisa.expires), (None, None));
    assert_eq!(elisa.email.as_deref(), Some("elisa@example.org"));
    assert_eq!(
        plan.cleared.into_iter().collect::<Vec<_>>(),
        [
            (PatronFieldName("date_of_birth"), 1),
            (PatronFieldName("email"), 1),
            (PatronFieldName("expires"), 1)
        ]
    );
    assert_eq!(
        plan.left,
        [
            (3, Leftover::Cardnumber),
            (4, Leftover::UnknownBranch),
            (5, Leftover::UnknownCategory)
        ]
    );
    assert_eq!(plan.staff, 1);
    assert!(
        ana.password_hash
            .as_deref()
            .is_some_and(|hash| hash.starts_with("$2a$08$"))
    );
    assert_eq!(
        (
            school.password_hash.as_deref(),
            elisa.password_hash.as_deref()
        ),
        (None, None)
    );
    assert_eq!(plan.old_passwords, 1);
    assert_eq!(
        plan.left_out,
        [
            ("title", 1),
            ("b_city", 1),
            ("sex", 1),
            ("userid", 2),
            ("gonenoaddress", 1)
        ]
    );
}

#[test]
fn restrictions_join_by_kind() {
    let plan = read(DUMP);
    let ana = &plan.patrons[0];
    let restrictions: Vec<_> = ana
        .restrictions
        .iter()
        .map(|r| (r.kind, r.expires, r.created_at.as_str()))
        .collect();
    assert_eq!(
        restrictions,
        [
            (
                RestrictionKind::Overdues,
                None,
                "2024-04-01T08:00:00-03:00[America/Sao_Paulo]"
            ),
            (
                RestrictionKind::Suspension,
                None,
                "2024-02-01T09:30:00-03:00[America/Sao_Paulo]"
            ),
        ]
    );
    assert!(plan.patrons[2].restrictions.is_empty());
    assert_eq!(plan.ended_restrictions, 1);
}

#[test]
fn batches_carry_the_registries_first() {
    let plan = read(DUMP);
    let batches = plan.batches();
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].branches.len(), 2);
    assert_eq!(batches[0].patrons.len(), 3);
}

#[test]
fn a_dump_without_patrons_is_refused() {
    let error = plan(
        &b"CREATE TABLE `branches` (`branchcode` varchar(10));"[..],
        date(2026, 11, 26),
        &TimeZone::UTC,
    )
    .unwrap_err();
    assert_eq!(error.problem, DumpProblem::MissingPatrons);
    let empty =
        read(b"CREATE DATABASE `k`; USE `k`; CREATE TABLE `borrowers` (`borrowernumber` int);");
    assert!(empty.patrons.is_empty());
}

#[test]
fn rows_without_their_key_column_are_refused() {
    let error = plan(
        &b"CREATE TABLE borrowers (cardnumber text); INSERT INTO borrowers VALUES ('A1');"[..],
        date(2026, 11, 26),
        &TimeZone::UTC,
    )
    .unwrap_err();
    assert_eq!(error.problem, DumpProblem::MissingColumns);
}

#[test]
fn the_catalog_import_still_refuses_a_whole_database() {
    let error = elzevir_app::import::koha_sql::prepare(DUMP).unwrap_err();
    assert_eq!(error.problem, DumpProblem::Unsupported);
}

#[test]
fn loans_carry_koha_numbers_and_the_library_zone() {
    let plan = read(DUMP);
    let numbers: Vec<u64> = plan.loans.iter().map(|loan| loan.issue_id).collect();
    assert_eq!(numbers, [1, 2, 3, 4, 5]);
    let first = &plan.loans[0];
    assert_eq!(
        (first.borrowernumber, &first.item, first.branch.as_str()),
        (
            1,
            &KohaItem {
                itemnumber: 100,
                barcode: Some("B100".to_owned())
            },
            "CTR"
        )
    );
    assert_eq!(
        first.issued_at,
        "2026-11-05T14:30:00-03:00[America/Sao_Paulo]"
    );
    assert_eq!(first.due, "2026-12-10T23:59:00-03:00[America/Sao_Paulo]");
    assert_eq!(
        (first.renewals, first.renewed_at.as_deref(), first.on_site),
        (
            1,
            Some("2026-11-20T10:00:00-03:00[America/Sao_Paulo]"),
            false
        )
    );
    assert!(plan.loans[1].on_site);
    // An Item Koha does not list keeps its number, without a barcode.
    assert_eq!(plan.loans[3].item.barcode, None);
    assert_eq!(plan.loans_left, [(6, CirculationLeftover::Incomplete)]);
}

#[test]
fn holds_come_in_queue_order() {
    let plan = read(DUMP);
    let order: Vec<(u64, u64)> = plan
        .holds
        .iter()
        .map(|hold| (hold.biblionumber, hold.reserve_id))
        .collect();
    // Set aside first, then by place in the queue.
    assert_eq!(
        order,
        [(10, 4), (10, 1), (10, 2), (11, 3), (11, 6), (99, 5)]
    );
    let transit = &plan.holds[0];
    assert_eq!(transit.state, KohaHoldState::InTransit);
    assert!(transit.item_level);
    assert_eq!(transit.item.as_ref().map(|item| item.itemnumber), Some(101));
    assert_eq!(plan.holds[1].state, KohaHoldState::Queued(1));
    assert_eq!(plan.holds[1].item, None);
    let waiting = &plan.holds[3];
    assert_eq!(
        waiting.state,
        KohaHoldState::Waiting {
            since: date(2026, 11, 24),
            until: Some(date(2026, 12, 1))
        }
    );
    assert_eq!(
        (waiting.placed_on, waiting.created_at.as_str()),
        (
            date(2026, 11, 1),
            "2026-11-01T00:00:00-03:00[America/Sao_Paulo]"
        )
    );
    assert!(plan.holds_left.is_empty());
    assert_eq!((plan.suspended_holds, plan.holds_with_end), (1, 1));
}

#[test]
fn circulation_goes_in_batches_of_its_own() {
    let plan = read(DUMP);
    let batches = plan.circulation_batches();
    assert_eq!(batches.len(), 1);
    assert_eq!((batches[0].loans.len(), batches[0].holds.len()), (5, 6));
}

#[test]
fn accounts_come_whole_by_patron() {
    let plan = read(DUMP);
    let owners: Vec<(u64, usize, usize)> = plan
        .accounts
        .iter()
        .map(|account| {
            (
                account.borrowernumber,
                account.lines.len(),
                account.offsets.len(),
            )
        })
        .collect();
    assert_eq!(owners, [(1, 3, 4), (3, 1, 1), (6, 2, 0)]);
    let fine = &plan.accounts[0].lines[0];
    assert_eq!(fine.kind, LineKind::Debit(DebitType::Overdue));
    assert_eq!(fine.status.as_deref(), Some("UNRETURNED"));
    assert_eq!((fine.amount, fine.outstanding), (Money(200), Money(200)));
    assert_eq!(fine.loan.as_ref().map(|item| item.itemnumber), Some(100));
    assert_eq!(
        fine.created_at,
        "2026-11-25T23:59:00-03:00[America/Sao_Paulo]"
    );
    let payment = &plan.accounts[0].lines[1];
    assert_eq!(payment.kind, LineKind::Credit(CreditType::Payment));
    assert_eq!(
        (payment.amount, payment.outstanding),
        (Money(-100), Money(0))
    );
    let apply = &plan.accounts[0].offsets[2];
    assert_eq!(
        (apply.credit, apply.debit, apply.kind.as_str(), apply.amount),
        (Some(2), Some(3), "APPLY", Money(-100))
    );
    // A type of this Koha comes as MANUAL or CREDIT, counted; without `date`, `timestamp`.
    let local = &plan.accounts[2].lines[0];
    assert_eq!(local.kind, LineKind::Debit(DebitType::Manual));
    assert_eq!(local.amount, Money(33));
    assert_eq!(
        local.created_at,
        "2026-11-20T09:00:00-03:00[America/Sao_Paulo]"
    );
    assert_eq!(
        plan.accounts[2].lines[1].kind,
        LineKind::Credit(CreditType::Credit)
    );
    let read = &plan.accounts_read;
    assert_eq!(read.lines_left, [6]);
    assert_eq!((read.offsets_left, read.rounded), (1, 1));
    assert_eq!(
        read.other_types
            .iter()
            .map(|(code, n)| (code.as_str(), *n))
            .collect::<Vec<_>>(),
        [("DOACAO", 1), ("MULTA_LOCAL", 1)]
    );
    assert_eq!(plan.account_batches().len(), 1);
}

#[test]
fn returned_loans_keep_where_they_came_back() {
    let plan = read(DUMP);
    let numbers: Vec<u64> = plan.returned.iter().map(|loan| loan.issue_id).collect();
    assert_eq!(numbers, [10, 11, 12, 13, 14]);
    let first = &plan.returned[0];
    assert_eq!(
        (
            first.borrowernumber,
            first.item.itemnumber,
            first.branch.as_str(),
            first.return_branch.as_str(),
            first.renewals
        ),
        (Some(1), 110, "CTR", "NRT", 1)
    );
    assert_eq!(
        first.returned_at,
        "2026-10-10T16:00:00-03:00[America/Sao_Paulo]"
    );
    // Anonymised in Koha; without `checkin_library`, it came back where it was lent.
    let anonymous = &plan.returned[2];
    assert_eq!(
        (anonymous.borrowernumber, anonymous.return_branch.as_str()),
        (None, "CTR")
    );
    assert_eq!(plan.returned_left, [(15, CirculationLeftover::Incomplete)]);
    assert_eq!(plan.history_batches().len(), 1);
}
