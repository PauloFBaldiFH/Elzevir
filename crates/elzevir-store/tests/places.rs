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
    let branches = codes
        .branches
        .into_iter()
        .map(|code| CatalogCommand::CreateBranch {
            name: code.clone(),
            code,
        });
    let types = codes
        .item_types
        .into_iter()
        .map(|code| CatalogCommand::CreateItemType {
            description: code.clone(),
            code,
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

/// Items whose registry rows do not stand for the codes they carry.
const OUT_OF_STEP: &str = "SELECT count(*) FROM items i
     LEFT JOIN terms l ON l.id = i.location
     LEFT JOIN terms c ON c.id = i.collection
     LEFT JOIN sublocations s ON s.id = i.sublocation_id
     LEFT JOIN koha_codes sk ON sk.kind = 2 AND sk.local_id = s.id
     LEFT JOIN collections o ON o.id = i.collection_id
     LEFT JOIN koha_codes ok ON ok.kind = 3 AND ok.local_id = o.id
     WHERE l.code IS NOT sk.code OR c.code IS NOT ok.code
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
            "SELECT count(*) FROM items WHERE location IS NOT NULL"
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
        CatalogCommand::CreateBranch {
            code: "FILIAL".to_owned(),
            name: "Filial".to_owned(),
        },
    )
    .unwrap();
    run(
        &store,
        CatalogCommand::CreateItemType {
            code: "T1".to_owned(),
            description: "Tipo".to_owned(),
        },
    )
    .unwrap();
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
    for (barcode, location) in [("f-1", "INF"), ("f-2", "KIDS"), ("f-3", "INF")] {
        run(
            &store,
            CatalogCommand::AddItem(Box::new(item("FILIAL", barcode, location))),
        )
        .unwrap();
    }
    let stored: Vec<String> = store
        .readers()
        .read(|c| {
            c.prepare(
                "SELECT s.name FROM sublocations s JOIN branches b ON b.id = s.branch_id
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
            "ALTER TABLE items DROP COLUMN sublocation_id;
             ALTER TABLE items DROP COLUMN collection_id;
             DROP TABLE sublocations; DROP TABLE collections; DROP TABLE koha_codes;
             ALTER TABLE branches DROP COLUMN address; ALTER TABLE branches DROP COLUMN phone;
             ALTER TABLE branches DROP COLUMN email; ALTER TABLE branches DROP COLUMN url;
             PRAGMA user_version = 13;",
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
