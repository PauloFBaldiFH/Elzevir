// SPDX-License-Identifier: GPL-3.0-or-later
//! `elzevir import` and `elzevir export` as a user runs them
//! (docs/specs/catalogo/linha-de-comando.md).
#![expect(
    clippy::unwrap_used,
    reason = "integration test: helper functions fail the test on error"
)]

use std::path::Path;
use std::process::{Command, Output};

const STAFF_EXPORT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../elzevir-app/tests/data/koha-staff-bibs-1-200.mrc"
);

fn elzevir(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_elzevir"))
        .args(args)
        .env_remove("ELZEVIR_SERIAL")
        .output()
        .unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn path(dir: &Path, name: &str) -> String {
    dir.join(name).to_str().unwrap().to_owned()
}

#[test]
fn a_koha_export_goes_in_and_comes_back_out_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let data = path(dir.path(), "data");

    let first = elzevir(&[
        "import",
        "--data-dir",
        &data,
        "--create-codes",
        STAFF_EXPORT,
    ]);
    assert!(first.status.success(), "{}", text(&first.stderr));
    let stdout = text(&first.stdout);
    assert!(stdout.contains("200 records read, 200 stored"), "{stdout}");
    assert!(stdout.contains("253 items stored"), "{stdout}");

    let output = path(dir.path(), "out.mrc");
    let exported = elzevir(&["export", "--data-dir", &data, "--output", &output]);
    assert!(exported.status.success(), "{}", text(&exported.stderr));
    assert!(text(&exported.stderr).contains("200 records exported, 0 failed"));
    assert_eq!(
        std::fs::read(&output).unwrap(),
        std::fs::read(STAFF_EXPORT).unwrap()
    );

    // Standard output gives the same file.
    let piped = elzevir(&["export", "--data-dir", &data]);
    assert!(piped.status.success());
    assert_eq!(piped.stdout, std::fs::read(STAFF_EXPORT).unwrap());

    // Importing it again with the Koha biblionumber rule stores nothing.
    let again = elzevir(&[
        "import",
        "--data-dir",
        &data,
        "--match",
        "koha-biblionumber",
        STAFF_EXPORT,
    ]);
    assert!(again.status.success(), "{}", text(&again.stderr));
    let stdout = text(&again.stdout);
    assert!(
        stdout.contains("200 records read, 0 stored, 200 matched"),
        "{stdout}"
    );
    assert!(
        stdout.contains("0 items stored, 0 replaced, 253 refused"),
        "{stdout}"
    );
}

#[test]
fn unknown_codes_are_refused_without_create_codes() {
    let dir = tempfile::tempdir().unwrap();
    let data = path(dir.path(), "data");
    let run = elzevir(&["import", "--data-dir", &data, STAFF_EXPORT]);
    assert!(run.status.success());
    // Records without Items go in; the others are refused whole.
    assert!(!text(&run.stdout).contains("200 stored"));
    assert!(text(&run.stderr).contains("unknown branch"));
}

#[test]
fn the_catalog_exports_as_bibframe_turtle_and_rdf_xml() {
    let dir = tempfile::tempdir().unwrap();
    let data = path(dir.path(), "data");
    let imported = elzevir(&[
        "import",
        "--data-dir",
        &data,
        "--create-codes",
        STAFF_EXPORT,
    ]);
    assert!(imported.status.success(), "{}", text(&imported.stderr));

    // The IRIs need a base.
    let refused = elzevir(&["export", "--data-dir", &data, "--format", "turtle"]);
    assert!(!refused.status.success());
    assert!(text(&refused.stderr).contains("--iri-base"));

    let base = "https://biblioteca.example/bibframe";
    let turtle = elzevir(&[
        "export",
        "--data-dir",
        &data,
        "--format",
        "turtle",
        "--iri-base",
        base,
    ]);
    assert!(turtle.status.success(), "{}", text(&turtle.stderr));
    assert!(text(&turtle.stderr).contains("200 records exported"));
    let turtle = text(&turtle.stdout);
    assert_eq!(turtle.matches("@prefix bf:").count(), 1);
    assert_eq!(turtle.matches("\n    a bf:Instance").count(), 200);

    let xml = elzevir(&[
        "export",
        "--data-dir",
        &data,
        "--format",
        "rdfxml",
        "--iri-base",
        base,
    ]);
    assert!(xml.status.success(), "{}", text(&xml.stderr));
    let xml = text(&xml.stdout);
    assert!(xml.starts_with("<?xml"));
    assert_eq!(xml.matches("<rdf:RDF").count(), 1);
    assert!(xml.ends_with("</rdf:RDF>\n"));
    assert_eq!(xml.matches("\n  <bf:Instance rdf:about=").count(), 200);
}

#[test]
fn matching_options_need_a_rule() {
    let dir = tempfile::tempdir().unwrap();
    let data = path(dir.path(), "data");
    let run = elzevir(&[
        "import",
        "--data-dir",
        &data,
        "--on-match",
        "replace",
        STAFF_EXPORT,
    ]);
    assert!(!run.status.success());
}

#[test]
fn a_data_directory_in_use_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    std::fs::create_dir(&data).unwrap();
    let lock = std::fs::File::create(data.join("elzevir.lock")).unwrap();
    lock.lock().unwrap();
    let run = elzevir(&["export", "--data-dir", data.to_str().unwrap()]);
    assert!(!run.status.success());
    assert!(text(&run.stderr).contains("another Elzevir process"));
    drop(lock);
    assert!(
        elzevir(&["export", "--data-dir", data.to_str().unwrap()])
            .status
            .success()
    );
}

#[test]
fn a_koha_dump_brings_its_patrons_once() {
    // docs/specs/migracao/leitores-do-koha.md: the dump as `koha-dump` writes it, gzipped.
    let dump = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../elzevir-app/tests/data/koha-patrons.sql"
    ))
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let gz = path(dir.path(), "library-2026-11-26.sql.gz");
    let mut encoder = flate2::write::GzEncoder::new(
        std::fs::File::create(&gz).unwrap(),
        flate2::Compression::fast(),
    );
    std::io::Write::write_all(&mut encoder, &dump).unwrap();
    encoder.finish().unwrap();
    let data = path(dir.path(), "data");
    let first = elzevir(&["migrate-koha", "--data-dir", &data, &gz]);
    assert!(first.status.success(), "{}", text(&first.stderr));
    let report = text(&first.stdout);
    for line in [
        "branches: 2 created, 0 updated",
        "patron categories: 3 created, 0 found by name, 1 with the default period of 12 months",
        "patrons: 3 migrated, 0 already migrated, 3 left out, 1 with staff permissions in Koha",
        "  borrowernumber 3: no card number, or a longer one than 32 characters",
        "  borrowernumber 4: branchcode not in branches",
        "  borrowernumber 5: categorycode not in categories",
        "restrictions: 2 migrated, 1 already ended",
        "  email: 1",
        "  userid: 2",
    ] {
        assert!(
            report.lines().any(|found| found == line),
            "{line} not in\n{report}"
        );
    }
    // No name, card number or address of a patron reaches the report.
    for private in ["Silva", "A0001", "Acácias", "ana@example.org"] {
        assert!(!report.contains(private), "{private} in the report");
    }
    let again = elzevir(&["migrate-koha", "--data-dir", &data, &gz]);
    assert!(text(&again.stdout).contains("patrons: 0 migrated, 3 already migrated"));
    let broken = path(dir.path(), "broken.sql");
    std::fs::write(
        &broken,
        "CREATE TABLE `branches` (`branchcode` varchar(10));\n",
    )
    .unwrap();
    let refused = elzevir(&["migrate-koha", "--data-dir", &data, &broken]);
    assert!(!refused.status.success());
    assert!(text(&refused.stderr).contains("no Koha patrons in this dump"));
}
