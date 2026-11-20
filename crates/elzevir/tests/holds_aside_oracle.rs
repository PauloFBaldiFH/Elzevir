// SPDX-License-Identifier: GPL-3.0-or-later
//! An Item checked in on the Koha oracle finds the hold, and is set aside for it, as
//! `circ.checkin` and `circ.set_hold_aside` do (docs/specs/circulacao/reservas.md, items 13
//! to 15; `scenarios/circulacao/reservas-devolucao.koha.json`).

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "tests: a missing code or field fails the test"
)]

mod oracle;

use std::collections::BTreeSet;
use std::path::Path;

use elzevir_app::catalog::{
    CatalogCommand, CatalogOutput, ItemStatus, NewInstance, NewRecord, RecordItems,
};
use elzevir_app::circulation_rules::{CirculationCommand, CirculationOutput};
use elzevir_app::commands::{WriteCommand, WriteOutput};
use elzevir_app::holds::{HoldAside, HoldLevel, HoldRequest};
use elzevir_app::loans::{CheckinRequest, LoanDesk};
use elzevir_domain::hold::{Found, HoldMatch};
use jiff::Zoned;
use jiff::civil::Date;
use oracle::{Library, RecordedRules, submit};
use serde::Deserialize;

#[derive(Deserialize)]
struct Recording {
    patron: RecordedPatron,
    pickup: String,
    items: Vec<RecordedItem>,
    steps: Vec<Step>,
}

#[derive(Deserialize)]
struct RecordedPatron {
    category: String,
    library: String,
}

#[derive(Deserialize)]
struct RecordedItem {
    barcode: String,
    item_type: String,
    home_library: String,
    holding_library: String,
}

#[derive(Deserialize)]
struct Step {
    rules: Vec<RecordedRules>,
    at: String,
    found: bool,
    status: String,
    priority: u32,
    item_set: bool,
    expiration_date: Date,
}

/// Another record, without Items.
fn add_instance(library: &Library) -> elzevir_domain::catalog::InstanceId {
    let Ok(WriteOutput::Catalog(CatalogOutput::Record { instance, .. })) = submit(
        &library.store,
        WriteCommand::Catalog(CatalogCommand::AddRecord(Box::new(NewRecord {
            work_title: "Obra".to_owned(),
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
                title: "Obra".to_owned(),
                ..NewInstance::default()
            },
        }))),
    ) else {
        panic!("no record");
    };
    instance
}

/// A moment the oracle wrote in UTC, in the library's time zone.
fn local(at: &str) -> Zoned {
    at.parse::<jiff::Timestamp>()
        .unwrap()
        .in_tz("America/Sao_Paulo")
        .unwrap()
}

#[test]
fn a_returned_item_is_set_aside_as_the_oracle_did() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scenarios/circulacao/reservas-devolucao.koha.json");
    let recording: Recording =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut codes = BTreeSet::new();
    codes.insert((true, recording.patron.library.clone()));
    codes.insert((true, recording.pickup.clone()));
    for item in &recording.items {
        codes.insert((true, item.home_library.clone()));
        codes.insert((true, item.holding_library.clone()));
        codes.insert((false, item.item_type.clone()));
    }
    let dir = tempfile::tempdir().unwrap();
    let library = Library::new(
        &dir,
        &codes,
        &recording.patron.library,
        &recording.patron.category,
    );
    let pickup = library.branches[&recording.pickup];
    for (number, (item, step)) in recording.items.iter().zip(&recording.steps).enumerate() {
        let branches = (item.home_library.as_str(), item.holding_library.as_str());
        if number == 0 {
            library.add_item(
                &item.barcode,
                branches,
                &item.item_type,
                ItemStatus::default(),
            );
        } else {
            let instance = add_instance(&library);
            library.add_item_to(
                instance,
                &item.barcode,
                branches,
                &item.item_type,
                ItemStatus::default(),
            );
        }
        library.set_rules(&step.rules).unwrap();
        let at = local(&step.at);
        let Ok(WriteOutput::Circulation(CirculationOutput::HoldPlaced(placed))) = submit(
            &library.store,
            WriteCommand::Circulation(CirculationCommand::PlaceHold(HoldRequest {
                patron: library.patron,
                barcode: item.barcode.clone(),
                level: HoldLevel::Record,
                pickup,
                at: at.clone(),
            })),
        ) else {
            panic!("step {number}: no hold");
        };
        let Ok(WriteOutput::Circulation(CirculationOutput::CheckedIn(back))) = submit(
            &library.store,
            WriteCommand::Circulation(CirculationCommand::Checkin(CheckinRequest {
                barcode: item.barcode.clone(),
                desk: pickup,
                at: at.clone(),
            })),
        ) else {
            panic!("step {number}: no checkin");
        };
        assert_eq!(
            back.hold.map(|hold| hold.found) == Some(HoldMatch::Reserved(placed.hold)),
            step.found,
            "step {number}"
        );
        submit(
            &library.store,
            WriteCommand::Circulation(CirculationCommand::SetHoldAside(HoldAside {
                hold: placed.hold,
                barcode: item.barcode.clone(),
                desk: pickup,
                at,
            })),
        )
        .unwrap();
        let holds = library.store.holds(library.patron).unwrap();
        let hold = holds.iter().find(|hold| hold.id == placed.hold).unwrap();
        assert_eq!(
            (
                hold.found.map(Found::code),
                hold.priority,
                hold.barcode.is_some(),
                hold.expires_on
            ),
            (
                Some(step.status.as_str()),
                step.priority,
                step.item_set,
                Some(step.expiration_date)
            ),
            "step {number}"
        );
    }
}
