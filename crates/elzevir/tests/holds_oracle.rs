// SPDX-License-Identifier: GPL-3.0-or-later
//! Holds placed and refused on the Koha oracle are placed and refused the same way by
//! `circ.place_hold`, and the queue closes up the same way after `circ.cancel_hold`
//! (docs/specs/circulacao/reservas.md; `scenarios/circulacao/reservas.koha.json`).

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
use elzevir_app::circulation_rules::{CirculationCommand, CirculationOutput, CirculationRefusal};
use elzevir_app::commands::{WriteCommand, WriteOutput, WriteRefusal};
use elzevir_app::holds::{HoldCancellation, HoldId, HoldLevel, HoldRefused, HoldRequest};
use elzevir_app::loans::LoanDesk;
use elzevir_domain::catalog::InstanceId;
use elzevir_store::CommandFailed;
use jiff::Zoned;
use oracle::{Library, RecordedRules, submit};
use serde::Deserialize;

#[derive(Deserialize)]
struct Recording {
    patron: RecordedPatron,
    pickup: String,
    records: Vec<Vec<RecordedItem>>,
    steps: Vec<Step>,
    queue: Vec<(usize, u32)>,
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
#[serde(untagged)]
enum Step {
    Place {
        rules: Vec<RecordedRules>,
        record: usize,
        item: Option<usize>,
        answer: Answer,
    },
    Cancel {
        cancelled: usize,
    },
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Answer {
    Placed { priority: u32 },
    Refused { error: String },
}

/// Another record, without Items.
fn add_instance(library: &Library) -> InstanceId {
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

fn at() -> Zoned {
    "2026-11-20T10:00:00-03:00[America/Sao_Paulo]"
        .parse()
        .unwrap()
}

fn place(
    library: &Library,
    barcode: &str,
    level: HoldLevel,
    pickup: &str,
) -> Result<(HoldId, u32), String> {
    match submit(
        &library.store,
        WriteCommand::Circulation(CirculationCommand::PlaceHold(HoldRequest {
            patron: library.patron,
            barcode: barcode.to_owned(),
            level,
            pickup: library.branches[pickup],
            at: at(),
            by_patron: false,
        })),
    ) {
        Ok(WriteOutput::Circulation(CirculationOutput::HoldPlaced(placed))) => {
            Ok((placed.hold, placed.priority))
        }
        Err(CommandFailed::Rejected(WriteRefusal::Circulation(CirculationRefusal::Hold(
            HoldRefused::Rules(refusal),
        )))) => Err(format!("Hold cannot be placed. Reason: {}", refusal.code())),
        other => panic!("{other:?}"),
    }
}

#[test]
fn holds_are_placed_and_refused_as_the_oracle_did() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scenarios/circulacao/reservas.koha.json");
    let recording: Recording =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut codes = BTreeSet::new();
    codes.insert((true, recording.patron.library.clone()));
    codes.insert((true, recording.pickup.clone()));
    for item in recording.records.iter().flatten() {
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
    for (number, items) in recording.records.iter().enumerate() {
        // The first record is the library's own; the others are added.
        let instance = (number > 0).then(|| add_instance(&library));
        for item in items {
            let branches = (item.home_library.as_str(), item.holding_library.as_str());
            match instance {
                Some(instance) => library.add_item_to(
                    instance,
                    &item.barcode,
                    branches,
                    &item.item_type,
                    ItemStatus::default(),
                ),
                None => library.add_item(
                    &item.barcode,
                    branches,
                    &item.item_type,
                    ItemStatus::default(),
                ),
            }
        }
    }
    let mut placed = vec![None; recording.records.len()];
    for (number, step) in recording.steps.iter().enumerate() {
        match step {
            Step::Place {
                rules,
                record,
                item,
                answer,
            } => {
                library.set_rules(rules).unwrap();
                let barcode = &recording.records[*record][item.unwrap_or(0)].barcode;
                let level = if item.is_some() {
                    HoldLevel::Item
                } else {
                    HoldLevel::Record
                };
                let ours = place(&library, barcode, level, &recording.pickup);
                match (answer, &ours) {
                    (Answer::Placed { priority }, Ok((hold, ours))) => {
                        assert_eq!(ours, priority, "step {number}");
                        placed[*record] = Some(*hold);
                    }
                    (Answer::Refused { error }, Err(ours)) => {
                        assert_eq!(ours, error, "step {number}");
                    }
                    _ => panic!("step {number}: the oracle and Elzevir disagree: {ours:?}"),
                }
            }
            Step::Cancel { cancelled } => {
                submit(
                    &library.store,
                    WriteCommand::Circulation(CirculationCommand::CancelHold(HoldCancellation {
                        patron: library.patron,
                        hold: placed[*cancelled].unwrap(),
                        at: at(),
                        by_patron: false,
                    })),
                )
                .unwrap();
            }
        }
    }
    let titles = library.store.holds(library.patron).unwrap();
    let mut queue: Vec<(usize, u32)> = titles
        .iter()
        .map(|hold| {
            let record = placed
                .iter()
                .position(|found| *found == Some(hold.id))
                .unwrap();
            (record, hold.priority)
        })
        .collect();
    queue.sort_unstable();
    assert_eq!(queue, recording.queue);
}
