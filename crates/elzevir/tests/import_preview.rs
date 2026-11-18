// SPDX-License-Identifier: GPL-3.0-or-later
//! CLI preparation uses a read-only catalog and covers all implemented MARC readers.
#![expect(
    clippy::unwrap_used,
    reason = "test process and file setup must succeed"
)]

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use elzevir_marc::{Field, Record, Subfield, Tag};

fn run(dir: &Path, file: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_elzevir"))
        .args(["import", "--data-dir"])
        .arg(dir)
        .args(args)
        .arg(file)
        .output()
        .unwrap()
}

fn iso(title: Option<&str>) -> Vec<u8> {
    let mut record = Record {
        leader: *b"00000nam a2200000 a 4500",
        fields: vec![],
    };
    if let Some(title) = title {
        record.fields.push(Field::Data {
            tag: Tag::new(*b"245"),
            indicators: *b"10",
            subfields: vec![Subfield {
                code: b'a',
                value: title.as_bytes().to_vec(),
            }],
        });
    }
    let mut bytes = Vec::new();
    record.write_iso2709(&mut bytes).unwrap();
    bytes
}

#[test]
fn three_marc_formats_preview_without_creating_a_catalog() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("input.bin");
    let dir = root.path().join("absent");
    let iso = iso(Some("Synthetic preview title"));
    for bytes in [
        iso.as_slice(),
        b"=LDR  00000nam a2200000 a 4500\n=245  10$aSynthetic preview title\n",
        b"<record xmlns='http://www.loc.gov/MARC21/slim'><leader>00000nam a2200000 a 4500</leader><datafield tag='245' ind1='1' ind2='0'><subfield code='a'>Synthetic preview title</subfield></datafield></record>",
    ] {
        fs::write(&file, bytes).unwrap();
        let output = run(&dir, &file, &["--preview"]);
        assert!(output.status.success(), "{output:?}");
        let summary = String::from_utf8(output.stdout).unwrap();
        assert!(summary.contains("1 records read, 1 prepared"), "{summary}");
        assert!(summary.contains("Synthetic preview title"));
        assert!(!dir.exists());
    }
}

#[test]
fn preview_reports_errors_after_the_three_samples_without_changing_an_existing_database() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("synthetic.mrc");
    let data = root.path().join("data");
    let record = iso(Some("Synthetic record"));
    fs::write(&file, &record).unwrap();
    assert!(run(&data, &file, &[]).status.success());
    let db = data.join("elzevir.db");
    let before = fs::read(&db).unwrap();
    let mut batch = record.repeat(4);
    batch.extend(iso(None));
    fs::write(&file, batch).unwrap();
    let output = run(&data, &file, &["--preview"]);
    assert_eq!(output.status.code(), Some(1));
    let status = String::from_utf8(output.stdout).unwrap();
    assert!(status.contains("5 records read, 4 prepared"), "{status}");
    assert_eq!(status.matches("Work:").count(), 3);
    assert!(String::from_utf8_lossy(&output.stderr).contains("record 5"));
    assert_eq!(fs::read(&db).unwrap(), before);
}

#[test]
fn preview_rejects_creating_codes_and_unsupported_sources() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("synthetic.mrc");
    let data = root.path().join("data");
    fs::write(&file, iso(Some("Synthetic record"))).unwrap();
    let refused = run(&data, &file, &["--preview", "--create-codes"]);
    assert_eq!(refused.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&refused.stderr).contains("--create-codes"));
    fs::write(&file, b"\x1f\x8b\x08").unwrap();
    let refused = run(&data, &file, &["--preview"]);
    assert_eq!(refused.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&refused.stderr).contains("Gzip"));
    assert!(!data.exists());
}
