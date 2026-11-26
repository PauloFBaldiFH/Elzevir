// SPDX-License-Identifier: GPL-3.0-or-later
//! UNIMARC in and out through the store (docs/specs/catalogo/unimarc.md, items 15 to 19).
#![expect(
    clippy::unwrap_used,
    reason = "integration test: helper functions fail the test on error"
)]

use elzevir_app::catalog::{CatalogCommand, CatalogReader, InstanceChange, InstanceUpdate};
use elzevir_app::commands::WriteCommand;
use elzevir_app::export::{RecordFormat, export_record, export_selection};
use elzevir_app::import::{ImportOptions, import_marc21};
use elzevir_domain::catalog::InstanceId;
use elzevir_marc::{Field, Limits, Record, RecordRef, Subfield, Tag};
use elzevir_store::{ElzevirHandler, ElzevirStore, StoreConfig};

fn data(tag: [u8; 3], indicators: [u8; 2], pairs: &[(u8, &str)]) -> Field {
    Field::Data {
        tag: Tag::new(tag),
        indicators,
        subfields: pairs
            .iter()
            .map(|&(code, value)| Subfield {
                code,
                value: value.as_bytes().to_vec(),
            })
            .collect(),
    }
}

/// A fictitious UNIMARC book in UTF-8, with a local field the projection does not read.
fn book() -> Record {
    Record {
        leader: *b"00000nam  2200000   4500",
        fields: vec![
            Field::Control {
                tag: Tag::new(*b"001"),
                value: b"42".to_vec(),
            },
            data(
                *b"100",
                *b"  ",
                &[(b'a', "20261126d2009    u  y0pory50      ba")],
            ),
            data(*b"101", *b"0 ", &[(b'a', "por")]),
            data(
                *b"200",
                *b"1 ",
                &[(b'a', "Rios do Paraná"), (b'f', "Ana Lemos")],
            ),
            data(
                *b"210",
                *b"  ",
                &[
                    (b'a', "Curitiba"),
                    (b'c', "Editora Exemplo"),
                    (b'd', "2009"),
                ],
            ),
            data(
                *b"606",
                *b"  ",
                &[(b'a', "Rios"), (b'y', "Paraná"), (b'2', "local")],
            ),
            data(
                *b"700",
                *b" 1",
                &[(b'a', "Lemos"), (b'b', "Ana"), (b'4', "070")],
            ),
            data(*b"930", *b"  ", &[(b'a', "Doação da família")]),
        ],
    }
}

fn imported(dir: &tempfile::TempDir, record: &Record) -> ElzevirStore {
    let store = ElzevirStore::open(
        &dir.path().join("unimarc.db"),
        &StoreConfig::default(),
        ElzevirHandler,
    )
    .unwrap();
    let mut bytes = Vec::new();
    record.write_iso2709(&mut bytes).unwrap();
    let report = import_marc21(bytes.as_slice(), &store, &ImportOptions::default()).unwrap();
    assert_eq!((report.read, report.imported), (1, 1), "{report:?}");
    store
}

fn exported(store: &ElzevirStore, format: RecordFormat) -> Vec<u8> {
    export_record(store, InstanceId(1), format, &Limits::default())
        .unwrap()
        .unwrap()
}

fn read(bytes: &[u8]) -> Record {
    Record::from_ref(&RecordRef::parse(bytes, &Limits::default()).unwrap())
}

fn field(record: &Record, tag: [u8; 3]) -> Option<&Field> {
    record
        .fields
        .iter()
        .find(|field| *field.tag().as_bytes() == tag)
}

#[test]
fn an_unchanged_unimarc_record_leaves_as_it_came() {
    let dir = tempfile::tempdir().unwrap();
    let store = imported(&dir, &book());
    let out = read(&exported(&store, RecordFormat::Unimarc));
    assert_eq!(out.fields, book().fields);
    let xml = String::from_utf8(exported(&store, RecordFormat::UnimarcXml)).unwrap();
    assert!(
        xml.contains("nam  22") && xml.contains("   4500</leader>"),
        "{xml}"
    );
    assert!(xml.contains("<subfield code=\"a\">Rios do Paraná</subfield>"));
    // The MARC21 export of the same record is the projection.
    let marc21 = read(&exported(&store, RecordFormat::Marc21));
    assert!(field(&marc21, *b"245").is_some() && field(&marc21, *b"200").is_none());
}

#[test]
fn a_legacy_record_leaves_in_utf8() {
    // ISO 5426: the acute accent (0xC2) comes before its letter.
    let mut legacy = book();
    for field in &mut legacy.fields {
        if let Field::Data { tag, subfields, .. } = field {
            for subfield in subfields.iter_mut() {
                match tag.as_bytes() {
                    b"100" => subfield
                        .value
                        .splice(26..30, b"0103".iter().copied())
                        .for_each(drop),
                    _ if subfield.value.ends_with("Paraná".as_bytes()) => {
                        let keep = subfield.value.len() - "á".len();
                        subfield.value.truncate(keep);
                        subfield.value.extend_from_slice(&[0xc2, b'a']);
                    }
                    _ => {}
                }
            }
        }
    }
    legacy
        .fields
        .retain(|field| field.tag().as_bytes() != b"930");
    let dir = tempfile::tempdir().unwrap();
    let store = imported(&dir, &legacy);
    let out = read(&exported(&store, RecordFormat::Unimarc));
    assert_eq!(field(&out, *b"200"), field(&book(), *b"200"));
    let Some(Field::Data { subfields, .. }) = field(&out, *b"100") else {
        panic!("no 100");
    };
    assert_eq!(subfields[0].value.get(26..30), Some(b"50  ".as_slice()));
}

#[test]
fn an_edited_record_goes_through_the_table_and_keeps_unread_fields() {
    let dir = tempfile::tempdir().unwrap();
    let store = imported(&dir, &book());
    let row_version = store.record_version(InstanceId(1)).unwrap().unwrap();
    store
        .submit(WriteCommand::Catalog(CatalogCommand::UpdateInstance(
            Box::new(InstanceUpdate {
                instance: InstanceId(1),
                row_version,
                changes: vec![InstanceChange::Edition(Some("2. ed.".into()))],
            }),
        )))
        .unwrap()
        .wait()
        .unwrap();
    let out = read(&exported(&store, RecordFormat::Unimarc));
    assert_eq!(
        field(&out, *b"205"),
        Some(&data(*b"205", *b"  ", &[(b'a', "2. ed.")]))
    );
    assert_eq!(field(&out, *b"930"), field(&book(), *b"930"));
    assert_eq!(
        field(&out, *b"700"),
        Some(&data(
            *b"700",
            *b" 1",
            &[(b'a', "Lemos"), (b'b', "Ana"), (b'4', "070")]
        ))
    );
    let selection = export_selection(
        &store,
        &[InstanceId(1)],
        RecordFormat::UnimarcXml,
        &Limits::default(),
    )
    .unwrap();
    let xml = String::from_utf8(selection.bytes).unwrap();
    assert!(xml.starts_with("<?xml") && xml.ends_with("</collection>\n"));
    assert!(xml.contains("<datafield tag=\"205\""));
}
