// SPDX-License-Identifier: GPL-3.0-or-later
//! Inventory by reading barcodes off the shelf (docs/specs/catalogo/inventario.md).
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "integration test: helper functions fail the test on error, and the fixture's seven Items are known"
)]

use elzevir_app::catalog::{CatalogCommand, CatalogOutput, CatalogRefusal};
use elzevir_app::codes::{CodeTable, NamedCode};
use elzevir_app::commands::{WriteCommand, WriteOutput, WriteRefusal};
use elzevir_app::export::item_codes;
use elzevir_app::import::{ImportOptions, import_marc21};
use elzevir_app::inventory::{InventoryArea, InventoryFilters, InventoryReport, InventoryScan};
use elzevir_app::registries::{RegistryChange, RegistryEntry, RegistryList};
use elzevir_domain::catalog::{BranchId, ItemId, SublocationId};
use elzevir_domain::inventory::InventoryProblem;
use elzevir_domain::inventory::InventoryProblem::{
    NoBarcode, NotScanned, OutOfOrder, UnknownBarcode, WasLost, Withdrawn, WrongPlace,
};
use elzevir_marc::Limits;
use elzevir_store::{CommandFailed, ElzevirHandler, ElzevirStore, StoreConfig};
use jiff::civil::date;

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
    let branches = codes.branches.into_iter().map(|c| (CodeTable::Branch, c));
    let types = codes
        .item_types
        .into_iter()
        .map(|c| (CodeTable::ItemType, c));
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

/// Seven Items moved to a shelf of a new branch: `869.1` to `869.8`, the fourth with
/// `900`; the third lost, the fifth withdrawn, the seventh without a barcode. Their ids and
/// barcodes.
fn shelf(
    store: &ElzevirStore,
    dir: &tempfile::TempDir,
) -> (BranchId, SublocationId, Vec<(ItemId, String)>) {
    let RegistryEntry::Branch(branch) = create(store, RegistryList::Branches, "Filial Leste")
    else {
        panic!("a branch");
    };
    let RegistryEntry::Sublocation(place) =
        create(store, RegistryList::Sublocations(branch), "Literatura")
    else {
        panic!("a shelving place");
    };
    let items: Vec<(ItemId, String)> = store
        .readers()
        .read(|c| {
            let mut statement = c.prepare(
                "SELECT id, barcode FROM items WHERE barcode IS NOT NULL ORDER BY id LIMIT 7",
            )?;
            statement
                .query_map([], |row| Ok((ItemId(row.get(0)?), row.get(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .unwrap();
    let connection = rusqlite::Connection::open(dir.path().join("elzevir.db")).unwrap();
    for (n, (item, _)) in items.iter().enumerate() {
        let call_number = if n == 3 {
            "900 X".to_owned()
        } else {
            format!("869.{} A{n}", n + 1)
        };
        connection
            .execute(
                "UPDATE items SET home_branch = ?2, sublocation_id = ?3, call_number = ?4,
                        lost = ?5, withdrawn = ?6, last_seen = NULL
                 WHERE id = ?1",
                rusqlite::params![
                    item.0,
                    branch.0,
                    place.0,
                    call_number,
                    i64::from(n == 2),
                    i64::from(n == 4),
                ],
            )
            .unwrap();
    }
    connection
        .execute(
            "UPDATE items SET barcode = NULL WHERE id = ?1",
            [items[6].0.0],
        )
        .unwrap();
    (branch, place, items)
}

fn inventory(store: &ElzevirStore, scan: InventoryScan) -> InventoryReport {
    let CatalogOutput::Inventory(report) =
        run(store, CatalogCommand::Inventory(Box::new(scan))).unwrap()
    else {
        panic!("not an inventory");
    };
    *report
}

fn lines(report: &InventoryReport) -> Vec<(Option<ItemId>, Vec<InventoryProblem>)> {
    report
        .lines
        .iter()
        .map(|line| (line.item, line.problems.clone()))
        .collect()
}

#[test]
fn a_reading_marks_the_items_seen_and_reports_what_is_wrong() {
    let dir = tempfile::tempdir().unwrap();
    let store = imported(&dir);
    let (branch, place, items) = shelf(&store, &dir);
    let id = |n: usize| items[n].0;
    let barcode = |n: usize| items[n].1.clone();
    let area = InventoryArea {
        branch: Some(branch),
        sublocation: Some(place),
        call_number_from: Some("869".to_owned()),
        ..InventoryArea::default()
    };
    let scan = |barcodes: Vec<String>, filters| InventoryScan {
        barcodes,
        seen_on: date(2026, 11, 26),
        area: area.clone(),
        filters,
        compare: true,
        out_of_order: true,
    };
    let report = inventory(
        &store,
        scan(
            vec![
                barcode(1),
                barcode(0),
                barcode(2).to_uppercase(),
                barcode(3),
                barcode(4),
                "NAO-EXISTE".to_owned(),
            ],
            InventoryFilters::default(),
        ),
    );
    assert_eq!((report.read, report.marked, report.expected), (6, 4, 6));
    assert_eq!(
        lines(&report),
        vec![
            (Some(id(1)), vec![OutOfOrder]),
            (Some(id(0)), vec![OutOfOrder]),
            (Some(id(2)), vec![WasLost]),
            (Some(id(3)), vec![WrongPlace]),
            (Some(id(4)), vec![Withdrawn]),
            (None, vec![UnknownBarcode]),
            (Some(id(5)), vec![NotScanned]),
            (Some(id(6)), vec![NoBarcode]),
        ]
    );
    assert_eq!(report.lines[0].call_number.as_deref(), Some("869.2 A1"));
    let state: String = store
        .readers()
        .read(|c| {
            c.query_row(
                "SELECT group_concat(coalesce(last_seen, '-') || ':' || lost, ',')
                 FROM (SELECT last_seen, lost FROM items WHERE id IN (?1, ?2, ?3) ORDER BY id)",
                [id(0).0, id(2).0, id(4).0],
                |row| row.get(0),
            )
        })
        .unwrap();
    assert_eq!(state, "2026-11-26:0,2026-11-26:0,-:0");
    // Only what was not seen that day is expected now: nothing read, the list compared.
    let report = inventory(
        &store,
        scan(
            Vec::new(),
            InventoryFilters {
                not_seen_since: Some(date(2026, 11, 26)),
                ..InventoryFilters::default()
            },
        ),
    );
    assert_eq!(
        lines(&report),
        vec![
            (Some(id(4)), vec![NotScanned]),
            (Some(id(5)), vec![NotScanned]),
            (Some(id(6)), vec![NoBarcode]),
        ]
    );
    let mut nothing = scan(Vec::new(), InventoryFilters::default());
    nothing.compare = false;
    assert!(matches!(
        run(&store, CatalogCommand::Inventory(Box::new(nothing))),
        Err(CommandFailed::Rejected(WriteRefusal::Catalog(
            CatalogRefusal::NoBarcodes
        )))
    ));
}
