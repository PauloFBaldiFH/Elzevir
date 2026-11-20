// SPDX-License-Identifier: GPL-3.0-or-later
//! The staff listing read from SQLite (docs/specs/catalogo/listagem.md).
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "integration test: helper functions fail the test on error"
)]

use elzevir_app::codes::{CodeTable, NamedCode};
use std::collections::BTreeSet;
use std::num::NonZeroUsize;

use elzevir_app::catalog::{
    CatalogCommand, CatalogOutput, CatalogReader, ItemData, ItemStatus, NewContribution,
    NewIdentifier, NewInstance, NewRecord, ShelfSignal, WorkChange, WorkUpdate,
};
use elzevir_app::commands::{WriteCommand, WriteOutput};
use elzevir_app::description::printed_book;
use elzevir_app::export::item_codes;
use elzevir_app::import::{ImportOptions, import_marc21};
use elzevir_app::listing::{CatalogListing, FamilyCursor, FamilyHead, browse, sort_key};
use elzevir_app::material::ContentKind;
use elzevir_domain::catalog::{AgentKind, HubId, IdentifierKind, InstanceId, WorkId};
use elzevir_marc::Limits;
use elzevir_store::{ElzevirHandler, ElzevirStore, StoreConfig};

const STAFF_EXPORT: &[u8] =
    include_bytes!("../../elzevir-app/tests/data/koha-staff-bibs-1-200.mrc");

fn open(dir: &tempfile::TempDir) -> ElzevirStore {
    ElzevirStore::open(
        &dir.path().join("elzevir.db"),
        &StoreConfig::default(),
        ElzevirHandler,
    )
    .unwrap()
}

fn run(store: &ElzevirStore, command: CatalogCommand) -> CatalogOutput {
    match store.submit(WriteCommand::Catalog(command)).unwrap().wait() {
        Ok(WriteOutput::Catalog(output)) => output,
        other => panic!("unexpected {other:?}"),
    }
}

fn imported(dir: &tempfile::TempDir) -> ElzevirStore {
    let store = open(dir);
    let codes = item_codes(STAFF_EXPORT, Limits::default()).unwrap();
    for code in codes.branches {
        run(
            &store,
            CatalogCommand::NameCodes(vec![NamedCode {
                table: CodeTable::Branch,
                name: code.clone(),
                code,
            }]),
        );
    }
    for code in codes.item_types {
        run(
            &store,
            CatalogCommand::NameCodes(vec![NamedCode {
                table: CodeTable::ItemType,
                name: code.clone(),
                code,
            }]),
        );
    }
    import_marc21(STAFF_EXPORT, &store, &ImportOptions::default()).unwrap();
    store
}

/// Every family head, one page after another.
fn all_heads(store: &ElzevirStore) -> Vec<FamilyCursor> {
    let mut heads = Vec::new();
    let mut after = None;
    loop {
        let page = browse(store, after.as_ref()).unwrap();
        heads.extend(page.rows.into_iter().map(|row| row.cursor));
        match page.next {
            Some(next) => after = Some(next),
            None => return heads,
        }
    }
}

#[test]
fn the_listing_pages_through_every_record_in_title_order() {
    let dir = tempfile::tempdir().unwrap();
    let store = imported(&dir);
    let heads = all_heads(&store);
    assert_eq!(heads.len(), 200);
    assert!(heads.is_sorted(), "families out of order");
    let distinct: BTreeSet<_> = heads.iter().map(|cursor| cursor.head).collect();
    assert_eq!(distinct.len(), 200);
    // The key is the Work's title as the listing sorts it.
    let first = &heads[0];
    let FamilyHead::Work(work) = first.head else {
        panic!("no Hubs in the import");
    };
    let instance = store
        .readers()
        .read(|c| {
            c.query_row(
                "SELECT id FROM instances WHERE work_id = ?1",
                [work.0],
                |row| row.get(0),
            )
        })
        .unwrap();
    let view = store.instance(InstanceId(instance)).unwrap().unwrap();
    assert_eq!(first.sort_key, sort_key(&view.work_title));
}

fn record(title: &str, hub: Option<HubId>) -> NewRecord {
    NewRecord {
        work_title: title.to_owned(),
        work_description: elzevir_domain::statement::Description::default(),
        hub,
        relations: Vec::new(),
        classification: Some("869.3".to_owned()),
        classification_scheme: Some("ddc".to_owned()),
        summary: None,
        audience: None,
        languages: vec!["por".to_owned()],
        contributions: vec![
            NewContribution {
                kind: AgentKind::Person,
                heading: "Bosi, Alfredo".to_owned(),
                role: None,
                primary: false,
                name_parts: Vec::new(),
                role_term: None,
                identifiers: Vec::new(),
            },
            NewContribution {
                kind: AgentKind::Person,
                heading: "Assis, Machado de".to_owned(),
                role: None,
                primary: true,
                name_parts: Vec::new(),
                role_term: None,
                identifiers: Vec::new(),
            },
        ],
        subjects: Vec::new(),
        items: elzevir_app::catalog::RecordItems::default(),
        source: None,
        instance: NewInstance {
            title: title.to_owned(),
            identifiers: vec![NewIdentifier {
                kind: IdentifierKind::Isbn,
                value: "9788535910667".to_owned(),
                qualifier: None,
            }],
            ..NewInstance::default()
        },
    }
}

fn item(inventory: &str, status: ItemStatus) -> ItemData {
    ItemData {
        barcode: None,
        inventory_number: Some(inventory.to_owned()),
        home_branch: "MAIN".to_owned(),
        holding_branch: "MAIN".to_owned(),
        item_type: "BK".to_owned(),
        call_number: Some("869.3 A848d".to_owned()),
        location: None,
        collection: None,
        price: None,
        replacement_price: None,
        copy_number: None,
        status,
        acquired_on: None,
    }
}

fn setup(store: &ElzevirStore) {
    run(
        store,
        CatalogCommand::NameCodes(vec![NamedCode {
            table: CodeTable::Branch,
            code: "MAIN".to_owned(),
            name: "Main".to_owned(),
        }]),
    );
    run(
        store,
        CatalogCommand::NameCodes(vec![NamedCode {
            table: CodeTable::ItemType,
            code: "BK".to_owned(),
            name: "Book".to_owned(),
        }]),
    );
}

fn add(store: &ElzevirStore, record: NewRecord) -> WorkId {
    match run(store, CatalogCommand::AddRecord(Box::new(record))) {
        CatalogOutput::Record { work, .. } => work,
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn a_hub_is_one_family_with_all_its_works_before_a_work_of_the_same_title() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(&dir);
    setup(&store);
    let CatalogOutput::Hub(hub) = run(
        &store,
        CatalogCommand::CreateHub {
            title: "Dom Casmurro".to_owned(),
        },
    ) else {
        panic!("no hub");
    };
    let lone = add(&store, record("Dom Casmurro", None));
    let portuguese = add(&store, record("Dom Casmurro", Some(hub)));
    let english = add(&store, record("Dom Casmurro (inglês)", Some(hub)));
    let before = add(&store, record("Ática", None));

    let heads: Vec<FamilyHead> = all_heads(&store)
        .into_iter()
        .map(|cursor| cursor.head)
        .collect();
    assert_eq!(
        heads,
        [
            FamilyHead::Work(before),
            FamilyHead::Hub(hub),
            FamilyHead::Work(lone),
        ]
    );
    let page = browse(&store, None).unwrap();
    let family = &page.rows[1];
    assert_eq!(family.hub_title.as_deref(), Some("Dom Casmurro"));
    let works: Vec<WorkId> = family.works.iter().map(|work| work.id).collect();
    assert_eq!(works, [portuguese, english]);
    // The main entry is shown even when it is not the first contributor.
    assert_eq!(
        family.works[0]
            .main_contributor
            .as_ref()
            .map(|(kind, name)| (*kind, name.as_str())),
        Some((AgentKind::Person, "Assis, Machado de"))
    );
    assert_eq!(
        family.works[0].instances[0].isbn.as_deref(),
        Some("9788535910667")
    );

    // By head, in the given order; a Work of a Hub and a missing Work are left out.
    let asked = [
        FamilyHead::Work(lone),
        FamilyHead::Work(portuguese),
        FamilyHead::Hub(hub),
        FamilyHead::Work(WorkId(9999)),
    ];
    let found: Vec<FamilyHead> = store
        .families(&asked)
        .unwrap()
        .into_iter()
        .map(|family| family.cursor.head)
        .collect();
    assert_eq!(found, [FamilyHead::Work(lone), FamilyHead::Hub(hub)]);

    // One family at a time crosses the Hub/Work boundary on the same key.
    let mut after: Option<FamilyCursor> = None;
    let mut one_by_one = Vec::new();
    loop {
        let next = store
            .families_after(after.as_ref(), NonZeroUsize::MIN)
            .unwrap();
        let Some(family) = next.into_iter().next() else {
            break;
        };
        one_by_one.push(family.cursor.head);
        after = Some(family.cursor);
    }
    assert_eq!(one_by_one, heads);
}

#[test]
fn pills_carry_the_signal_of_each_item() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(&dir);
    setup(&store);
    let mut with_items = record("Memórias póstumas", None);
    with_items.work_description = printed_book().0;
    let lost = ItemStatus {
        lost: 1,
        ..ItemStatus::default()
    };
    with_items.items = vec![item("0002", lost), item("0001", ItemStatus::default())].into();
    add(&store, with_items);
    let page = browse(&store, None).unwrap();
    let instance = &page.rows[0].works[0].instances[0];
    let signals: Vec<(Option<&str>, ShelfSignal)> = instance
        .pills
        .iter()
        .map(|pill| (pill.inventory_number.as_deref(), pill.signal))
        .collect();
    assert_eq!(
        signals,
        [
            (Some("0001"), ShelfSignal::Available),
            (Some("0002"), ShelfSignal::OutOfCirculation),
        ]
    );
    assert_eq!((instance.items, instance.available), (2, 1));
    assert_eq!(
        instance.material.content,
        Some(ContentKind::Text),
        "the line reads its content from the Work's description"
    );
}

#[test]
fn renaming_a_work_moves_its_family() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(&dir);
    setup(&store);
    let first = add(&store, record("Alpha", None));
    let second = add(&store, record("Beta", None));
    run(
        &store,
        CatalogCommand::UpdateWork(Box::new(WorkUpdate {
            work: first,
            row_version: 1,
            changes: vec![WorkChange::Title("Zeta".to_owned())],
        })),
    );
    let heads: Vec<FamilyHead> = all_heads(&store)
        .into_iter()
        .map(|cursor| cursor.head)
        .collect();
    assert_eq!(heads, [FamilyHead::Work(second), FamilyHead::Work(first)]);
}

#[test]
fn migration_8_computes_the_keys_of_older_databases() {
    let dir = tempfile::tempdir().unwrap();
    let store = imported(&dir);
    let before = all_heads(&store);
    drop(store);
    let connection = rusqlite::Connection::open(dir.path().join("elzevir.db")).unwrap();
    connection
        .execute_batch(
            "DROP TABLE notice_items; DROP TABLE notices; DROP TABLE overdue_notice_rules; DROP TABLE ended_holds; DROP TABLE holds; DROP TABLE account_offsets; DROP TABLE account_lines; DROP TABLE patron_restrictions; DROP TABLE transfers; DROP TABLE returned_loans; DROP TABLE loans; DROP TABLE calendar_dates; DROP TABLE calendar_annual; DROP TABLE calendar_weekly; DROP TABLE circulation_rules; DROP TABLE patrons; DROP TABLE patron_categories; DROP INDEX audit_imports; DROP TABLE saved_list_families; DROP TABLE saved_lists; DROP INDEX audit_by_actor; DROP INDEX audit_by_kind; DROP INDEX audit_entities_by_seq; DROP INDEX works_in_listing; DROP INDEX hubs_in_listing;
             ALTER TABLE works DROP COLUMN sort_key; ALTER TABLE hubs DROP COLUMN sort_key; ALTER TABLE audit_log DROP COLUMN actor; DROP INDEX agents_by_heading; DROP INDEX subjects_by_heading; DROP TABLE koha_type_statements; ALTER TABLE circulation_policies ADD COLUMN code TEXT; UPDATE circulation_policies SET code = COALESCE((SELECT code FROM koha_codes WHERE kind = 4 AND local_id = circulation_policies.id), 'P' || id); ALTER TABLE circulation_policies RENAME COLUMN name TO description; ALTER TABLE circulation_policies RENAME TO item_types; ALTER TABLE items RENAME COLUMN policy_id TO item_type; DELETE FROM koha_codes WHERE kind = 4; ALTER TABLE branches ADD COLUMN code TEXT; UPDATE branches SET code = COALESCE((SELECT code FROM koha_codes WHERE kind = 1 AND local_id = branches.id), 'B' || id); ALTER TABLE items ADD COLUMN location INTEGER REFERENCES terms (id); ALTER TABLE items ADD COLUMN collection INTEGER REFERENCES terms (id); INSERT OR IGNORE INTO terms (vocabulary, code, label) SELECT 3, k.code, p.name FROM sublocations p JOIN koha_codes k ON k.kind = 2 AND k.local_id = p.id; INSERT OR IGNORE INTO terms (vocabulary, code, label) SELECT 4, k.code, p.name FROM collections p JOIN koha_codes k ON k.kind = 3 AND k.local_id = p.id; UPDATE items SET location = (SELECT t.id FROM terms t JOIN koha_codes k ON k.kind = 2 AND k.code = t.code WHERE t.vocabulary = 3 AND k.local_id = items.sublocation_id), collection = (SELECT t.id FROM terms t JOIN koha_codes k ON k.kind = 3 AND k.code = t.code WHERE t.vocabulary = 4 AND k.local_id = items.collection_id); ALTER TABLE items DROP COLUMN sublocation_id; ALTER TABLE items DROP COLUMN collection_id; DROP TABLE sublocations; DROP TABLE collections; DROP TABLE koha_codes; ALTER TABLE branches DROP COLUMN address; ALTER TABLE branches DROP COLUMN phone; ALTER TABLE branches DROP COLUMN email; ALTER TABLE branches DROP COLUMN url; ALTER TABLE hubs ADD COLUMN extra TEXT NOT NULL DEFAULT '{}'; ALTER TABLE works ADD COLUMN extra TEXT NOT NULL DEFAULT '{}'; ALTER TABLE instances ADD COLUMN extra TEXT NOT NULL DEFAULT '{}'; ALTER TABLE items ADD COLUMN extra TEXT NOT NULL DEFAULT '{}'; DROP TABLE entity_classes; DROP TABLE statements; DROP TABLE lc_concepts; DROP TABLE ontology_names;
             DROP TABLE subject_identifiers; ALTER TABLE subjects DROP COLUMN name_parts; DROP TABLE contribution_identifiers; ALTER TABLE contributions DROP COLUMN role_term; ALTER TABLE agents DROP COLUMN name_parts; DROP INDEX subjects_identity; ALTER TABLE subjects DROP COLUMN subdivisions; CREATE UNIQUE INDEX subjects_identity ON subjects (kind, heading, coalesce(thesaurus, 0)); ALTER TABLE works ADD COLUMN material_type INTEGER; ALTER TABLE instances ADD COLUMN material_type INTEGER; ALTER TABLE subjects DROP COLUMN row_version; ALTER TABLE agents DROP COLUMN row_version; ALTER TABLE instances DROP COLUMN opac_hidden; DROP TABLE audit_entities; ALTER TABLE audit_log DROP COLUMN at; PRAGMA user_version = 7;",
        )
        .unwrap();
    drop(connection);
    let store = open(&dir);
    assert_eq!(all_heads(&store), before);
}
