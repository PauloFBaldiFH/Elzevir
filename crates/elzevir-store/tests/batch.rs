// SPDX-License-Identifier: GPL-3.0-or-later
//! Every Item of some Instances changed in one transaction (docs/specs/catalogo/lote.md,
//! items 10 to 13).
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "integration test: helper functions fail the test on error"
)]

use elzevir_app::batch::{ItemSet, ItemsChange, MAX_BATCH_ITEMS};
use elzevir_app::catalog::{CatalogCommand, CatalogOutput, CatalogRefusal};
use elzevir_app::codes::{CodeTable, NamedCode};
use elzevir_app::commands::{WriteCommand, WriteOutput, WriteRefusal};
use elzevir_app::export::item_codes;
use elzevir_app::import::{ImportOptions, import_marc21};
use elzevir_app::items::{ItemColumn, RowProblem};
use elzevir_app::registries::{RegistryChange, RegistryEntry, RegistryList};
use elzevir_domain::catalog::{BranchId, CollectionId, InstanceId, SublocationId};
use elzevir_marc::Limits;
use elzevir_store::{CommandFailed, ElzevirHandler, ElzevirStore, StoreConfig};

const STAFF_EXPORT: &[u8] =
    include_bytes!("../../elzevir-app/tests/data/koha-staff-bibs-1-200.mrc");

fn imported(dir: &tempfile::TempDir) -> ElzevirStore {
    let store = ElzevirStore::open(
        &dir.path().join("elzevir.db"),
        &StoreConfig::default(),
        ElzevirHandler,
    )
    .unwrap();
    // The file's branches and item types must be named before its Items come in.
    let codes = item_codes(STAFF_EXPORT, Limits::default()).unwrap();
    let branches = codes
        .branches
        .into_iter()
        .map(|code| (CodeTable::Branch, code));
    let types = codes
        .item_types
        .into_iter()
        .map(|code| (CodeTable::ItemType, code));
    for (table, code) in branches.chain(types) {
        run(
            &store,
            CatalogCommand::NameCodes(vec![NamedCode {
                table,
                name: code.clone(),
                code,
            }]),
        )
        .unwrap();
    }
    import_marc21(STAFF_EXPORT, &store, &ImportOptions::default()).unwrap();
    store
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

fn refusal(result: Result<CatalogOutput, CommandFailed<WriteRefusal>>) -> CatalogRefusal {
    match result {
        Err(CommandFailed::Rejected(WriteRefusal::Catalog(refusal))) => refusal,
        other => panic!("not a refusal: {other:?}"),
    }
}

fn count(store: &ElzevirStore, sql: &str) -> i64 {
    store
        .readers()
        .read(|c| c.query_row(sql, [], |row| row.get(0)))
        .unwrap()
}

fn create(store: &ElzevirStore, list: RegistryList, name: &str) -> RegistryEntry {
    let CatalogOutput::RegistryChanged(Some(entry)) = run(
        store,
        CatalogCommand::ChangeRegistry(RegistryChange::Create {
            list,
            name: name.to_owned(),
        }),
    )
    .unwrap() else {
        panic!("nothing created");
    };
    entry
}

/// A new branch with one shelving place and one collection of its own.
fn branch_with_places(store: &ElzevirStore) -> (BranchId, SublocationId, CollectionId) {
    let RegistryEntry::Branch(branch) = create(store, RegistryList::Branches, "Filial Leste")
    else {
        panic!("a branch");
    };
    let RegistryEntry::Sublocation(location) =
        create(store, RegistryList::Sublocations(branch), "Infantil")
    else {
        panic!("a shelving place");
    };
    let RegistryEntry::Collection(collection) =
        create(store, RegistryList::Collections(branch), "Doações")
    else {
        panic!("a collection");
    };
    (branch, location, collection)
}

/// The Instances with Items, in id order, until their Items reach `items`.
fn instances_reaching(store: &ElzevirStore, items: i64) -> Vec<InstanceId> {
    store
        .readers()
        .read(|c| {
            let mut statement = c.prepare(
                "SELECT instance_id, count(*) FROM items GROUP BY instance_id ORDER BY instance_id",
            )?;
            let mut total = 0;
            let mut chosen = Vec::new();
            for row in statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))? {
                let (instance, n): (i64, i64) = row?;
                if total >= items {
                    break;
                }
                total += n;
                chosen.push(InstanceId(instance));
            }
            Ok::<_, rusqlite::Error>(chosen)
        })
        .unwrap()
}

/// Copies of every Item, without barcode, until the catalog holds `items`: the fixture
/// holds fewer.
fn grow_to(store: &ElzevirStore, dir: &tempfile::TempDir, items: i64) {
    let db = rusqlite::Connection::open(dir.path().join("elzevir.db")).unwrap();
    while count(store, "SELECT count(*) FROM items") < items {
        db.execute_batch(
            "INSERT INTO items (instance_id, home_branch, holding_branch, policy_id,
                                sublocation_id, collection_id)
             SELECT instance_id, home_branch, holding_branch, policy_id, sublocation_id,
                    collection_id
             FROM items",
        )
        .unwrap();
    }
}

fn items_of(instances: &[InstanceId]) -> String {
    let ids: Vec<String> = instances.iter().map(|id| id.0.to_string()).collect();
    format!("instance_id IN ({})", ids.join(","))
}

#[test]
fn every_item_of_the_instances_moves_to_another_branch_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let store = imported(&dir);
    let (branch, location, collection) = branch_with_places(&store);
    grow_to(&store, &dir, 500);
    let instances = instances_reaching(&store, 500);
    let within = items_of(&instances);
    let items = count(
        &store,
        &format!("SELECT count(*) FROM items WHERE {within}"),
    );
    assert!(items >= 500, "the fixture has {items} Items");
    let versions = count(
        &store,
        &format!(
            "SELECT sum(row_version) FROM instances WHERE id IN (SELECT instance_id FROM items WHERE {within})"
        ),
    );
    let reindexed = count(
        &store,
        "SELECT count(*) FROM effect_outbox WHERE kind = 'catalog.reindex_instance'",
    );
    let output = run(
        &store,
        CatalogCommand::ChangeItems(Box::new(ItemsChange {
            // An Instance named twice counts once.
            instances: [instances.clone(), vec![instances[0]]].concat(),
            set: vec![
                ItemSet::HomeBranch(branch),
                ItemSet::HoldingBranch(branch),
                ItemSet::Location(Some(location)),
                ItemSet::Collection(Some(collection)),
            ],
        })),
    )
    .unwrap();
    assert_eq!(
        output,
        CatalogOutput::ItemsChanged {
            instances: instances.clone(),
            items: usize::try_from(items).unwrap(),
        }
    );
    let moved = count(
        &store,
        &format!(
            "SELECT count(*) FROM items WHERE {within} AND home_branch = {} AND holding_branch = {}
             AND sublocation_id = {} AND collection_id = {}",
            branch.0, branch.0, location.0, collection.0
        ),
    );
    assert_eq!(moved, items);
    let n = i64::try_from(instances.len()).unwrap();
    assert_eq!(
        count(
            &store,
            &format!(
                "SELECT sum(row_version) FROM instances WHERE id IN (SELECT instance_id FROM items WHERE {within})"
            ),
        ),
        versions + n
    );
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM effect_outbox WHERE kind = 'catalog.reindex_instance'"
        ),
        reindexed + n
    );
    // The history of each Instance reaches the command.
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM audit_entities e JOIN audit_log a ON a.seq = e.audit_seq
             WHERE a.kind = 'catalog.change_items' AND e.entity_kind = 3"
        ),
        n
    );
    // Clearing goes through too.
    run(
        &store,
        CatalogCommand::ChangeItems(Box::new(ItemsChange {
            instances: instances.clone(),
            set: vec![ItemSet::Location(None), ItemSet::Collection(None)],
        })),
    )
    .unwrap();
    assert_eq!(
        count(
            &store,
            &format!(
                "SELECT count(*) FROM items WHERE {within}
                 AND sublocation_id IS NULL AND collection_id IS NULL"
            )
        ),
        items
    );
}

#[test]
fn a_place_of_another_branch_refuses_the_batch_and_names_each_item() {
    let dir = tempfile::tempdir().unwrap();
    let store = imported(&dir);
    let (_, location, _) = branch_with_places(&store);
    let instances = instances_reaching(&store, 3);
    let within = items_of(&instances);
    let items = count(
        &store,
        &format!("SELECT count(*) FROM items WHERE {within}"),
    );
    let before = count(&store, "SELECT sum(row_version) FROM items");
    let CatalogRefusal::BatchItems(refusals) = refusal(run(
        &store,
        CatalogCommand::ChangeItems(Box::new(ItemsChange {
            instances,
            set: vec![ItemSet::Location(Some(location))],
        })),
    )) else {
        panic!("not a refusal per Item");
    };
    assert_eq!(i64::try_from(refusals.len()).unwrap(), items);
    assert!(refusals.iter().all(|refusal| {
        refusal.column == ItemColumn::Location
            && refusal.problem == RowProblem::NotInBranch
            && !refusal.label.is_empty()
    }));
    assert_eq!(count(&store, "SELECT sum(row_version) FROM items"), before);
}

#[test]
fn a_batch_without_changes_instances_or_room_is_refused_whole() {
    let dir = tempfile::tempdir().unwrap();
    let store = imported(&dir);
    let (branch, _, _) = branch_with_places(&store);
    let change = |instances: Vec<InstanceId>, set: Vec<ItemSet>| {
        refusal(run(
            &store,
            CatalogCommand::ChangeItems(Box::new(ItemsChange { instances, set })),
        ))
    };
    assert_eq!(
        change(vec![InstanceId(1)], Vec::new()),
        CatalogRefusal::NothingToChange
    );
    assert_eq!(
        change(Vec::new(), vec![ItemSet::HoldingBranch(branch)]),
        CatalogRefusal::NothingToChange
    );
    assert_eq!(
        change(
            vec![InstanceId(1), InstanceId(99_999)],
            vec![ItemSet::HoldingBranch(branch)]
        ),
        CatalogRefusal::UnknownInstance(InstanceId(99_999))
    );
    // More Items than a batch reaches: copies of one Item fill an Instance past the cap.
    let instance = instances_reaching(&store, 1)[0];
    rusqlite::Connection::open(dir.path().join("elzevir.db"))
        .unwrap()
        .execute_batch(&format!(
            "WITH RECURSIVE n(k) AS (SELECT 1 UNION ALL SELECT k + 1 FROM n WHERE k < {MAX_BATCH_ITEMS})
             INSERT INTO items (instance_id, home_branch, holding_branch, policy_id)
             SELECT instance_id, home_branch, holding_branch, policy_id
             FROM (SELECT * FROM items WHERE instance_id = {} LIMIT 1), n",
            instance.0
        ))
        .unwrap();
    let CatalogRefusal::TooManyItems { count, max } =
        change(vec![instance], vec![ItemSet::HoldingBranch(branch)])
    else {
        panic!("not too many");
    };
    assert_eq!(max, MAX_BATCH_ITEMS);
    assert!(count > MAX_BATCH_ITEMS);
}
