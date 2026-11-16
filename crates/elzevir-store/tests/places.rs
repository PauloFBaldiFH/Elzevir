// SPDX-License-Identifier: GPL-3.0-or-later
//! Sublocations and collections per branch and the edge table of Koha codes
//! (docs/specs/catalogo/bibframe-completo.md, items 23a and 23b).
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "integration test: helper functions fail the test on error"
)]

use elzevir_app::catalog::{CatalogCommand, CatalogOutput, ItemData, ItemStatus, NewItem};
use elzevir_app::codes::{CodeTable, NamedCode};
use elzevir_app::commands::{WriteCommand, WriteOutput, WriteRefusal};
use elzevir_app::export::item_codes;
use elzevir_app::import::{ImportOptions, import_marc21};
use elzevir_domain::catalog::InstanceId;
use elzevir_marc::Limits;
use elzevir_store::{CommandFailed, ElzevirHandler, ElzevirStore, StoreConfig};

const STAFF_EXPORT: &[u8] =
    include_bytes!("../../elzevir-app/tests/data/koha-staff-bibs-1-200.mrc");

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

fn imported(dir: &tempfile::TempDir) -> ElzevirStore {
    let store = ElzevirStore::open(
        &dir.path().join("elzevir.db"),
        &StoreConfig::default(),
        ElzevirHandler,
    )
    .unwrap();
    let codes = item_codes(STAFF_EXPORT, Limits::default()).unwrap();
    let branches = codes.branches.into_iter().map(|code| {
        CatalogCommand::NameCodes(vec![NamedCode {
            table: CodeTable::Branch,
            name: code.clone(),
            code,
        }])
    });
    let types = codes.item_types.into_iter().map(|code| {
        CatalogCommand::NameCodes(vec![NamedCode {
            table: CodeTable::ItemType,
            name: code.clone(),
            code,
        }])
    });
    for command in branches.chain(types) {
        run(&store, command).unwrap();
    }
    import_marc21(STAFF_EXPORT, &store, &ImportOptions::default()).unwrap();
    store
}

fn count(store: &ElzevirStore, sql: &str) -> i64 {
    store
        .readers()
        .read(|c| c.query_row(sql, [], |row| row.get(0)))
        .unwrap()
}

/// Items whose places have no edge code or belong to another branch.
const OUT_OF_STEP: &str = "SELECT count(*) FROM items i
     LEFT JOIN sublocations s ON s.id = i.sublocation_id
     LEFT JOIN koha_codes sk ON sk.kind = 2 AND sk.local_id = s.id
     LEFT JOIN collections o ON o.id = i.collection_id
     LEFT JOIN koha_codes ok ON ok.kind = 3 AND ok.local_id = o.id
     WHERE (s.id IS NOT NULL AND sk.code IS NULL) OR (o.id IS NOT NULL AND ok.code IS NULL)
        OR (s.id IS NOT NULL AND s.branch_id <> i.home_branch)
        OR (o.id IS NOT NULL AND o.branch_id <> i.home_branch)";

fn item(code: &str, barcode: &str, location: &str) -> NewItem {
    NewItem {
        instance: InstanceId(1),
        item: ItemData {
            barcode: Some(barcode.to_owned()),
            inventory_number: None,
            home_branch: code.to_owned(),
            holding_branch: code.to_owned(),
            item_type: "T1".to_owned(),
            call_number: None,
            location: Some(location.to_owned()),
            collection: None,
            price: None,
            replacement_price: None,
            copy_number: None,
            status: ItemStatus::default(),
            acquired_on: None,
        },
    }
}

#[test]
fn imported_items_point_at_the_places_of_their_branch() {
    let dir = tempfile::tempdir().unwrap();
    let store = imported(&dir);
    assert!(
        count(
            &store,
            "SELECT count(*) FROM items WHERE sublocation_id IS NOT NULL"
        ) > 0
    );
    assert_eq!(count(&store, OUT_OF_STEP), 0);
    assert_eq!(
        count(&store, "SELECT count(*) FROM koha_codes WHERE kind = 1"),
        count(&store, "SELECT count(*) FROM branches")
    );
}

#[test]
fn the_same_code_in_two_branches_is_two_places_and_names_stay_unique() {
    let dir = tempfile::tempdir().unwrap();
    let store = imported(&dir);
    run(
        &store,
        CatalogCommand::NameCodes(vec![NamedCode {
            table: CodeTable::Branch,
            code: "FILIAL".to_owned(),
            name: "Filial".to_owned(),
        }]),
    )
    .unwrap();
    run(
        &store,
        CatalogCommand::NameCodes(vec![NamedCode {
            table: CodeTable::ItemType,
            code: "T1".to_owned(),
            name: "Tipo".to_owned(),
        }]),
    )
    .unwrap();
    for (barcode, location) in [("f-1", "INF"), ("f-2", "KIDS"), ("f-3", "INF")] {
        run(
            &store,
            CatalogCommand::AddItem(Box::new(item("FILIAL", barcode, location))),
        )
        .unwrap();
    }
    // Two codes with one name: the second place of a branch gets its code in the name.
    let named = |code: &str| NamedCode {
        table: CodeTable::Location,
        code: code.to_owned(),
        name: "Infantil".to_owned(),
    };
    run(
        &store,
        CatalogCommand::NameCodes(vec![named("INF"), named("KIDS")]),
    )
    .unwrap();
    let stored: Vec<String> = store
        .readers()
        .read(|c| {
            c.prepare(
                "SELECT s.name FROM sublocations s
                 JOIN koha_codes b ON b.kind = 1 AND b.local_id = s.branch_id
                 WHERE b.code = 'FILIAL' ORDER BY s.id",
            )?
            .query_map([], |row| row.get(0))?
            .collect()
        })
        .unwrap();
    assert_eq!(stored, ["Infantil", "Infantil (KIDS)"]);
    assert_eq!(count(&store, OUT_OF_STEP), 0);
}

#[test]
fn migration_14_fills_the_places_of_older_databases() {
    let dir = tempfile::tempdir().unwrap();
    let before = {
        let store = imported(&dir);
        let places = count(&store, "SELECT count(*) FROM sublocations");
        drop(store);
        places
    };
    assert!(before > 0);
    let connection = rusqlite::Connection::open(dir.path().join("elzevir.db")).unwrap();
    connection
        .execute_batch(
            "DROP TABLE koha_type_statements; ALTER TABLE circulation_policies ADD COLUMN code TEXT; UPDATE circulation_policies SET code = COALESCE((SELECT code FROM koha_codes WHERE kind = 4 AND local_id = circulation_policies.id), 'P' || id); ALTER TABLE circulation_policies RENAME COLUMN name TO description; ALTER TABLE circulation_policies RENAME TO item_types; ALTER TABLE items RENAME COLUMN policy_id TO item_type; DELETE FROM koha_codes WHERE kind = 4; ALTER TABLE branches ADD COLUMN code TEXT; UPDATE branches SET code = COALESCE((SELECT code FROM koha_codes WHERE kind = 1 AND local_id = branches.id), 'B' || id); ALTER TABLE items ADD COLUMN location INTEGER REFERENCES terms (id); ALTER TABLE items ADD COLUMN collection INTEGER REFERENCES terms (id); INSERT OR IGNORE INTO terms (vocabulary, code, label) SELECT 3, k.code, p.name FROM sublocations p JOIN koha_codes k ON k.kind = 2 AND k.local_id = p.id; INSERT OR IGNORE INTO terms (vocabulary, code, label) SELECT 4, k.code, p.name FROM collections p JOIN koha_codes k ON k.kind = 3 AND k.local_id = p.id; UPDATE items SET location = (SELECT t.id FROM terms t JOIN koha_codes k ON k.kind = 2 AND k.code = t.code WHERE t.vocabulary = 3 AND k.local_id = items.sublocation_id), collection = (SELECT t.id FROM terms t JOIN koha_codes k ON k.kind = 3 AND k.code = t.code WHERE t.vocabulary = 4 AND k.local_id = items.collection_id); ALTER TABLE items DROP COLUMN sublocation_id;
             ALTER TABLE items DROP COLUMN collection_id;
             DROP TABLE sublocations; DROP TABLE collections; DROP TABLE koha_codes;
             ALTER TABLE branches DROP COLUMN address; ALTER TABLE branches DROP COLUMN phone;
             ALTER TABLE branches DROP COLUMN email; ALTER TABLE branches DROP COLUMN url;
             DROP TABLE subject_identifiers; ALTER TABLE subjects DROP COLUMN name_parts; DROP TABLE contribution_identifiers; ALTER TABLE contributions DROP COLUMN role_term; ALTER TABLE agents DROP COLUMN name_parts; DROP INDEX subjects_identity; ALTER TABLE subjects DROP COLUMN subdivisions; CREATE UNIQUE INDEX subjects_identity ON subjects (kind, heading, coalesce(thesaurus, 0)); ALTER TABLE works ADD COLUMN material_type INTEGER; ALTER TABLE instances ADD COLUMN material_type INTEGER; ALTER TABLE subjects DROP COLUMN row_version; ALTER TABLE agents DROP COLUMN row_version; ALTER TABLE instances DROP COLUMN opac_hidden; DROP TABLE audit_entities; ALTER TABLE audit_log DROP COLUMN at; PRAGMA user_version = 13;",
        )
        .unwrap();
    drop(connection);
    let store = ElzevirStore::open(
        &dir.path().join("elzevir.db"),
        &StoreConfig::default(),
        ElzevirHandler,
    )
    .unwrap();
    assert_eq!(count(&store, "SELECT count(*) FROM sublocations"), before);
    assert_eq!(count(&store, OUT_OF_STEP), 0);
}
