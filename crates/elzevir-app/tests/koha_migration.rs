// SPDX-License-Identifier: GPL-3.0-or-later
//! Reading a whole Koha dump for the patron migration
//! (docs/specs/migracao/leitores-do-koha.md), with fictitious patrons.
#![expect(clippy::unwrap_used, reason = "fixed synthetic dump")]

use elzevir_app::import::koha_sql::DumpProblem;
use elzevir_app::koha_migration::{KohaPlan, Leftover, PatronFieldName, plan};
use elzevir_app::patrons::ReadingHistory;
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
