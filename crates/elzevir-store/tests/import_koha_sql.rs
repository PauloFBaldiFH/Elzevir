// SPDX-License-Identifier: GPL-3.0-or-later
//! Koha SQL joins catalog data before sending anything to the existing writer.
#![expect(
    clippy::unwrap_used,
    reason = "synthetic dump and database setup must succeed"
)]

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::num::NonZeroUsize;

use elzevir_app::catalog::CatalogCommand;
use elzevir_app::codes::{CodeTable, NamedCode};
use elzevir_app::commands::WriteCommand;
use elzevir_app::export::{ExportOptions, export_marc21};
use elzevir_app::import::{ImportOptions, Parallelism, import_koha_sql};
use elzevir_app::marc_mapping::marc_to_bibframe;
use elzevir_marc::{Field, Limits, MarcReader, Record};
use elzevir_store::{ElzevirHandler, ElzevirStore, StoreConfig};

const SOURCE: &[u8] = include_bytes!("../../elzevir-app/tests/data/koha-staff-bibs-1-200.mrc");
const I1_DIGEST: &str = "bf89658626085174";
const PRIVATE: &str = "SYNTHETIC_PRIVATE_READER";

fn quoted(text: &[u8]) -> String {
    let text = std::str::from_utf8(text).unwrap();
    format!("'{}'", text.replace('\\', "\\\\").replace('\'', "''"))
}

fn dump() -> String {
    let mut metadata = String::from(
        "CREATE TABLE biblio_metadata (metadata longtext, biblionumber int, format varchar(16));\n",
    );
    let mut items = String::from(
        "CREATE TABLE items (barcode varchar(20), itemnumber int, biblionumber int, PRIMARY KEY (itemnumber));\n",
    );
    let mut reader = MarcReader::new(SOURCE, Limits::default());
    while let Some(input) = reader.next_record() {
        let record = Record::from_ref(&input.unwrap());
        let mut xml = String::new();
        record.write_marcxml_document(&mut xml).unwrap();
        let biblio = record
            .fields
            .iter()
            .filter(|field| field.tag().as_bytes() == b"999")
            .find_map(|field| value(field, b'c'))
            .unwrap();
        let number = std::str::from_utf8(biblio).unwrap();
        writeln!(
            metadata,
            "INSERT INTO biblio_metadata VALUES ({},{number},'marcxml');",
            quoted(xml.as_bytes())
        )
        .unwrap();
        for field in record
            .fields
            .iter()
            .filter(|field| field.tag().as_bytes() == b"952")
        {
            let id = std::str::from_utf8(value(field, b'9').unwrap()).unwrap();
            let barcode = quoted(value(field, b'p').unwrap());
            writeln!(
                items,
                "INSERT INTO items (itemnumber, barcode, biblionumber) VALUES ({id},{barcode},{number});"
            )
            .unwrap();
        }
    }
    format!(
        "{items}\nCREATE TABLE borrowers (name text);\nINSERT INTO borrowers VALUES ('{PRIVATE}; CREATE TABLE systempreferences (v text);');\n{metadata}"
    )
}

fn value(field: &Field, code: u8) -> Option<&[u8]> {
    match field {
        Field::Data { subfields, .. } => subfields
            .iter()
            .find(|subfield| subfield.code == code)
            .map(|subfield| subfield.value.as_slice()),
        Field::Control { .. } => None,
    }
}

fn store(directory: &tempfile::TempDir, setup_i1: bool) -> ElzevirStore {
    let store = ElzevirStore::open(
        &directory.path().join("elzevir.db"),
        &StoreConfig::default(),
        ElzevirHandler,
    )
    .unwrap();
    if setup_i1 {
        let mut branches = BTreeSet::new();
        let mut types = BTreeSet::new();
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
            store
                .submit(WriteCommand::Catalog(CatalogCommand::NameCodes(vec![
                    NamedCode {
                        table,
                        name: code.clone(),
                        code,
                    },
                ])))
                .unwrap()
                .wait()
                .unwrap();
        }
    }
    store
}

#[test]
fn sql_of_the_reference_catalog_keeps_i1_and_the_exact_marc_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(&dir, true);
    let sql = dump();
    let report = import_koha_sql(
        sql.as_bytes(),
        &store,
        &ImportOptions {
            parallelism: Parallelism::Threads(NonZeroUsize::new(4).unwrap()),
            chunk: NonZeroUsize::new(32).unwrap(),
            ..ImportOptions::default()
        },
    )
    .unwrap();
    assert_eq!(
        (report.read, report.imported, report.items),
        (200, 200, 253)
    );
    assert!(report.failures.is_empty() && report.item_failures.is_empty());
    assert_eq!(store.digest().unwrap(), I1_DIGEST);
    let mut bytes = Vec::new();
    let export = export_marc21(&store, &mut bytes, &ExportOptions::default()).unwrap();
    assert_eq!(export.written, 200);
    assert!(export.failures.is_empty());
    assert_eq!(bytes, SOURCE);
}

#[test]
fn a_separate_inventory_row_reaches_the_writer_as_a_new_marc_item() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(&dir, false);
    for (table, code) in [
        (CodeTable::Branch, "SYNTH-OWNER"),
        (CodeTable::ItemType, "SYNTH-LOAN"),
    ] {
        store
            .submit(WriteCommand::Catalog(CatalogCommand::NameCodes(vec![
                NamedCode {
                    table,
                    name: code.to_owned(),
                    code: code.to_owned(),
                },
            ])))
            .unwrap()
            .wait()
            .unwrap();
    }
    let sql = br#"
INSERT INTO items (stocknumber,biblionumber,itemnumber,homebranch,itype,barcode)
VALUES ('INV-1',54,92,'SYNTH-OWNER','SYNTH-LOAN','FICTION-92');
INSERT INTO biblio_metadata (metadata,biblionumber,format,schema) VALUES ('
<record xmlns="http://www.loc.gov/MARC21/slim"><leader>00000nam a2200000 i 4500</leader>
<datafield tag="245" ind1="1" ind2="0"><subfield code="a">Synthetic separate item</subfield></datafield>
</record>',54,'marcxml','MARC21');
"#;
    let report = import_koha_sql(sql.as_slice(), &store, &ImportOptions::default()).unwrap();
    assert_eq!((report.imported, report.items), (1, 1));
    assert!(report.failures.is_empty() && report.item_failures.is_empty());
    let mut bytes = Vec::new();
    let export = export_marc21(&store, &mut bytes, &ExportOptions::default()).unwrap();
    assert_eq!(export.written, 1);
    let mut reader = MarcReader::new(bytes.as_slice(), Limits::default());
    let record = reader.next_record().unwrap().unwrap();
    let item = record
        .fields()
        .find(|field| field.tag().as_bytes() == b"952")
        .unwrap();
    let expected = [
        (b'9', b"92".as_slice()),
        (b'p', b"FICTION-92".as_slice()),
        (b'i', b"INV-1".as_slice()),
    ];
    for (code, value) in expected {
        assert_eq!(
            item.subfields()
                .find_map(|(found, bytes)| (found == code).then_some(bytes)),
            Some(value)
        );
    }
}

#[test]
fn a_full_inventory_after_the_detection_prefix_never_reaches_the_writer() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(&dir, false);
    let before = store.digest().unwrap();
    let mut sql = dump();
    sql.push_str("CREATE TABLE issues (x int);\nCREATE TABLE systempreferences (v text);");
    let error = import_koha_sql(sql.as_bytes(), &store, &ImportOptions::default())
        .unwrap_err()
        .to_string();
    assert!(error.contains("complete Koha backup"), "{error}");
    assert!(!error.contains(PRIVATE));
    assert_eq!(store.digest().unwrap(), before);
}
