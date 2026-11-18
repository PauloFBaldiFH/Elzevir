// SPDX-License-Identifier: GPL-3.0-or-later
//! Preview and confirmation use the same preparation and catalog rules.
#![expect(
    clippy::unwrap_used,
    reason = "test database and fixture setup must succeed"
)]

use elzevir_app::catalog::CatalogCommand;
use elzevir_app::codes::{CodeTable, NamedCode};
use elzevir_app::commands::WriteCommand;
use elzevir_app::import::{ImportOptions, import_marc21};
use elzevir_app::matching::{ItemAction, Matching, OnNoMatch};
use elzevir_domain::catalog::MatchRule;
use elzevir_marc::{Field, Record, Subfield, Tag};
use elzevir_store::{ElzevirHandler, ElzevirStore, StoreConfig};

fn data(tag: [u8; 3], pairs: &[(u8, &str)]) -> Field {
    Field::Data {
        tag: Tag::new(tag),
        indicators: *b"  ",
        subfields: pairs
            .iter()
            .map(|&(code, value)| Subfield {
                code,
                value: value.as_bytes().to_vec(),
            })
            .collect(),
    }
}

fn source(barcode: Option<&str>) -> Vec<u8> {
    let mut item = vec![(b'a', "Synthetic branch"), (b'y', "Synthetic policy")];
    if let Some(barcode) = barcode {
        item.push((b'p', barcode));
    }
    let record = Record {
        leader: *b"00000nam a2200000 a 4500",
        fields: vec![
            data(*b"245", &[(b'a', "Synthetic title")]),
            data(*b"999", &[(b'c', "42")]),
            data(*b"952", &item),
        ],
    };
    let mut bytes = Vec::new();
    record.write_iso2709(&mut bytes).unwrap();
    bytes
}

fn open(dir: &tempfile::TempDir) -> ElzevirStore {
    ElzevirStore::open(
        &dir.path().join("preview.db"),
        &StoreConfig::default(),
        ElzevirHandler,
    )
    .unwrap()
}

fn seed(store: &ElzevirStore) {
    let codes = [
        (CodeTable::Branch, "Synthetic branch"),
        (CodeTable::ItemType, "Synthetic policy"),
    ]
    .into_iter()
    .map(|(table, name)| NamedCode {
        table,
        name: name.into(),
        code: name.into(),
    })
    .collect();
    store
        .submit(WriteCommand::Catalog(CatalogCommand::NameCodes(codes)))
        .unwrap()
        .wait()
        .unwrap();
}

#[test]
fn preview_refuses_the_same_unknown_registry_code_as_the_writer_and_keeps_the_digest() {
    let directory = tempfile::tempdir().unwrap();
    let store = open(&directory);
    let source = source(Some("SYNTHETIC-1"));
    let options = ImportOptions::default();
    let before = store.digest().unwrap();
    let forecast = store
        .readers()
        .preview_import(source.as_slice(), options.clone())
        .unwrap();
    assert_eq!(forecast.issues.len(), 1);
    let issue = forecast.issues.first().unwrap();
    assert_eq!((issue.index, issue.offset, issue.item), (0, 0, Some(0)));
    assert_eq!(issue.reason, "unknown branch");
    assert_eq!(store.digest().unwrap(), before);
    let confirmed = import_marc21(source.as_slice(), &store, &options).unwrap();
    assert_eq!(confirmed.imported, 0);
    assert!(
        confirmed
            .failures
            .first()
            .unwrap()
            .failure
            .to_string()
            .contains(&issue.reason)
    );
}

#[test]
fn preview_reports_existing_and_batch_conflicts_and_respects_item_replacement() {
    let directory = tempfile::tempdir().unwrap();
    let store = open(&directory);
    seed(&store);
    let source = source(Some("SYNTHETIC-2"));
    let options = ImportOptions::default();
    assert_eq!(
        import_marc21(source.as_slice(), &store, &options)
            .unwrap()
            .items,
        1
    );

    let before = store.digest().unwrap();
    let matching = Matching::new(MatchRule::KohaBiblionumber);
    let options = ImportOptions {
        matching: Some(matching),
        ..options
    };
    let repeated = source.repeat(2);
    let forecast = store
        .readers()
        .preview_import(repeated.as_slice(), options.clone())
        .unwrap();
    assert_eq!(forecast.matches.len(), 2);
    for reason in [
        "barcode already in use",
        "barcode already present in the batch",
        "possible match within the batch",
    ] {
        assert!(
            forecast
                .issues
                .iter()
                .any(|issue| issue.reason.starts_with(reason))
        );
    }
    let options = ImportOptions {
        matching: Some(Matching {
            items: ItemAction::Replace,
            ..matching
        }),
        ..options
    };
    assert!(
        store
            .readers()
            .preview_import(source.as_slice(), options.clone())
            .unwrap()
            .issues
            .is_empty()
    );
    assert_eq!(store.digest().unwrap(), before);
    assert_eq!(
        import_marc21(source.as_slice(), &store, &options)
            .unwrap()
            .replaced_items,
        1
    );
}

#[test]
fn absence_of_barcode_requires_review_unless_the_item_would_be_skipped() {
    let directory = tempfile::tempdir().unwrap();
    let store = open(&directory);
    seed(&store);
    let source = source(None);
    let options = ImportOptions::default();
    let before = store.digest().unwrap();
    let forecast = store
        .readers()
        .preview_import(source.as_slice(), options.clone())
        .unwrap();
    assert_eq!(forecast.issues.first().unwrap().reason, "missing barcode");
    let options = ImportOptions {
        matching: Some(Matching {
            on_no_match: OnNoMatch::Ignore,
            ..Matching::new(MatchRule::KohaBiblionumber)
        }),
        ..options
    };
    assert!(
        store
            .readers()
            .preview_import(source.as_slice(), options)
            .unwrap()
            .issues
            .is_empty()
    );
    assert_eq!(store.digest().unwrap(), before);
}
