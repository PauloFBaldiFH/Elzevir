// SPDX-License-Identifier: GPL-3.0-or-later
//! Acceptance of native sheets and transactional validation by the existing writer.
#![expect(clippy::unwrap_used, reason = "test fixtures use a disposable catalog")]

use std::collections::BTreeSet;

use elzevir_app::catalog::{
    CatalogCommand, CatalogReader, CatalogRefusal, CodeLists, NativeItemProblem, RecordItem,
    RecordItems,
};
use elzevir_app::commands::WriteCommand;
use elzevir_app::export::{ExportOptions, export_marc21};
use elzevir_app::import::sheet::{GroupBy, PreparedTable, SheetError, TableSettings, prepare};
use elzevir_app::import::table::{Delimiter, Encoding, model_columns};
use elzevir_app::import::{PendingRecord, RecordFailure, RecordRejected, RecordSink};
use elzevir_app::items::{ItemColumn, RowProblem};
use elzevir_app::matching::{ItemAction, Matching};
use elzevir_app::registries::{RegistryChange, RegistryEntry};
use elzevir_domain::catalog::MatchRule;
use elzevir_domain::statement::{Resource, Value};
use elzevir_marc::{Limits, MarcReader};
use elzevir_store::{ElzevirHandler, ElzevirStore, StoreConfig};

fn catalog() -> (tempfile::TempDir, ElzevirStore) {
    let data = tempfile::tempdir().unwrap();
    let store = ElzevirStore::open(
        &data.path().join("elzevir.db"),
        &StoreConfig::default(),
        ElzevirHandler,
    )
    .unwrap();
    for command in [
        CatalogCommand::CreateBranch {
            name: "Library East".to_owned(),
        },
        CatalogCommand::CreateBranch {
            name: "Library West".to_owned(),
        },
        CatalogCommand::CreatePolicy {
            name: "Standard loan".to_owned(),
        },
    ] {
        store
            .submit(WriteCommand::Catalog(command))
            .unwrap()
            .wait()
            .unwrap();
    }
    (data, store)
}

fn isbn(number: u8) -> String {
    let prefix = format!("9781234567{number:02}");
    let sum: u32 = prefix
        .bytes()
        .zip([1, 3].into_iter().cycle())
        .map(|(digit, weight)| u32::from(digit - b'0') * weight)
        .sum();
    format!("{prefix}{}", (10 - sum % 10) % 10)
}

fn fixture(rows: impl IntoIterator<Item = (u8, String)>) -> String {
    let header = model_columns();
    let mut output = header.join(",");
    for (number, barcode) in rows {
        let identifier = serde_json::to_string(&[Value::Resource(Resource {
            class: "Isbn".to_owned(),
            label: Some(isbn(number)),
            statements: Vec::new(),
        })])
        .unwrap();
        let fields = header
            .iter()
            .map(|column| match column.as_str() {
                "Work.@type" => "Text".to_owned(),
                "Work.title" => format!("Synthetic book {number}"),
                "Instance.@type" => "Print".to_owned(),
                "Instance.identifiedBy" => format!("\"{}\"", identifier.replace('"', "\"\"")),
                "Item.catalog.barcode" => barcode.clone(),
                "Item.catalog.home_branch" => "Library East".to_owned(),
                "Item.catalog.holding_branch" => "Library West".to_owned(),
                "Item.catalog.policy" => "Standard loan".to_owned(),
                _ => String::new(),
            })
            .collect::<Vec<_>>();
        output.push('\n');
        output.push_str(&fields.join(","));
    }
    output
}

fn prepared(
    source: &str,
    codes: &CodeLists,
    group_by: Option<GroupBy>,
) -> Result<PreparedTable, SheetError> {
    prepare(
        source.as_bytes(),
        &TableSettings {
            delimiter: Delimiter::Comma,
            encoding: Encoding::Utf8,
            profile: None,
            group_by,
        },
        codes,
        &BTreeSet::new(),
        &BTreeSet::new(),
    )
}

#[test]
fn twenty_native_books_resolve_names_export_marc_and_match_on_reimport() {
    let (_data, store) = catalog();
    let source = fixture((0..20).map(|index| (index, format!("native-{index:02}"))));
    let codes = store.codes().unwrap();
    let table = prepared(&source, &codes, None).unwrap();
    let first = table.records().first().unwrap();
    let item = first
        .record
        .items
        .entries()
        .find_map(|item| match item {
            RecordItem::Native(item) => Some(item),
            RecordItem::Marc(_) => None,
        })
        .unwrap();
    assert_eq!(codes.branch(item.fields.home_branch), "Library East");
    assert_eq!(codes.branch(item.fields.holding_branch), "Library West");
    assert_eq!(codes.policy(item.fields.policy), "Standard loan");
    assert!(first.record.source.is_none());

    let mut table = table;
    table.apply_review(&elzevir_app::import::ImportOptions {
        origin: Some("sheet".into()),
        ..Default::default()
    });
    let report = table.submit(&store, None).unwrap();
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert_eq!((report.imported, report.items), (20, 20));
    let matching = Matching {
        items: ItemAction::Replace,
        ..Matching::new(MatchRule::Isbn)
    };
    let recipe = elzevir_app::import::edits::Recipe {
        matching: Some(matching),
        origin: Some("sheet".into()),
        ..elzevir_app::import::edits::Recipe::default()
    };
    let (mut second, report) = store
        .readers()
        .preview_table_source(&recipe, 0, |codes| prepared(&source, codes, None))
        .unwrap();
    second.apply_review(&elzevir_app::import::ImportOptions {
        reviewed: report.scopes.into_iter().map(|(_, scope)| scope).collect(),
        origin: Some("sheet".into()),
        ..Default::default()
    });
    let again = second.submit(&store, Some(matching)).unwrap();
    assert_eq!(
        (
            again.imported,
            again.items,
            again.matched.len(),
            again.replaced_items
        ),
        (0, 0, 20, 20)
    );
    assert!(again.failures.is_empty() && again.item_failures.is_empty());

    let mut out = Vec::new();
    let exported = export_marc21(&store, &mut out, &ExportOptions::default()).unwrap();
    assert!(exported.failures.is_empty());
    let mut reader = MarcReader::new(out.as_slice(), Limits::default());
    let (mut barcodes, mut isbns) = (BTreeSet::new(), BTreeSet::new());
    while let Some(record) = reader.next_record() {
        let record = record.unwrap();
        for field in record.fields() {
            match field.tag().as_bytes() {
                b"952" => barcodes.extend(
                    field
                        .subfields()
                        .filter(|(code, _)| *code == b'p')
                        .map(|(_, value)| value.to_vec()),
                ),
                b"020" => isbns.extend(
                    field
                        .subfields()
                        .filter(|(code, _)| *code == b'a')
                        .map(|(_, value)| value.to_vec()),
                ),
                _ => {}
            }
        }
    }
    assert_eq!(
        barcodes,
        (0..20)
            .map(|index| format!("native-{index:02}").into_bytes())
            .collect()
    );
    assert_eq!(
        isbns,
        (0..20).map(|index| isbn(index).into_bytes()).collect()
    );
}

#[test]
fn transactional_revalidation_refuses_the_exact_item_and_rolls_back_add_record() {
    let (_data, store) = catalog();
    let codes = store.codes().unwrap();
    let pending = fixture([
        (1, "native-first".to_owned()),
        (1, "native-taken".to_owned()),
    ]);
    let prepared_before_change = prepared(&pending, &codes, Some(GroupBy::Isbn)).unwrap();
    let prepared_with_matching = prepared(&pending, &codes, Some(GroupBy::Isbn)).unwrap();

    let racer = fixture([(2, "native-taken".to_owned())]);
    let inserted = prepared(&racer, &codes, None)
        .unwrap()
        .submit(&store, None)
        .unwrap();
    assert_eq!((inserted.imported, inserted.items), (1, 1));

    let before = store.digest().unwrap();
    let result = prepared_before_change.submit(&store, None).unwrap();
    assert_eq!(result.imported, 0);
    assert_eq!(result.items, 0);
    assert_eq!(result.failures.len(), 1);
    let refused = CatalogRefusal::NativeItem {
        position: 2,
        barcode: Some("native-taken".to_owned()),
        problem: NativeItemProblem::Field {
            column: ItemColumn::Barcode,
            problem: RowProblem::BarcodeTaken,
        },
    };
    assert_eq!(
        result.failures.first().unwrap().failure,
        RecordFailure::Rejected(RecordRejected::Refused(refused.clone()))
    );
    assert!(
        refused
            .to_string()
            .contains("item 2 (barcode \"native-taken\")")
    );
    assert_eq!(store.digest().unwrap(), before);

    let matched_import = prepared_with_matching
        .submit(&store, Some(Matching::new(MatchRule::Isbn)))
        .unwrap();
    assert_eq!((matched_import.imported, matched_import.items), (1, 1));
    assert!(matched_import.failures.is_empty());
    assert_eq!(matched_import.item_failures.len(), 1);
    let item_failure = matched_import.item_failures.first().unwrap();
    assert_eq!((item_failure.index, item_failure.position), (1, 1));
    assert_eq!(item_failure.refusal, refused);
}

#[test]
fn duplicate_barcodes_block_the_entire_sheet_before_the_first_write() {
    let (_data, store) = catalog();
    let codes = store.codes().unwrap();
    let source = fixture([
        (1, "native-duplicate".to_owned()),
        (2, "native-duplicate".to_owned()),
    ]);
    let before = store.digest().unwrap();
    assert!(matches!(prepared(&source, &codes, None),
        Err(SheetError::DuplicateBarcode { row: 3, first: 2, barcode }) if barcode == "native-duplicate"));
    assert_eq!(store.digest().unwrap(), before);
}

#[test]
fn a_registry_removed_after_preparation_refuses_the_exact_item() {
    let (_data, store) = catalog();
    let codes = store.codes().unwrap();
    let source = fixture([(1, "native-with-policy".to_owned())]);
    let table = prepared(&source, &codes, None).unwrap();
    let policy = codes.policies.first().unwrap().id;
    store
        .submit(WriteCommand::Catalog(CatalogCommand::ChangeRegistry(
            RegistryChange::Delete(RegistryEntry::Policy(policy)),
        )))
        .unwrap()
        .wait()
        .unwrap();

    let before = store.digest().unwrap();
    let report = table.submit(&store, None).unwrap();
    assert_eq!((report.imported, report.items), (0, 0));
    assert_eq!(report.failures.len(), 1);
    assert_eq!(
        report.failures.first().unwrap().failure,
        RecordFailure::Rejected(RecordRejected::Refused(CatalogRefusal::NativeItem {
            position: 1,
            barcode: Some("native-with-policy".to_owned()),
            problem: NativeItemProblem::Field {
                column: ItemColumn::Policy,
                problem: RowProblem::UnknownPolicy,
            },
        }))
    );
    assert_eq!(store.digest().unwrap(), before);
}

#[test]
fn a_sheet_with_only_separators_needs_no_profile_or_write() {
    let table = prepared(",,\r\n,,\r\n", &CodeLists::default(), None).unwrap();
    assert_eq!((table.read, table.excluded), (0, 0));
    assert!(table.records().is_empty());
}

#[test]
fn writer_rejects_duplicate_barcodes_in_both_commands_before_applying_matching() {
    let (_data, store) = catalog();
    let codes = store.codes().unwrap();
    let source = fixture([(2, "native-duplicate".to_owned())]);
    let table = prepared(&source, &codes, None).unwrap();
    let mut record = table.records().first().unwrap().record.clone();
    let item = record
        .items
        .entries()
        .find_map(|item| match item {
            RecordItem::Native(item) => Some(item.clone()),
            RecordItem::Marc(_) => None,
        })
        .unwrap();
    record.items = RecordItems::Native {
        native: vec![item.clone(), item],
    };
    let matching = Matching {
        items: ItemAction::Replace,
        ..Matching::new(MatchRule::Isbn)
    };
    let before = store.digest().unwrap();
    for selection in [None, Some(matching)] {
        let result = RecordSink::submit(&store, record.clone(), selection)
            .unwrap()
            .wait();
        assert_eq!(
            result.unwrap_err(),
            RecordRejected::Refused(CatalogRefusal::NativeItem {
                position: 2,
                barcode: Some("native-duplicate".to_owned()),
                problem: NativeItemProblem::Duplicate { first: 1 },
            })
        );
        assert_eq!(store.digest().unwrap(), before);
    }
}
