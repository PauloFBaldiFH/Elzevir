// SPDX-License-Identifier: GPL-3.0-or-later
//! Model coverage of the BIBFRAME ontology (docs/specs/catalogo/bibframe-completo.md,
//! items 3 and 10a to 10g): every property that describes a Hub, Work, Instance or Item
//! either has a typed column or comes back unchanged after being stored as a statement.
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "integration test: helper functions fail the test on error"
)]

use elzevir_app::codes::{CodeTable, NamedCode};
use std::collections::BTreeSet;

use elzevir_app::catalog::{
    CatalogCommand, CatalogOutput, CatalogRefusal, ItemData, ItemStatus, NewInstance, NewItem,
    NewRecord, StatementReader, StatementsUpdate,
};
use elzevir_app::commands::{WriteCommand, WriteOutput, WriteRefusal};
use elzevir_app::description::{concept, work_of_material};
use elzevir_data::bibframe::{self, PropertyKind, properties_of};
use elzevir_data::vocabulary;
use elzevir_domain::catalog::{HubId, MaterialType};
use elzevir_domain::statement::{
    Description, EntityKind, EntityRef, Resource, Statement, StatementRefusal, TYPED, Value,
    is_typed,
};
use elzevir_store::{CommandFailed, ElzevirHandler, ElzevirStore, StoreConfig};

/// Where each typed property is stored.
const COLUMNS: &[(EntityKind, &str, &str)] = &[
    (EntityKind::Hub, "title", "hubs.title"),
    (EntityKind::Hub, "hasExpression", "works.hub_id"),
    (EntityKind::Work, "title", "works.title"),
    (EntityKind::Work, "expressionOf", "works.hub_id"),
    (EntityKind::Work, "hasInstance", "instances.work_id"),
    (EntityKind::Work, "contribution", "contributions.agent_id"),
    (EntityKind::Work, "subject", "work_subjects.subject_id"),
    (EntityKind::Work, "language", "work_languages.language"),
    (EntityKind::Work, "classification", "works.classification"),
    (EntityKind::Work, "summary", "works.summary"),
    (EntityKind::Work, "intendedAudience", "works.audience"),
    (EntityKind::Work, "translationOf", "work_relations.kind"),
    (EntityKind::Work, "derivativeOf", "work_relations.kind"),
    (EntityKind::Work, "partOf", "work_relations.kind"),
    (EntityKind::Work, "supplementTo", "work_relations.kind"),
    (EntityKind::Work, "continues", "work_relations.kind"),
    (EntityKind::Instance, "instanceOf", "instances.work_id"),
    (EntityKind::Instance, "hasItem", "items.instance_id"),
    (EntityKind::Instance, "title", "instances.title"),
    (
        EntityKind::Instance,
        "editionStatement",
        "instances.edition",
    ),
    (
        EntityKind::Instance,
        "provisionActivity",
        "instances.publisher",
    ),
    (EntityKind::Instance, "extent", "instances.extent"),
    (EntityKind::Instance, "seriesStatement", "instances.series"),
    (
        EntityKind::Instance,
        "identifiedBy",
        "instance_identifiers.value",
    ),
    (EntityKind::Instance, "note", "instance_notes.text"),
    (EntityKind::Item, "itemOf", "items.instance_id"),
    (EntityKind::Item, "identifiedBy", "items.barcode"),
    (EntityKind::Item, "shelfMark", "items.call_number"),
    (EntityKind::Item, "heldBy", "items.holding_branch"),
    (EntityKind::Item, "sublocation", "items.sublocation_id"),
    (EntityKind::Item, "status", "items.not_for_loan"),
    (EntityKind::Item, "usageAndAccessPolicy", "items.restricted"),
    (
        EntityKind::Item,
        "immediateAcquisition",
        "items.acquired_on",
    ),
];

struct Catalog {
    _dir: tempfile::TempDir,
    store: ElzevirStore,
    path: std::path::PathBuf,
    subjects: [EntityRef; 4],
}

fn run(
    store: &ElzevirStore,
    command: CatalogCommand,
) -> Result<CatalogOutput, CommandFailed<WriteRefusal>> {
    store
        .submit(WriteCommand::Catalog(command))
        .unwrap()
        .wait()
        .map(|output| {
            let WriteOutput::Catalog(output) = output else {
                panic!("unexpected output {output:?}");
            };
            output
        })
}

/// A Hub with a Work, an Instance and an Item.
fn record(
    title: &str,
    hub: Option<HubId>,
    work_description: Description,
    description: Description,
) -> NewRecord {
    NewRecord {
        work_title: title.to_owned(),
        material_type: MaterialType::Book,
        work_description,
        hub,
        relations: Vec::new(),
        classification: None,
        classification_scheme: None,
        summary: None,
        audience: None,
        languages: Vec::new(),
        contributions: Vec::new(),
        subjects: Vec::new(),
        items: Vec::new(),
        source: None,
        instance: NewInstance {
            title: title.to_owned(),
            description,
            ..NewInstance::default()
        },
    }
}

fn catalog() -> Catalog {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("elzevir.db");
    let store = ElzevirStore::open(&path, &StoreConfig::default(), ElzevirHandler).unwrap();
    run(
        &store,
        CatalogCommand::NameCodes(vec![NamedCode {
            table: CodeTable::Branch,
            code: "MAIN".to_owned(),
            name: "Main".to_owned(),
        }]),
    )
    .unwrap();
    run(
        &store,
        CatalogCommand::CreateItemType {
            code: "BK".to_owned(),
            description: "Book".to_owned(),
        },
    )
    .unwrap();
    let CatalogOutput::Hub(hub) = run(
        &store,
        CatalogCommand::CreateHub {
            title: "Dom Casmurro".to_owned(),
        },
    )
    .unwrap() else {
        panic!("no hub");
    };
    let record = record(
        "Dom Casmurro",
        Some(hub),
        work_of_material(MaterialType::Book),
        Description {
            classes: vec!["Electronic".to_owned()],
            statements: vec![concept("carrier", "carriers", "cr")],
        },
    );
    let CatalogOutput::Record { work, instance, .. } =
        run(&store, CatalogCommand::AddRecord(Box::new(record))).unwrap()
    else {
        panic!("no record");
    };
    let item = NewItem {
        instance,
        item: ItemData {
            barcode: Some("0001".to_owned()),
            inventory_number: None,
            home_branch: "MAIN".to_owned(),
            holding_branch: "MAIN".to_owned(),
            item_type: "BK".to_owned(),
            call_number: None,
            location: None,
            collection: None,
            price: None,
            replacement_price: None,
            copy_number: None,
            status: ItemStatus::default(),
            acquired_on: None,
        },
    };
    let CatalogOutput::Item(item) = run(&store, CatalogCommand::AddItem(Box::new(item))).unwrap()
    else {
        panic!("no item");
    };
    let subjects = [
        EntityRef {
            entity: EntityKind::Hub,
            id: hub.0,
        },
        EntityRef {
            entity: EntityKind::Work,
            id: work.0,
        },
        EntityRef {
            entity: EntityKind::Instance,
            id: instance.0,
        },
        EntityRef {
            entity: EntityKind::Item,
            id: item.0,
        },
    ];
    Catalog {
        _dir: dir,
        store,
        path,
        subjects,
    }
}

fn version(catalog: &Catalog, subject: EntityRef) -> i64 {
    let table = match subject.entity {
        EntityKind::Hub => "hubs",
        EntityKind::Work => "works",
        EntityKind::Instance => "instances",
        EntityKind::Item => "items",
    };
    let connection = rusqlite::Connection::open(&catalog.path).unwrap();
    connection
        .query_row(
            &format!("SELECT row_version FROM {table} WHERE id = ?1"),
            [subject.id],
            |row| row.get(0),
        )
        .unwrap()
}

fn set(
    catalog: &Catalog,
    subject: EntityRef,
    statements: Vec<Statement>,
) -> Result<CatalogOutput, CommandFailed<WriteRefusal>> {
    let row_version = version(catalog, subject);
    let update = StatementsUpdate {
        subject,
        row_version,
        statements,
    };
    run(
        &catalog.store,
        CatalogCommand::SetStatements(Box::new(update)),
    )
}

/// A value the ontology allows for `property`.
fn sample(property: &bibframe::Property) -> Value {
    if property.kind == PropertyKind::Datatype {
        return Value::Literal {
            text: format!("{} value", property.name),
            language: Some("por".to_owned()),
        };
    }
    if let Some(scheme) = vocabulary::schemes_of(property.name).next() {
        let concept = scheme.offered().next().unwrap();
        return Value::Concept {
            scheme: scheme.name.to_owned(),
            code: concept.code.to_owned(),
        };
    }
    let Some(class) = property
        .ranges
        .iter()
        .chain(property.suggested_values)
        .find(|name| bibframe::class(name).is_some())
    else {
        return Value::Iri(format!("https://example.org/{}", property.name));
    };
    Value::Resource(Resource {
        class: (*class).to_owned(),
        label: Some("rótulo".to_owned()),
        statements: Vec::new(),
    })
}

#[test]
fn every_typed_property_has_its_column() {
    let typed: BTreeSet<_> = TYPED
        .iter()
        .map(|(entity, name)| (format!("{entity:?}"), *name))
        .collect();
    let columns: BTreeSet<_> = COLUMNS
        .iter()
        .map(|(entity, name, _)| (format!("{:?}", entity.bibframe()), *name))
        .collect();
    assert_eq!(typed, columns);
    let catalog = catalog();
    let connection = rusqlite::Connection::open(&catalog.path).unwrap();
    for (_, name, place) in COLUMNS {
        let (table, column) = place.split_once('.').unwrap();
        let found: i64 = connection
            .query_row(
                "SELECT count(*) FROM pragma_table_info(?1) WHERE name = ?2",
                [table, column],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(found, 1, "{name}: no column {place}");
    }
}

#[test]
fn every_other_property_comes_back_as_a_statement() {
    let catalog = catalog();
    let mut total = 0;
    for subject in catalog.subjects {
        let entity = subject.entity.bibframe();
        let statements: Vec<Statement> = properties_of(entity)
            .filter(|property| !is_typed(entity, property.name))
            .map(|property| Statement {
                property: property.name.to_owned(),
                value: sample(property),
            })
            .collect();
        total += statements.len();
        set(&catalog, subject, statements.clone()).unwrap();
        assert_eq!(
            catalog.store.statements(subject).unwrap(),
            statements,
            "{entity:?}"
        );
    }
    let typed = TYPED.len();
    let applicable: usize = EntityKind::ALL
        .iter()
        .map(|kind| properties_of(kind.bibframe()).count())
        .sum();
    assert_eq!(
        total + typed,
        applicable,
        "every applicable property is covered once"
    );
}

#[test]
fn nested_resources_entities_and_iris_come_back_in_order() {
    let catalog = catalog();
    let [hub, work, instance, _] = catalog.subjects;
    let publication = Value::Resource(Resource {
        class: "Publication".to_owned(),
        label: None,
        statements: vec![
            Statement {
                property: "place".to_owned(),
                value: Value::Concept {
                    scheme: "countries".to_owned(),
                    code: "bl".to_owned(),
                },
            },
            Statement {
                property: "date".to_owned(),
                value: Value::Literal {
                    text: "1899".to_owned(),
                    language: None,
                },
            },
        ],
    });
    let statements = vec![
        Statement {
            property: "relatedTo".to_owned(),
            value: Value::Entity(hub),
        },
        Statement {
            property: "genreForm".to_owned(),
            value: Value::Iri("http://id.loc.gov/authorities/genreForms/gf2014026339".to_owned()),
        },
        Statement {
            property: "originPlace".to_owned(),
            value: Value::Concept {
                scheme: "countries".to_owned(),
                code: "bl".to_owned(),
            },
        },
        Statement {
            property: "hasPart".to_owned(),
            value: publication,
        },
    ];
    set(&catalog, work, statements.clone()).unwrap();
    assert_eq!(catalog.store.statements(work).unwrap(), statements);
    // Replacing removes what is not repeated, nested rows included.
    set(&catalog, work, statements[..1].to_vec()).unwrap();
    assert_eq!(
        catalog.store.statements(work).unwrap(),
        statements[..1].to_vec()
    );
    let connection = rusqlite::Connection::open(&catalog.path).unwrap();
    let rows: i64 = connection
        .query_row(
            "SELECT count(*) FROM statements WHERE subject_kind IS NOT 3",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(rows, 1);
    assert_eq!(catalog.store.statements(instance).unwrap().len(), 1);
}

#[test]
fn the_version_moves_and_stale_or_bad_edits_are_refused() {
    let catalog = catalog();
    let [_, work, instance, item] = catalog.subjects;
    let content = vec![Statement {
        property: "content".to_owned(),
        value: Value::Concept {
            scheme: "contentTypes".to_owned(),
            code: "txt".to_owned(),
        },
    }];
    let before = version(&catalog, instance);
    let CatalogOutput::StatementsSet { row_version, .. } =
        set(&catalog, work, content.clone()).unwrap()
    else {
        panic!("no answer");
    };
    assert_eq!(row_version, version(&catalog, work));
    assert!(
        version(&catalog, instance) > before,
        "the record shows the Work"
    );
    let stale = StatementsUpdate {
        subject: work,
        row_version: row_version - 1,
        statements: content.clone(),
    };
    assert!(matches!(
        run(
            &catalog.store,
            CatalogCommand::SetStatements(Box::new(stale))
        ),
        Err(CommandFailed::Rejected(WriteRefusal::Catalog(
            CatalogRefusal::Stale { .. }
        )))
    ));
    let before = version(&catalog, instance);
    set(
        &catalog,
        item,
        vec![Statement {
            property: "enumerationAndChronology".to_owned(),
            value: Value::Resource(Resource {
                class: "EnumerationAndChronology".to_owned(),
                label: Some("v. 1".to_owned()),
                statements: Vec::new(),
            }),
        }],
    )
    .unwrap();
    assert!(
        version(&catalog, instance) > before,
        "an Item edit changes its record"
    );
    assert_eq!(
        set(&catalog, instance, content.clone()),
        Err(CommandFailed::Rejected(WriteRefusal::Catalog(
            CatalogRefusal::BadStatement(StatementRefusal::NotApplicable {
                property: "content".to_owned(),
                entity: EntityKind::Instance
            })
        )))
    );
    let missing = EntityRef {
        entity: EntityKind::Work,
        id: 999,
    };
    let pointing = vec![Statement {
        property: "relatedTo".to_owned(),
        value: Value::Entity(missing),
    }];
    assert_eq!(
        set(&catalog, work, pointing),
        Err(CommandFailed::Rejected(WriteRefusal::Catalog(
            CatalogRefusal::UnknownEntity(missing)
        )))
    );
    let update = StatementsUpdate {
        subject: missing,
        row_version: 1,
        statements: Vec::new(),
    };
    assert_eq!(
        run(
            &catalog.store,
            CatalogCommand::SetStatements(Box::new(update))
        ),
        Err(CommandFailed::Rejected(WriteRefusal::Catalog(
            CatalogRefusal::UnknownEntity(missing)
        )))
    );
}

#[test]
fn a_record_keeps_its_classes_and_its_coded_values() {
    let catalog = catalog();
    let [_, work, instance, _] = catalog.subjects;
    assert_eq!(
        catalog.store.description(work).unwrap(),
        work_of_material(MaterialType::Book)
    );
    let stored = catalog.store.description(instance).unwrap();
    assert_eq!(stored.classes, ["Electronic"]);
    assert_eq!(stored.statements, [concept("carrier", "carriers", "cr")]);
    // Replacing the statements leaves the classes.
    set(&catalog, instance, Vec::new()).unwrap();
    assert_eq!(
        catalog.store.description(instance).unwrap().classes,
        ["Electronic"]
    );
}

#[test]
fn a_class_of_another_entity_is_refused() {
    let catalog = catalog();
    let print = Description {
        classes: vec!["Print".to_owned()],
        statements: Vec::new(),
    };
    let record = record("Errado", None, print, Description::default());
    let refused = run(&catalog.store, CatalogCommand::AddRecord(Box::new(record))).unwrap_err();
    assert!(
        matches!(
            refused,
            CommandFailed::Rejected(WriteRefusal::Catalog(CatalogRefusal::BadStatement(
                StatementRefusal::NotASubclass { .. }
            )))
        ),
        "{refused:?}"
    );
}
