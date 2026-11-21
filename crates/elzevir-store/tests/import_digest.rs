// SPDX-License-Identifier: GPL-3.0-or-later
//! Fixed import baseline for the universal import roadmap
//! (docs/specs/catalogo/importacao-universal.md, I1).
#![expect(
    clippy::unwrap_used,
    reason = "integration test: setup and import failures must fail the test"
)]

use std::collections::BTreeSet;
use std::num::NonZeroUsize;

use elzevir_app::catalog::CatalogCommand;
use elzevir_app::codes::{CodeTable, NamedCode};
use elzevir_app::commands::WriteCommand;
use elzevir_app::import::{ImportOptions, Parallelism, import_marc21};
use elzevir_app::koha_types::TypeTranslations;
use elzevir_app::marc_mapping::marc_to_bibframe;
use elzevir_marc::{Limits, Record, RecordRef};
use elzevir_store::{ElzevirHandler, ElzevirStore, StoreConfig};

const STAFF_EXPORT: &[u8] =
    include_bytes!("../../elzevir-app/tests/data/koha-staff-bibs-1-200.mrc");

// Schema 61 includes the original representation, decoder evidence and scoped headings.
// The fixed catalog fixture must still export the same 200 ISO records byte for byte.
const STAFF_EXPORT_DIGEST: &str = "0280171440ec1014";

/// Keep the commands and their order identical to `import.rs`'s
/// `the_staff_export_imports_whole`, including one `NameCodes` command per code.
fn setup(store: &ElzevirStore) {
    let mut branches = BTreeSet::new();
    let mut item_types = BTreeSet::new();
    for raw in STAFF_EXPORT
        .split_inclusive(|&byte| byte == 0x1d)
        .filter(|raw| raw.len() > 1)
    {
        let mut record = Record::from_ref(&RecordRef::parse(raw, &Limits::default()).unwrap());
        record.marc21_to_utf8();
        for item in marc_to_bibframe(&record, None).unwrap().items.into_marc() {
            branches.insert(item.home_branch);
            branches.insert(item.holding_branch);
            item_types.insert(item.item_type);
        }
    }
    let commands = branches
        .into_iter()
        .map(|code| {
            CatalogCommand::NameCodes(vec![NamedCode {
                table: CodeTable::Branch,
                name: code.clone(),
                code,
            }])
        })
        .chain(item_types.into_iter().map(|code| {
            CatalogCommand::NameCodes(vec![NamedCode {
                table: CodeTable::ItemType,
                name: code.clone(),
                code,
            }])
        }));
    for command in commands {
        store
            .submit(WriteCommand::Catalog(command))
            .unwrap()
            .wait()
            .unwrap();
    }
}

#[test]
fn the_staff_export_keeps_the_fixed_import_digest() {
    let directory = tempfile::tempdir().unwrap();
    let store = ElzevirStore::open(
        &directory.path().join("elzevir.db"),
        &StoreConfig::default(),
        ElzevirHandler,
    )
    .unwrap();
    setup(&store);
    let options = ImportOptions {
        limits: Limits::default(),
        parallelism: Parallelism::Threads(NonZeroUsize::new(4).unwrap()),
        chunk: NonZeroUsize::new(32).unwrap(),
        biblivre: None,
        local_note_visibility: None,
        origin: None,
        reviewed: Vec::new(),
        originals: std::collections::BTreeMap::new(),
        matching: None,
        translations: TypeTranslations::default(),
    };
    let report = import_marc21(STAFF_EXPORT, &store, &options).unwrap();
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert!(
        report.item_failures.is_empty(),
        "{:?}",
        report.item_failures
    );
    assert_eq!(
        (report.read, report.imported, report.items),
        (200, 200, 253)
    );
    assert_eq!(store.digest().unwrap(), STAFF_EXPORT_DIGEST);
}
