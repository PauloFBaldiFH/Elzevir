// SPDX-License-Identifier: GPL-3.0-or-later
//! Model coverage of the BIBFRAME ontology (docs/specs/catalogo/bibframe-completo.md,
//! item 3): every property that describes a Hub, Work, Instance or Item is either stored
//! somewhere in the schema or listed in `tests/data/bibframe-model-missing.txt`. That list
//! only shrinks: stage 3 of the plan empties it.
#![expect(
    clippy::unwrap_used,
    reason = "integration test: helper functions fail the test on error"
)]

use std::collections::BTreeSet;
use std::fmt::Write as _;

use elzevir_data::bibframe::{Entity, properties_of, property};
use elzevir_store::{ElzevirHandler, ElzevirStore, StoreConfig};

/// The properties without a place in the model yet, one `Entity property` per line.
const MISSING: &str = include_str!("data/bibframe-model-missing.txt");

/// Where the model stores a property today: a column, or a table of its own.
const STORED: &[(Entity, &str, &str)] = &[
    (Entity::Hub, "title", "hubs.title"),
    (Entity::Hub, "hasExpression", "works.hub_id"),
    (Entity::Work, "title", "works.title"),
    (Entity::Work, "expressionOf", "works.hub_id"),
    (Entity::Work, "hasInstance", "instances.work_id"),
    (Entity::Work, "contribution", "contributions.agent_id"),
    (Entity::Work, "subject", "work_subjects.subject_id"),
    (Entity::Work, "language", "work_languages.language"),
    (Entity::Work, "classification", "works.classification"),
    (Entity::Work, "summary", "works.summary"),
    (Entity::Work, "intendedAudience", "works.audience"),
    // `work_relations.kind`: translation, adaptation, part, supplement, continuation.
    (Entity::Work, "translationOf", "work_relations.kind"),
    (Entity::Work, "derivativeOf", "work_relations.kind"),
    (Entity::Work, "partOf", "work_relations.kind"),
    (Entity::Work, "supplementTo", "work_relations.kind"),
    (Entity::Work, "continues", "work_relations.kind"),
    (Entity::Instance, "instanceOf", "instances.work_id"),
    (Entity::Instance, "hasItem", "items.instance_id"),
    (Entity::Instance, "title", "instances.title"),
    (Entity::Instance, "editionStatement", "instances.edition"),
    (Entity::Instance, "provisionActivity", "instances.publisher"),
    (Entity::Instance, "extent", "instances.extent"),
    (Entity::Instance, "seriesStatement", "instances.series"),
    (
        Entity::Instance,
        "identifiedBy",
        "instance_identifiers.value",
    ),
    (Entity::Instance, "note", "instance_notes.text"),
    (Entity::Item, "itemOf", "items.instance_id"),
    (Entity::Item, "identifiedBy", "items.barcode"),
    (Entity::Item, "shelfMark", "items.call_number"),
    (Entity::Item, "heldBy", "items.holding_branch"),
    (Entity::Item, "sublocation", "items.location"),
    (Entity::Item, "status", "items.not_for_loan"),
    (Entity::Item, "usageAndAccessPolicy", "items.restricted"),
    (Entity::Item, "immediateAcquisition", "items.acquired_on"),
];

/// Every `table.column` of a freshly migrated database.
fn columns() -> BTreeSet<String> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("elzevir.db");
    let store = ElzevirStore::open(&path, &StoreConfig::default(), ElzevirHandler).unwrap();
    let connection =
        rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let mut statement = connection
        .prepare(
            "SELECT m.name || '.' || c.name FROM sqlite_master m, pragma_table_info(m.name) c \
             WHERE m.type = 'table'",
        )
        .unwrap();
    let found = statement
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    drop(store);
    found
}

#[test]
fn every_stored_place_exists_and_names_a_property_of_its_entity() {
    let columns = columns();
    for (entity, name, place) in STORED {
        let property = property(name).unwrap();
        assert!(
            property.applies_to(*entity),
            "{name} does not describe {entity:?}"
        );
        assert!(columns.contains(*place), "{name}: no column {place}");
    }
}

#[test]
fn the_missing_list_is_exactly_what_the_model_does_not_store() {
    let mut missing = String::new();
    for entity in Entity::ALL {
        for property in properties_of(entity) {
            let stored = STORED
                .iter()
                .any(|(stored, name, _)| *stored == entity && *name == property.name);
            if !stored {
                writeln!(missing, "{entity:?} {}", property.name).unwrap();
            }
        }
    }
    assert!(
        missing == MISSING,
        "tests/data/bibframe-model-missing.txt is out of date; it should be:\n{missing}"
    );
}

#[test]
fn the_missing_list_only_shrinks() {
    // Lower this with every property the model gains; never raise it.
    const MOST: usize = 263;
    assert!(MISSING.lines().count() <= MOST);
}
