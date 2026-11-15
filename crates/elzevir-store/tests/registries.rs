// SPDX-License-Identifier: GPL-3.0-or-later
//! The registries of branches, shelving places, collections and circulation policies
//! (docs/specs/catalogo/bibframe-completo.md, items 23d and 23f).
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "integration test: helper functions fail the test on error"
)]

use elzevir_app::catalog::{CatalogCommand, CatalogOutput, CatalogReader, CatalogRefusal};
use elzevir_app::commands::{WriteCommand, WriteOutput, WriteRefusal};
use elzevir_app::import::{ImportOptions, import_marc21};
use elzevir_app::registries::{
    BranchContact, RegistryChange, RegistryEntry, RegistryList, RegistryRefusal,
};
use elzevir_domain::catalog::BranchId;
use elzevir_store::{CommandFailed, ElzevirHandler, ElzevirStore, StoreConfig};

const STAFF_EXPORT: &[u8] =
    include_bytes!("../../elzevir-app/tests/data/koha-staff-bibs-1-200.mrc");

fn store(dir: &tempfile::TempDir) -> ElzevirStore {
    ElzevirStore::open(
        &dir.path().join("elzevir.db"),
        &StoreConfig::default(),
        ElzevirHandler,
    )
    .unwrap()
}

fn change(
    store: &ElzevirStore,
    change: RegistryChange,
) -> Result<Option<RegistryEntry>, RegistryRefusal> {
    match store
        .submit(WriteCommand::Catalog(CatalogCommand::ChangeRegistry(
            change,
        )))
        .unwrap()
        .wait()
    {
        Ok(WriteOutput::Catalog(CatalogOutput::RegistryChanged(entry))) => Ok(entry),
        Err(CommandFailed::Rejected(WriteRefusal::Catalog(CatalogRefusal::Registry(refusal)))) => {
            Err(refusal)
        }
        other => panic!("unexpected answer {other:?}"),
    }
}

fn create(list: RegistryList, name: &str) -> RegistryChange {
    RegistryChange::Create {
        list,
        name: name.to_owned(),
    }
}

#[test]
fn rows_are_created_renamed_and_deleted_with_unique_names() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(&dir);
    let Some(RegistryEntry::Branch(central)) = change(
        &store,
        create(RegistryList::Branches, " Biblioteca Central "),
    )
    .unwrap() else {
        panic!("a branch");
    };
    let Some(RegistryEntry::Branch(east)) =
        change(&store, create(RegistryList::Branches, "Filial Leste")).unwrap()
    else {
        panic!("a branch");
    };
    assert_eq!(
        change(&store, create(RegistryList::Branches, "Biblioteca Central")),
        Err(RegistryRefusal::NameTaken)
    );
    // One name per branch: two branches may each have their "Infantil".
    let infantil = change(
        &store,
        create(RegistryList::Sublocations(central), "Infantil"),
    )
    .unwrap()
    .unwrap();
    change(&store, create(RegistryList::Sublocations(east), "Infantil")).unwrap();
    assert_eq!(
        change(
            &store,
            create(RegistryList::Sublocations(central), "Infantil")
        ),
        Err(RegistryRefusal::NameTaken)
    );
    change(
        &store,
        create(RegistryList::Sublocations(central), "Referência"),
    )
    .unwrap();
    assert_eq!(
        change(
            &store,
            RegistryChange::Rename {
                entry: infantil,
                name: "Referência".to_owned()
            }
        ),
        Err(RegistryRefusal::NameTaken)
    );
    change(
        &store,
        RegistryChange::Rename {
            entry: infantil,
            name: "Infantojuvenil".to_owned(),
        },
    )
    .unwrap();
    assert_eq!(
        change(
            &store,
            create(RegistryList::Sublocations(BranchId(99)), "X")
        ),
        Err(RegistryRefusal::Unknown)
    );
    let codes = store.codes().unwrap();
    assert_eq!(codes.branch(central), "Biblioteca Central");
    assert!(
        codes
            .locations
            .iter()
            .any(|place| place.branch == central && place.name == "Infantojuvenil")
    );
    // A branch with shelving places is in use; emptied, it goes.
    assert_eq!(
        change(&store, RegistryChange::Delete(RegistryEntry::Branch(east))),
        Err(RegistryRefusal::InUse)
    );
    let east_place = codes
        .locations
        .iter()
        .find(|place| place.branch == east)
        .unwrap()
        .id;
    change(
        &store,
        RegistryChange::Delete(RegistryEntry::Sublocation(east_place)),
    )
    .unwrap();
    change(&store, RegistryChange::Delete(RegistryEntry::Branch(east))).unwrap();
    assert_eq!(store.codes().unwrap().branches.len(), 1);
}

#[test]
fn a_branch_keeps_its_contact_data() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(&dir);
    let Some(RegistryEntry::Branch(central)) =
        change(&store, create(RegistryList::Branches, "Biblioteca Central")).unwrap()
    else {
        panic!("a branch");
    };
    change(
        &store,
        RegistryChange::Contact {
            branch: central,
            contact: BranchContact {
                address: Some("Rua A, 1".to_owned()),
                ..BranchContact::default()
            },
        },
    )
    .unwrap();

    let codes = store.codes().unwrap();
    assert_eq!(
        codes.contacts,
        [(
            central,
            BranchContact {
                address: Some("Rua A, 1".to_owned()),
                ..BranchContact::default()
            }
        )]
    );
}

#[test]
fn what_items_use_stays_and_its_edge_code_goes_with_it() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(&dir);
    let codes =
        elzevir_app::export::item_codes(STAFF_EXPORT, elzevir_marc::Limits::default()).unwrap();
    let named = |table, code: String| elzevir_app::codes::NamedCode {
        table,
        name: code.clone(),
        code,
    };
    let new: Vec<_> = codes
        .branches
        .into_iter()
        .map(|code| named(elzevir_app::codes::CodeTable::Branch, code))
        .chain(
            codes
                .item_types
                .into_iter()
                .map(|code| named(elzevir_app::codes::CodeTable::ItemType, code)),
        )
        .collect();
    store
        .submit(WriteCommand::Catalog(CatalogCommand::NameCodes(new)))
        .unwrap()
        .wait()
        .unwrap();
    import_marc21(STAFF_EXPORT, &store, &ImportOptions::default()).unwrap();
    let codes = store.codes().unwrap();
    let used = codes.policies[0].id;
    assert_eq!(
        change(&store, RegistryChange::Delete(RegistryEntry::Policy(used))),
        Err(RegistryRefusal::InUse)
    );
    let Some(RegistryEntry::Policy(spare)) =
        change(&store, create(RegistryList::Policies, "Sem uso")).unwrap()
    else {
        panic!("a policy");
    };
    change(&store, RegistryChange::Delete(RegistryEntry::Policy(spare))).unwrap();
    let place = codes.locations[0].id;
    assert_eq!(
        change(
            &store,
            RegistryChange::Delete(RegistryEntry::Sublocation(place))
        ),
        Err(RegistryRefusal::InUse)
    );
    // A policy named from a Koha code no Item uses: deleting it drops the code too.
    store
        .submit(WriteCommand::Catalog(CatalogCommand::NameCodes(vec![
            named(elzevir_app::codes::CodeTable::ItemType, "UNUSED".to_owned()),
        ])))
        .unwrap()
        .wait()
        .unwrap();
    let codes = store.codes().unwrap();
    let unused = codes
        .policies
        .iter()
        .find(|policy| policy.name == "UNUSED")
        .unwrap()
        .id;
    change(
        &store,
        RegistryChange::Delete(RegistryEntry::Policy(unused)),
    )
    .unwrap();
    assert!(
        !store
            .codes()
            .unwrap()
            .edge
            .iter()
            .any(|entry| entry.code == "UNUSED")
    );
}
