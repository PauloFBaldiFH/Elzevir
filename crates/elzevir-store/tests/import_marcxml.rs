// SPDX-License-Identifier: GPL-3.0-or-later
//! MARCXML reaches the existing writer and the fixed I1 database without changing source data.
#![expect(
    clippy::unwrap_used,
    reason = "integration test setup and writer failures must fail the test"
)]

use std::collections::BTreeSet;
use std::num::NonZeroUsize;

use elzevir_app::catalog::CatalogCommand;
use elzevir_app::codes::{CodeTable, NamedCode};
use elzevir_app::commands::WriteCommand;
use elzevir_app::export::{ExportOptions, export_marc21};
use elzevir_app::import::{
    ImportOptions, Parallelism, RecordFailure, import_marc21, import_marcxml,
};
use elzevir_app::marc_mapping::marc_to_bibframe;
use elzevir_app::matching::Matching;
use elzevir_domain::catalog::MatchRule;
use elzevir_marc::{
    Limits, MARCXML_COLLECTION_FOOTER, MARCXML_COLLECTION_HEADER, MarcReader, Record,
};
use elzevir_store::{ElzevirHandler, ElzevirStore, StoreConfig};

const SOURCE: &[u8] = include_bytes!("../../elzevir-app/tests/data/koha-staff-bibs-1-200.mrc");
// The unchanged I1 reference in import_digest.rs; never derive the expected value at runtime.
const I1_DIGEST: &str = "5c1410f0b9f53486";

fn xml() -> String {
    let mut xml = MARCXML_COLLECTION_HEADER.to_owned();
    let mut reader = MarcReader::new(SOURCE, Limits::default());
    while let Some(record) = reader.next_record() {
        Record::from_ref(&record.unwrap())
            .write_marcxml_collection_record(&mut xml)
            .unwrap();
    }
    xml.push_str(MARCXML_COLLECTION_FOOTER);
    xml
}

fn open(directory: &tempfile::TempDir) -> ElzevirStore {
    let store = ElzevirStore::open(
        &directory.path().join("elzevir.db"),
        &StoreConfig::default(),
        ElzevirHandler,
    )
    .unwrap();
    // Same command order as the_staff_export_imports_whole and the fixed I1 test.
    let (mut branches, mut types) = (BTreeSet::new(), BTreeSet::new());
    let mut reader = MarcReader::new(SOURCE, Limits::default());
    while let Some(record) = reader.next_record() {
        let mut record = Record::from_ref(&record.unwrap());
        record.marc21_to_utf8();
        for item in marc_to_bibframe(&record, None).unwrap().items.into_marc() {
            branches.insert(item.home_branch);
            branches.insert(item.holding_branch);
            types.insert(item.item_type);
        }
    }
    for (table, code) in branches
        .into_iter()
        .map(|code| (CodeTable::Branch, code))
        .chain(types.into_iter().map(|code| (CodeTable::ItemType, code)))
    {
        let command = CatalogCommand::NameCodes(vec![NamedCode {
            table,
            name: code.clone(),
            code,
        }]);
        store
            .submit(WriteCommand::Catalog(command))
            .unwrap()
            .wait()
            .unwrap();
    }
    store
}

#[test]
fn the_200_records_from_xml_match_i1_and_export_the_original_iso_bytes() {
    let xml = xml();
    for (parallelism, chunk) in [
        (Parallelism::Threads(NonZeroUsize::new(4).unwrap()), 32),
        (Parallelism::Serial, 7),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let store = open(&directory);
        let options = ImportOptions {
            parallelism,
            chunk: NonZeroUsize::new(chunk).unwrap(),
            ..ImportOptions::default()
        };
        let report = import_marcxml(xml.as_bytes(), &store, &options).unwrap();
        assert_eq!(
            (report.read, report.imported, report.items),
            (200, 200, 253)
        );
        assert!(report.failures.is_empty(), "{:?}", report.failures);
        assert!(report.item_failures.is_empty());
        assert_eq!(store.digest().unwrap(), I1_DIGEST);
        let mut exported = Vec::new();
        let report = export_marc21(&store, &mut exported, &ExportOptions::default()).unwrap();
        assert_eq!(report.written, 200);
        assert!(report.failures.is_empty());
        assert_eq!(exported, SOURCE);
    }
}

#[test]
fn xml_reimport_uses_the_existing_matching_keys_and_item_actions() {
    let (iso_dir, xml_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (iso_store, xml_store) = (open(&iso_dir), open(&xml_dir));
    let xml = xml();
    let mut options = ImportOptions::default();
    assert_eq!(
        import_marc21(SOURCE, &iso_store, &options).unwrap(),
        import_marcxml(xml.as_bytes(), &xml_store, &options).unwrap()
    );
    options.matching = Some(Matching::new(MatchRule::KohaBiblionumber));
    let expected = import_marc21(SOURCE, &iso_store, &options).unwrap();
    let actual = import_marcxml(xml.as_bytes(), &xml_store, &options).unwrap();
    assert_eq!(actual, expected);
    assert_eq!(actual.matched.len(), 200);
    assert_eq!(actual.item_failures.len(), 253);
    for store in [&xml_store, &iso_store] {
        let mut bytes = Vec::new();
        export_marc21(store, &mut bytes, &ExportOptions::default()).unwrap();
        assert_eq!(bytes, SOURCE);
    }
    // The captured XML and ISO source evidence has a different serialization.
    assert_eq!(xml_store.digest().unwrap(), "37d8dfb99f38f08b");
    assert_eq!(iso_store.digest().unwrap(), "af6b0fa6d27ab943");
}

#[test]
fn a_mapping_refusal_is_reported_and_the_next_xml_record_still_enters() {
    let directory = tempfile::tempdir().unwrap();
    let store = open(&directory);
    let xml = "<collection xmlns='http://www.loc.gov/MARC21/slim'><record><leader>00000nam a2200000 a 4500</leader></record><record><leader>00000nam a2200000 a 4500</leader><datafield tag='245' ind1=' ' ind2=' '><subfield code='a'>Synthetic title</subfield></datafield></record></collection>";
    let report = import_marcxml(xml.as_bytes(), &store, &ImportOptions::default()).unwrap();
    assert_eq!(
        (report.read, report.imported, report.failures.len()),
        (2, 1, 1)
    );
    let first = report.failures.first().unwrap();
    assert_eq!(first.index, 0);
    assert_eq!(
        first.offset,
        u64::try_from(xml.find("<record>").unwrap()).unwrap()
    );
}

#[test]
fn an_iso_conversion_refusal_is_reported_before_the_next_xml_record() {
    let directory = tempfile::tempdir().unwrap();
    let store = open(&directory);
    let xml = format!(
        "<collection xmlns='http://www.loc.gov/MARC21/slim'><record><leader>00000nam a2200000 a 4500</leader><controlfield tag='001'>{}</controlfield></record><record><leader>00000nam a2200000 a 4500</leader><datafield tag='245' ind1=' ' ind2=' '><subfield code='a'>Synthetic title</subfield></datafield></record></collection>",
        "x".repeat(10_000)
    );
    let report = import_marcxml(xml.as_bytes(), &store, &ImportOptions::default()).unwrap();
    assert_eq!(
        (report.read, report.imported, report.failures.len()),
        (2, 1, 1)
    );
    let first = report.failures.first().unwrap();
    assert_eq!(first.index, 0);
    assert!(matches!(first.failure, RecordFailure::Source(_)));
    let mut exported = Vec::new();
    assert_eq!(
        export_marc21(&store, &mut exported, &ExportOptions::default())
            .unwrap()
            .written,
        1
    );
}
