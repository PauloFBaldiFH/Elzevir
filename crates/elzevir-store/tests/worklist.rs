// SPDX-License-Identifier: GPL-3.0-or-later
//! The work lists read the `audit_log` (docs/specs/catalogo/facetas.md, items 15 and 16).
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "integration test: helper functions fail the test on error"
)]

use std::collections::BTreeSet;
use std::num::NonZeroUsize;

use elzevir_app::catalog::{
    CatalogCommand, CatalogReader, InstanceChange, InstanceUpdate, WorkChange, WorkUpdate,
};
use elzevir_app::catalog::{CatalogOutput, CatalogRefusal};
use elzevir_app::codes::{CodeTable, NamedCode};
use elzevir_app::commands::{StaffCommand, WriteCommand, WriteOutput};
use elzevir_app::editing::{CatalogEditor, Edit};
use elzevir_app::export::item_codes;
use elzevir_app::import::{ImportOptions, import_marc21};
use elzevir_app::listing::FamilyHead;
use elzevir_app::staff::StaffId;
use elzevir_app::worklist::{
    MAX_LIST_FAMILIES, SavedList, SavedListChange, SavedListId, SavedListRefusal, WorkListQuery,
    WorkListReader,
};

use elzevir_domain::catalog::{HubId, InstanceId, WorkId};
use elzevir_marc::Limits;
use elzevir_store::{ElzevirHandler, ElzevirStore, StoreConfig};

const STAFF_EXPORT: &[u8] =
    include_bytes!("../../elzevir-app/tests/data/koha-staff-bibs-1-200.mrc");

const PAGE: NonZeroUsize = NonZeroUsize::new(500).unwrap();

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
        let named = CatalogCommand::NameCodes(vec![NamedCode {
            table,
            name: code.clone(),
            code,
        }]);
        store
            .submit(WriteCommand::Catalog(named))
            .unwrap()
            .wait()
            .unwrap();
    }
    import_marc21(STAFF_EXPORT, &store, &ImportOptions::default()).unwrap();
    store
}

fn account(store: &ElzevirStore, login: &str) -> StaffId {
    let WriteOutput::StaffCreated(id) = store
        .submit(WriteCommand::Staff(StaffCommand::Create {
            login: login.to_owned(),
            password_hash: "x".to_owned(),
            permissions: BTreeSet::new(),
        }))
        .unwrap()
        .wait()
        .unwrap()
    else {
        panic!("no account");
    };
    id
}

/// The family an Instance belongs to.
fn family_of(store: &ElzevirStore, instance: i64) -> FamilyHead {
    store
        .readers()
        .read(|c| {
            c.query_row(
                "SELECT w.hub_id, w.id FROM instances i JOIN works w ON w.id = i.work_id
                 WHERE i.id = ?1",
                [instance],
                |row| {
                    let hub: Option<i64> = row.get(0)?;
                    let work: i64 = row.get(1)?;
                    Ok(hub.map_or(FamilyHead::Work(WorkId(work)), |hub| {
                        FamilyHead::Hub(HubId(hub))
                    }))
                },
            )
        })
        .unwrap()
}

fn edit_summary(store: &ElzevirStore, staff: StaffId, instance: i64) {
    let view = store.instance(InstanceId(instance)).unwrap().unwrap();
    store
        .apply(
            staff,
            Edit::Work(WorkUpdate {
                work: view.work,
                row_version: view.work_version,
                changes: vec![WorkChange::Summary(Some("Resumo novo.".to_owned()))],
            }),
        )
        .unwrap();
}

fn edit_edition(store: &ElzevirStore, staff: StaffId, instance: i64) {
    let view = store.instance(InstanceId(instance)).unwrap().unwrap();
    store
        .apply(
            staff,
            Edit::Instance(InstanceUpdate {
                instance: view.id,
                row_version: view.version,
                changes: vec![InstanceChange::Edition(Some("2. ed.".to_owned()))],
            }),
        )
        .unwrap();
}

#[test]
fn my_recent_families_are_mine_newest_first_each_once() {
    let dir = tempfile::tempdir().unwrap();
    let store = imported(&dir);
    let ana = account(&store, "ana");
    let bia = account(&store, "bia");
    edit_summary(&store, ana, 1);
    edit_edition(&store, bia, 2);
    edit_edition(&store, ana, 3);
    // The Work of Instance 1 again, through its Instance: still one line, now the latest.
    edit_edition(&store, ana, 1);
    let recent = |staff| {
        store
            .heads(WorkListQuery::TouchedBy(staff), 0, PAGE)
            .unwrap()
    };
    assert_eq!(recent(ana), [family_of(&store, 1), family_of(&store, 3)]);
    assert_eq!(recent(bia), [family_of(&store, 2)]);
    assert_eq!(recent(StaffId(999)), []);
    let second = store.heads(WorkListQuery::TouchedBy(ana), 1, PAGE).unwrap();
    assert_eq!(second, [family_of(&store, 3)]);
}

#[test]
fn imported_today_lists_the_families_a_file_brought_since_the_instant() {
    let dir = tempfile::tempdir().unwrap();
    let store = imported(&dir);
    let ana = account(&store, "ana");
    edit_summary(&store, ana, 5);
    // Every import ran on the morning of 2026-11-16, in UTC.
    rusqlite::Connection::open(dir.path().join("elzevir.db"))
        .unwrap()
        .execute(
            "UPDATE audit_log SET at = '2026-11-16T10:00:00Z' WHERE kind <> 'catalog.update_work'",
            [],
        )
        .unwrap();
    let since = |at: &str| {
        store
            .heads(WorkListQuery::ImportedSince(at.parse().unwrap()), 0, PAGE)
            .unwrap()
    };
    let all = since("2026-11-16T03:00:00Z");
    let families: i64 = store
        .readers()
        .read(|c| {
            c.query_row(
                "SELECT count(DISTINCT coalesce('h' || hub_id, 'w' || id)) FROM works",
                [],
                |row| row.get(0),
            )
        })
        .unwrap();
    assert_eq!(i64::try_from(all.len()).unwrap(), families);
    // The last record of the file first; an edit does not move a family up.
    let last: i64 = store
        .readers()
        .read(|c| c.query_row("SELECT max(id) FROM instances", [], |row| row.get(0)))
        .unwrap();
    assert_eq!(all.first(), Some(&family_of(&store, last)));
    assert_eq!(
        since("2026-11-16T10:00:00Z").len(),
        all.len(),
        "the instant counts"
    );
    assert_eq!(since("2026-11-17T03:00:00Z"), []);
}

fn change_list(
    store: &ElzevirStore,
    staff: StaffId,
    change: SavedListChange,
) -> Result<SavedListId, SavedListRefusal> {
    match store.apply(staff, Edit::SavedList(change)) {
        Ok(CatalogOutput::SavedListChanged(list)) => Ok(list),
        Err(elzevir_app::editing::EditFailed::Refused(CatalogRefusal::SavedList(refusal))) => {
            Err(refusal)
        }
        other => panic!("unexpected {other:?}"),
    }
}

fn work(n: i64) -> FamilyHead {
    FamilyHead::Work(WorkId(n))
}

#[test]
fn a_saved_list_keeps_families_once_the_last_added_first() {
    let dir = tempfile::tempdir().unwrap();
    let store = imported(&dir);
    let ana = account(&store, "ana");
    let heads = |list| store.heads(WorkListQuery::Saved(list), 0, PAGE).unwrap();
    let list = change_list(
        &store,
        ana,
        SavedListChange::Create {
            name: "  Doações de 2026 ".to_owned(),
            heads: vec![work(1), work(2), work(1)],
        },
    )
    .unwrap();
    assert_eq!(heads(list), [work(2), work(1)]);
    // A family already there stays where it is.
    change_list(
        &store,
        ana,
        SavedListChange::Add {
            list,
            heads: vec![work(3), work(1)],
        },
    )
    .unwrap();
    assert_eq!(heads(list), [work(3), work(2), work(1)]);
    change_list(
        &store,
        ana,
        SavedListChange::Remove {
            list,
            heads: vec![work(2)],
        },
    )
    .unwrap();
    assert_eq!(heads(list), [work(3), work(1)]);
    assert_eq!(
        store.heads(WorkListQuery::Saved(list), 1, PAGE).unwrap(),
        [work(1)]
    );
    let other = change_list(
        &store,
        ana,
        SavedListChange::Create {
            name: "Avaliar".to_owned(),
            heads: Vec::new(),
        },
    )
    .unwrap();
    assert_eq!(
        store.saved_lists().unwrap(),
        [
            SavedList {
                id: other,
                name: "Avaliar".to_owned(),
                families: 0
            },
            SavedList {
                id: list,
                name: "Doações de 2026".to_owned(),
                families: 2
            },
        ]
    );
    // Saving lists does not touch the records, so it is not a recent family.
    assert_eq!(
        store.heads(WorkListQuery::TouchedBy(ana), 0, PAGE).unwrap(),
        []
    );
    change_list(&store, ana, SavedListChange::Delete { list }).unwrap();
    assert_eq!(heads(list), []);
    assert_eq!(store.saved_lists().unwrap().len(), 1);
}

#[test]
fn a_taken_name_a_full_list_or_an_unknown_one_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let store = imported(&dir);
    let ana = account(&store, "ana");
    let create = |name: &str, heads: Vec<FamilyHead>| {
        change_list(
            &store,
            ana,
            SavedListChange::Create {
                name: name.to_owned(),
                heads,
            },
        )
    };
    let list = create("Doações", Vec::new()).unwrap();
    assert_eq!(
        create("DOAÇÕES", Vec::new()),
        Err(SavedListRefusal::NameTaken)
    );
    assert_eq!(create(" ", Vec::new()), Err(SavedListRefusal::BadName));
    let full: Vec<FamilyHead> = (1..=i64::try_from(MAX_LIST_FAMILIES).unwrap())
        .map(work)
        .collect();
    change_list(&store, ana, SavedListChange::Add { list, heads: full }).unwrap();
    assert_eq!(
        change_list(
            &store,
            ana,
            SavedListChange::Add {
                list,
                heads: vec![work(1), work(999_999)]
            }
        ),
        Err(SavedListRefusal::TooManyFamilies)
    );
    assert_eq!(
        change_list(
            &store,
            ana,
            SavedListChange::Delete {
                list: SavedListId(404)
            }
        ),
        Err(SavedListRefusal::Unknown)
    );
}
