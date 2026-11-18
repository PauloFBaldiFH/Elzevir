// SPDX-License-Identifier: GPL-3.0-or-later
//! Import detection refuses unsupported input before creating or opening the store.
#![expect(
    clippy::unwrap_used,
    reason = "integration test: input creation and subprocess failures must fail the test"
)]

use std::path::Path;
use std::process::{Command, Output};

use elzevir_marc::{Field, Record, Subfield, Tag};

const UNSUPPORTED: &[(&str, &[u8])] = &[
    ("MARCXML", b"<record xmlns=\"http://www.loc.gov/MARC21/slim\"><leader>00000nam a2200000 a 4500</leader></record>"),
    ("MarcEdit", b"=LDR  00000nam a2200000 a 4500\n=245  10$aSynthetic title\n"),
    ("CSV/TSV", b"title,barcode\nSynthetic title,SYNTHETIC-1\n"),
    ("Koha SQL", b"CREATE TABLE `biblio_metadata` (`metadata` longtext);\nINSERT INTO biblio_metadata VALUES ('SYNTHETIC_PRIVATE_SENTINEL');"),
    ("backup", b"CREATE TABLE biblio_metadata (metadata text); CREATE TABLE items (itemnumber int); CREATE TABLE borrowers (borrowernumber int); CREATE TABLE issues (itemnumber int); CREATE TABLE systempreferences (variable text);"),
    ("ZIP", b"PK\x03\x04SYNTHETIC_PRIVATE_SENTINEL"),
    ("gzip", b"\x1f\x8b\x08SYNTHETIC_PRIVATE_SENTINEL"),
    ("unknown", b"SYNTHETIC_PRIVATE_SENTINEL"),
];

fn import(file: &Path, data: &Path, create_codes: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_elzevir"));
    command.args(["import", "--data-dir"]).arg(data);
    if create_codes {
        command.arg("--create-codes");
    }
    command.arg(file).output().unwrap()
}

fn check_refusal(output: &Output, label: &str) {
    assert_eq!(output.status.code(), Some(1), "{label}: {output:?}");
    assert!(output.stdout.is_empty(), "{label}: {output:?}");
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains(label), "{label}: {error}");
    assert!(!error.contains("SYNTHETIC_PRIVATE_SENTINEL"), "{error}");
}

fn synthetic_record(title: &str) -> Vec<u8> {
    let record = Record {
        leader: *b"00000nam a2200000 a 4500",
        fields: vec![Field::Data {
            tag: Tag::new(*b"245"),
            indicators: *b"10",
            subfields: vec![Subfield {
                code: b'a',
                value: title.as_bytes().to_vec(),
            }],
        }],
    };
    let mut bytes = Vec::new();
    record.write_iso2709(&mut bytes).unwrap();
    bytes
}

#[test]
fn unsupported_formats_never_create_a_store_even_with_create_codes() {
    let directory = tempfile::tempdir().unwrap();
    // Every source deliberately has an ISO extension.
    let file = directory.path().join("source.mrc");
    let data = directory.path().join("absent");
    for &(label, bytes) in UNSUPPORTED {
        std::fs::write(&file, bytes).unwrap();
        for create_codes in [false, true] {
            check_refusal(&import(&file, &data, create_codes), label);
            assert!(!data.exists(), "{label} created a data directory");
        }
    }
}

#[test]
fn unsupported_formats_leave_an_existing_catalog_unchanged() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("source.csv");
    let data = directory.path().join("data");
    std::fs::write(&file, synthetic_record("Synthetic baseline")).unwrap();
    let initial = import(&file, &data, false);
    assert!(initial.status.success(), "{initial:?}");
    let database = data.join("elzevir.db");
    let before = std::fs::read(&database).unwrap();
    for &(label, bytes) in UNSUPPORTED {
        std::fs::write(&file, bytes).unwrap();
        for create_codes in [false, true] {
            check_refusal(&import(&file, &data, create_codes), label);
            assert_eq!(std::fs::read(&database).unwrap(), before, "{label}");
        }
    }
}

#[test]
fn iso_input_larger_than_the_detection_buffer_keeps_its_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("source.xml");
    let data = directory.path().join("data");
    let bytes = synthetic_record(&"x".repeat(9_000));
    std::fs::write(&file, &bytes).unwrap();
    let imported = import(&file, &data, false);
    assert!(imported.status.success(), "{imported:?}");
    assert!(String::from_utf8_lossy(&imported.stdout).contains("1 records read, 1 stored"));
    let exported = Command::new(env!("CARGO_BIN_EXE_elzevir"))
        .args(["export", "--data-dir"])
        .arg(&data)
        .output()
        .unwrap();
    assert!(exported.status.success(), "{exported:?}");
    assert_eq!(exported.stdout, bytes);
}

#[test]
fn empty_input_and_a_damaged_first_iso_record_keep_the_existing_report() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("source.bin");
    let data = directory.path().join("data");
    for bytes in [b"".as_slice(), b" \n\t\r"] {
        std::fs::write(&file, bytes).unwrap();
        let output = import(&file, &data, false);
        assert!(output.status.success(), "{output:?}");
        assert!(String::from_utf8_lossy(&output.stdout).contains("0 records read, 0 stored"));
    }
    let mut bytes = b"damaged\x1d".to_vec();
    bytes.extend(synthetic_record("Synthetic recovery"));
    std::fs::write(&file, bytes).unwrap();
    let output = import(&file, &data, false);
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("2 records read, 1 stored"));
}
