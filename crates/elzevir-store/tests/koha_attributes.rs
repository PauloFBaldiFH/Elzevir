// SPDX-License-Identifier: GPL-3.0-or-later
//! Koha's patron attributes through the real store, and their check
//! (docs/specs/circulacao/patron-attributes.md, items 16 to 19).
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "integration test: helper functions fail the test on error"
)]

use elzevir_app::commands::{WriteCommand, WriteOutput};
use elzevir_app::koha_migration::{KohaAttributesMigrated, KohaPlan, ValueLeftover, plan};
use elzevir_app::koha_verify::{Cause, Table, verify};
use elzevir_app::patron_fields::FieldKind;
use elzevir_app::patrons::{PatronCommand, PatronDesk, PatronOutput};
use elzevir_store::{ElzevirHandler, ElzevirStore, StoreConfig};
use jiff::civil::date;
use jiff::tz::TimeZone;

const PATRONS: &str = include_str!("../../elzevir-app/tests/data/koha-patrons.sql");
const ATTRIBUTES: &str = include_str!("../../elzevir-app/tests/data/koha-attributes.sql");

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
    let dump = format!("{PATRONS}\n{ATTRIBUTES}");
    plan(
        dump.as_bytes(),
        date(2026, 11, 27),
        &TimeZone::get("America/Sao_Paulo").unwrap(),
    )
    .unwrap()
}

fn attributes(store: &ElzevirStore, plan: &KohaPlan) -> KohaAttributesMigrated {
    let mut done = KohaAttributesMigrated::default();
    for batch in plan.attribute_batches() {
        let PatronOutput::KohaAttributesMigrated(write) =
            run(store, PatronCommand::MigrateKohaAttributes(Box::new(batch)))
        else {
            panic!("not an attribute migration");
        };
        done.add(write);
    }
    done
}

#[test]
fn types_become_fields_and_values_follow_their_rules() {
    let dir = tempfile::tempdir().unwrap();
    let store = ElzevirStore::open(
        &dir.path().join("elzevir.db"),
        &StoreConfig::default(),
        ElzevirHandler,
    )
    .unwrap();
    let plan = the_plan();
    // Koha's SIP2 server cuts the active currency to three letters.
    assert_eq!(plan.sip2_currency.as_deref(), Some("REA"));
    for batch in plan.batches() {
        run(&store, PatronCommand::MigrateKoha(Box::new(batch)));
    }
    let done = attributes(&store, &plan);
    assert_eq!(done.fields_created, 4);
    assert_eq!(done.fields_existing, 0);
    assert_eq!(done.options, 2, "two codes with one label share an option");
    assert_eq!(done.values, 4);
    assert_eq!(done.already, 0);
    assert!(done.names_taken.is_empty());
    let left = |name: &str, why| done.left.get(&(name.to_owned(), why)).copied();
    assert_eq!(left("Turno", ValueLeftover::Patron), Some(2));
    assert_eq!(left("Turno", ValueLeftover::Repeated), Some(1));
    assert_eq!(left("Escola", ValueLeftover::Taken), Some(1));
    assert_eq!(left("Formatura", ValueLeftover::Invalid), Some(1));
    assert_eq!(left("SUMIU", ValueLeftover::Type), Some(1));
    assert_eq!(done.left.len(), 5);

    let fields = store.fields().unwrap();
    let shape: Vec<(&str, &str, u64)> = fields
        .iter()
        .map(|line| {
            let kind = match &line.spec.kind {
                FieldKind::Text => "text",
                FieldKind::Date => "date",
                FieldKind::List(_) => "list",
            };
            (line.spec.name.as_str(), kind, line.patrons)
        })
        .collect();
    assert_eq!(
        shape,
        [
            ("Escola", "text", 1),
            ("Formatura", "date", 1),
            ("Turno", "list", 2),
            ("Vazio", "text", 0),
        ]
    );
    let turno = fields
        .iter()
        .find(|line| line.spec.name == "Turno")
        .unwrap();
    let FieldKind::List(options) = &turno.spec.kind else {
        panic!("Turno is a list");
    };
    let labels: Vec<&str> = options.iter().map(|option| option.label.as_str()).collect();
    assert_eq!(labels, ["Manhã", "Noite"]);

    // A second run finds every field and value already there.
    let again = attributes(&store, &plan);
    assert_eq!(again.fields_created, 0);
    assert_eq!(again.fields_existing, 4);
    assert_eq!(again.values, 0);
    assert_eq!(again.already, 4);
    assert_eq!(again.left, done.left);

    let footprint = store.koha_footprint().unwrap();
    let checks = verify(&plan, &[], &footprint);
    let check = checks
        .iter()
        .find(|check| check.table == Table::AttributeTypes)
        .unwrap();
    assert_eq!((check.in_dump, check.arrived), (4, 4));
    assert!(check.unexplained.is_empty(), "{:?}", check.unexplained);
    let explained = |why| {
        check
            .explained
            .get(&Cause::Attribute(why))
            .map_or(0, Vec::len)
    };
    assert_eq!(explained(ValueLeftover::Patron), 2);
    assert_eq!(explained(ValueLeftover::Repeated), 1);
    assert_eq!(explained(ValueLeftover::Taken), 1);
    assert_eq!(explained(ValueLeftover::Invalid), 1);
}

#[test]
fn verify_names_a_field_whose_values_differ() {
    let dir = tempfile::tempdir().unwrap();
    let store = ElzevirStore::open(
        &dir.path().join("elzevir.db"),
        &StoreConfig::default(),
        ElzevirHandler,
    )
    .unwrap();
    let plan = the_plan();
    for batch in plan.batches() {
        run(&store, PatronCommand::MigrateKoha(Box::new(batch)));
    }
    // Only the types arrive: every value the dump gives is then missing.
    let mut types_only = plan.attribute_batches();
    types_only.truncate(1);
    if let Some(first) = types_only.first_mut() {
        first.values.clear();
    }
    for batch in types_only {
        run(
            &store,
            PatronCommand::MigrateKohaAttributes(Box::new(batch)),
        );
    }
    let footprint = store.koha_footprint().unwrap();
    let checks = verify(&plan, &[], &footprint);
    let check = checks
        .iter()
        .find(|check| check.table == Table::AttributeTypes)
        .unwrap();
    let codes: Vec<&str> = check
        .unexplained
        .iter()
        .map(|(code, _)| code.as_str())
        .collect();
    assert_eq!(codes, ["ESCOLA", "FORM", "TURNO"]);
}
