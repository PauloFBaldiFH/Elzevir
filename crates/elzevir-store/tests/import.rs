// SPDX-License-Identifier: GPL-3.0-or-later
//! Batch import through the real writer (docs/specs/catalogo/importacao.md).
#![expect(
    clippy::unwrap_used,
    reason = "integration test: helper functions fail the test on error"
)]

use elzevir_app::codes::{CodeTable, NamedCode};
use std::collections::BTreeSet;
use std::num::NonZeroUsize;

use elzevir_app::catalog::{CatalogCommand, CatalogRefusal};
use elzevir_app::commands::WriteCommand;
use elzevir_app::import::{
    ImportOptions, ImportReport, Parallelism, RecordFailure, RecordRejected, import_marc21,
};
use elzevir_app::koha_types::TypeTranslations;
use elzevir_app::marc_mapping::marc_to_bibframe;
use elzevir_marc::{Limits, Record, RecordRef};
use elzevir_store::{ElzevirHandler, ElzevirStore, StoreConfig};

const STAFF_EXPORT: &[u8] =
    include_bytes!("../../elzevir-app/tests/data/koha-staff-bibs-1-200.mrc");

fn open(dir: &tempfile::TempDir) -> ElzevirStore {
    ElzevirStore::open(
        &dir.path().join("elzevir.db"),
        &StoreConfig::default(),
        ElzevirHandler,
    )
    .unwrap()
}

fn records(file: &[u8]) -> impl Iterator<Item = &[u8]> {
    file.split_inclusive(|&b| b == 0x1d)
        .filter(|raw| raw.len() > 1)
}

/// Branches and item types the export refers to, created before importing it.
fn setup(store: &ElzevirStore) {
    let mut branches = BTreeSet::new();
    let mut item_types = BTreeSet::new();
    for raw in records(STAFF_EXPORT) {
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

fn options(parallelism: Parallelism, chunk: usize) -> ImportOptions {
    ImportOptions {
        limits: Limits::default(),
        parallelism,
        chunk: NonZeroUsize::new(chunk).unwrap(),
        matching: None,
        translations: TypeTranslations::default(),
    }
}

fn threads(n: usize) -> Parallelism {
    Parallelism::Threads(NonZeroUsize::new(n).unwrap())
}

fn import(store: &ElzevirStore, file: &[u8], options: &ImportOptions) -> ImportReport {
    import_marc21(file, store, options).unwrap()
}

fn count(store: &ElzevirStore, sql: &str) -> i64 {
    store
        .readers()
        .read(|c| c.query_row(sql, [], |row| row.get(0)))
        .unwrap()
}

#[test]
fn the_staff_export_imports_whole() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(&dir);
    setup(&store);
    let report = import(&store, STAFF_EXPORT, &options(threads(4), 32));
    assert_eq!(report.failures, []);
    assert_eq!(
        (report.read, report.imported, report.items),
        (200, 200, 253)
    );
    assert_eq!(count(&store, "SELECT count(*) FROM instances"), 200);
    assert_eq!(count(&store, "SELECT count(*) FROM items"), 253);
    assert_eq!(count(&store, "SELECT count(*) FROM source_records"), 200);
    // Every record arrived as UTF-8 MARC21, so each source is the file's bytes.
    let first = records(STAFF_EXPORT).next().unwrap();
    let stored: Vec<u8> = store
        .readers()
        .read(|c| {
            c.query_row(
                "SELECT record FROM source_records ORDER BY instance_id LIMIT 1",
                [],
                |row| row.get(0),
            )
        })
        .unwrap();
    assert_eq!(stored, first);
    // One reindex effect per record, written with it.
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM effect_outbox WHERE kind = 'catalog.reindex_instance'"
        ),
        200
    );
}

#[test]
fn parallel_and_serial_imports_build_the_same_database() {
    let digest = |options: &ImportOptions| {
        let dir = tempfile::tempdir().unwrap();
        let store = open(&dir);
        setup(&store);
        let report = import(&store, STAFF_EXPORT, options);
        assert_eq!(report.imported, 200);
        store.digest().unwrap()
    };
    let serial = digest(&options(Parallelism::Serial, 1024));
    assert_eq!(serial, digest(&options(threads(4), 7)));
    assert_eq!(serial, digest(&options(threads(16), 64)));
    assert_eq!(serial, digest(&options(Parallelism::Serial, 3)));
}

#[test]
fn the_digest_sees_one_changed_value() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(&dir);
    setup(&store);
    let before = store.digest().unwrap();
    store
        .submit(WriteCommand::Catalog(CatalogCommand::CreateHub {
            title: "Dom Casmurro".to_owned(),
        }))
        .unwrap()
        .wait()
        .unwrap();
    assert_ne!(before, store.digest().unwrap());
}

#[test]
fn a_broken_record_is_reported_and_the_rest_imported() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(&dir);
    setup(&store);
    let mut file = Vec::new();
    let mut taken = records(STAFF_EXPORT);
    file.extend_from_slice(taken.next().unwrap());
    let broken_at = file.len();
    file.extend_from_slice(b"00042nam  2200000   4500\x1d");
    file.extend_from_slice(taken.next().unwrap());
    let report = import(&store, &file, &options(threads(2), 2));
    assert_eq!((report.read, report.imported), (3, 2));
    let [failed] = report.failures.as_slice() else {
        panic!("expected one failure, got {:?}", report.failures);
    };
    assert_eq!(
        (failed.index, failed.offset),
        (1, u64::try_from(broken_at).unwrap())
    );
    assert!(matches!(failed.failure, RecordFailure::Unreadable(_)));
}

#[test]
fn a_refused_record_is_reported_and_the_rest_imported() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(&dir);
    // No branches or item types: every record with items is refused by the writer.
    let report = import(&store, STAFF_EXPORT, &options(threads(4), 50));
    assert_eq!(report.read, 200);
    assert!(!report.failures.is_empty());
    assert_eq!(
        report.imported,
        200 - u64::try_from(report.failures.len()).unwrap()
    );
    assert!(report.failures.windows(2).all(|w| w[0].index < w[1].index));
    assert!(report.failures.iter().all(|failed| matches!(
        failed.failure,
        RecordFailure::Rejected(RecordRejected::Refused(
            CatalogRefusal::UnknownBranch(_) | CatalogRefusal::UnknownItemType(_)
        ))
    )));
    // A refused record leaves nothing behind, effect included.
    let imported = i64::try_from(report.imported).unwrap();
    assert_eq!(count(&store, "SELECT count(*) FROM instances"), imported);
    assert_eq!(
        count(&store, "SELECT count(*) FROM effect_outbox"),
        imported
    );
}

/// A one-record file: a Leader that says "text, monograph" and a `942 $c` of `type_code`.
fn typed_record(type_code: &str) -> Vec<u8> {
    use elzevir_marc::{Field, Subfield, Tag};
    let field = |tag: &[u8; 3], code: u8, value: &str| Field::Data {
        tag: Tag::new(*tag),
        indicators: *b"  ",
        subfields: vec![Subfield {
            code,
            value: value.as_bytes().to_vec(),
        }],
    };
    let record = Record {
        leader: *b"00000nam a2200000 a 4500",
        fields: vec![
            field(b"245", b'a', "A translated record"),
            field(b"942", b'c', type_code),
        ],
    };
    let mut out = Vec::new();
    record.write_iso2709(&mut out).unwrap();
    out
}

#[test]
fn the_stored_type_translation_fills_only_what_the_record_does_not_say() {
    use elzevir_app::catalog::StatementReader;
    use elzevir_app::description::concept;
    use elzevir_app::koha_types::{TypeTranslationSource, parse};
    use elzevir_domain::statement::{EntityKind, EntityRef};

    let dir = tempfile::tempdir().unwrap();
    let store = open(&dir);
    let translations = parse(
        "# type\tentity\tproperty\tvalue\n\
         X1\twork\tclass\tCartography\n\
         X1\twork\tcontent\tcontentTypes:cri\n\
         X1\tinstance\tmedia\tmediaTypes:n\n",
    )
    .unwrap();
    store
        .submit(WriteCommand::Catalog(CatalogCommand::SetTypeTranslations(
            translations.clone(),
        )))
        .unwrap()
        .wait()
        .unwrap();
    assert_eq!(store.type_translations().unwrap(), translations);

    let options = ImportOptions {
        translations: store.type_translations().unwrap(),
        ..options(Parallelism::Serial, 1)
    };
    let file = [typed_record("X1"), typed_record("X2")].concat();
    assert_eq!(import(&store, &file, &options).imported, 2);
    let description = |entity, id| store.description(EntityRef { entity, id }).unwrap();
    let translated = description(EntityKind::Work, 1);
    // The Leader's classes stay first; the translation adds the class it lacks.
    assert_eq!(translated.classes, ["Text", "Monograph", "Cartography"]);
    // The Leader already gives content, so the translated content is not added.
    assert_eq!(
        translated.statements,
        [concept("content", "contentTypes", "txt")]
    );
    assert!(
        description(EntityKind::Instance, 1)
            .statements
            .contains(&concept("media", "mediaTypes", "n"))
    );
    // A type with no translation imports as the record says.
    assert_eq!(
        description(EntityKind::Work, 2).classes,
        ["Text", "Monograph"]
    );

    // Setting a new translation replaces the old one.
    store
        .submit(WriteCommand::Catalog(CatalogCommand::SetTypeTranslations(
            TypeTranslations::default(),
        )))
        .unwrap()
        .wait()
        .unwrap();
    assert_eq!(
        store.type_translations().unwrap(),
        TypeTranslations::default()
    );
}
