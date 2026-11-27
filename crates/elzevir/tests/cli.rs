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
        "OPAC passwords: 1 migrated, 1 in a format older than bcrypt left out",
        "  email: 1",
        "  userid: 2",
        // No catalog in this directory: the loans and holds wait for it.
        "loans: 0 migrated, 6 left out",
        "  issue_id 6: a number, patron, Item or date missing",
        "  issue_id 3: patron not migrated",
        "  issue_id 1: no Item, or two, with its itemnumber or barcode",
        "holds: 0 migrated, 6 left out, 1 suspended in Koha (active here), 1 queued with an end date (dropped)",
        "  reserve_id 4: no record, or two, with its biblionumber",
        "loans and holds already migrated: 0",
        "patron accounts: 2 migrated with 5 lines and 4 offsets, 0 already migrated, 1 left out",
        "  borrowernumber 3: patron not migrated",
        "account lines left out: 1; offsets left out: 1; amounts rounded to cents: 1",
        "  accountlines_id 6: a number, patron, type, amount or date missing",
        "  DOACAO: 1",
        "  MULTA_LOCAL: 1",
        "returned loans: 0 migrated (0 in a reading history, the others without patron), 0 already migrated, 6 left out",
        "  issue_id 15: a number, patron, Item or date missing",
        "  issue_id 10: no Item, or two, with its itemnumber or barcode",
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
    let again = text(&again.stdout);
    assert!(again.contains("patrons: 0 migrated, 3 already migrated"));
    assert!(
        again
            .contains("patron accounts: 0 migrated with 0 lines and 0 offsets, 2 already migrated")
    );
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

#[test]
fn koha_preferences_become_an_elzevir_toml() {
    // docs/specs/migracao/preferencias-do-koha.md: a whole-database dump, with a table the
    // importer skips.
    let dump = "CREATE DATABASE /*!32312 IF NOT EXISTS*/ `koha_library`;\nUSE `koha_library`;\n\
        CREATE TABLE `borrowers` (`borrowernumber` int(11) NOT NULL, `surname` mediumtext);\n\
        INSERT INTO `borrowers` VALUES (1,'Fictícia');\n\
        CREATE TABLE `systempreferences` (`variable` varchar(50) NOT NULL DEFAULT '', \
        `value` mediumtext DEFAULT NULL, `options` mediumtext DEFAULT NULL, \
        `explanation` mediumtext DEFAULT NULL, `type` varchar(20) DEFAULT NULL, \
        PRIMARY KEY (`variable`)) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;\n\
        INSERT INTO `systempreferences` VALUES \
        ('ExpireReservesMaxPickUpDelay','1',NULL,NULL,'YesNo'),\
        ('PatronAnonymizeDelay','1095','','','Integer'),\
        ('OAI-PMH','1',NULL,NULL,'YesNo'),\
        ('OAI-PMH:archiveID','biblioteca',NULL,NULL,'Free'),\
        ('LibraryName','Biblioteca Pública de Exemplo',NULL,NULL,'Free'),\
        ('OPACUserCSS','#logo { display: none; }\\n',NULL,NULL,'free'),\
        ('SMSSendPassword','fictícia',NULL,NULL,'free'),\
        ('dateformat','us','metric|us|iso|dmydot','','Choice'),\
        ('LocalPreference','x',NULL,NULL,NULL),\
        ('maxoutstanding','5','','','Integer');\n";
    let dir = tempfile::tempdir().unwrap();
    let sql = path(dir.path(), "koha.sql");
    std::fs::write(&sql, dump).unwrap();
    let output = elzevir(&["import-koha-prefs", &sql]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    let toml = text(&output.stdout);
    for line in [
        "[circulation.holds]",
        "expire_waiting = true",
        "[privacy]",
        "anonymize_expired_days = 1095",
        "[oai]",
        "enabled = false",
        "repository_identifier = \"biblioteca\"",
        "repository_name = \"Biblioteca Pública de Exemplo\"",
        "[koha_legacy]",
        "OPACUserCSS = \"#logo { display: none; }\\n\"",
        "LocalPreference = \"x\"",
        "# SMSSendPassword: a password or a key, not copied",
    ] {
        assert!(
            toml.lines().any(|found| found == line),
            "{line} not in\n{toml}"
        );
    }
    // At Koha's default: left out.
    assert!(!toml.contains("dateformat") && !toml.contains("maxoutstanding"));
    assert!(!toml.contains("fictícia"));
    let report = text(&output.stderr);
    for line in [
        "system preferences: 10 read, 2 at Koha's default left out, 5 in fields, 2 in [koha_legacy] (1 not from Koha 26.05), 1 passwords or keys not copied",
        "  ExpireReservesMaxPickUpDelay -> [circulation.holds] expire_waiting",
        "not from Koha 26.05: LocalPreference",
        "not copied: SMSSendPassword",
        "OAI-PMH was on in Koha: [oai] is written turned off; check it and set enabled = true",
    ] {
        assert!(
            report.lines().any(|found| found == line),
            "{line} not in\n{report}"
        );
    }
    let refused = elzevir(&["import-koha-prefs", &path(dir.path(), "missing.sql")]);
    assert!(!refused.status.success());
}

/// A record of the synthetic catalog, as `biblio_metadata.metadata` keeps it.
fn marcxml(biblionumber: u32, title: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\\n<record xmlns=\"http://www.loc.gov/MARC21/slim\">\
         <leader>00000nam a2200000 a 4500</leader>\
         <datafield tag=\"245\" ind1=\"0\" ind2=\"0\"><subfield code=\"a\">{title}</subfield></datafield>\
         <datafield tag=\"999\" ind1=\" \" ind2=\" \"><subfield code=\"c\">{biblionumber}</subfield>\
         <subfield code=\"d\">{biblionumber}</subfield></datafield></record>"
    )
}

/// The patrons' synthetic dump, with the catalog, the item types and the preferences of
/// the same database, and the patrons' dump alone.
fn switch_dump() -> (String, String) {
    let patrons = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../elzevir-app/tests/data/koha-patrons.sql"
    ))
    .unwrap();
    let items = "  `homebranch` varchar(10) DEFAULT NULL,\n  PRIMARY KEY (`itemnumber`)";
    assert!(patrons.contains(items));
    let rows =
        "(100,10,'B100','CTR'),(101,10,'B101','CTR'),(110,11,'B110','CTR'),(120,11,'B120','CTR')";
    assert!(patrons.contains(rows));
    let dump = patrons.clone()
        .replace(
            items,
            "  `homebranch` varchar(10) DEFAULT NULL,\n  `holdingbranch` varchar(10) DEFAULT NULL,\n  \
             `itype` varchar(10) DEFAULT NULL,\n  PRIMARY KEY (`itemnumber`)",
        )
        .replace(
            rows,
            "(100,10,'B100','CTR','CTR','LIV'),(101,10,'B101','CTR','CTR','LIV'),\
             (110,11,'B110','CTR','CTR','DVD'),(120,11,'B120','NRT','NRT','LIV'),\
             (130,12,'B130','LUA','LUA','LIV')",
        )
        .replace(
            "-- Dump completed",
            &format!(
                "CREATE TABLE `itemtypes` (`itemtype` varchar(10) NOT NULL, `description` longtext);\n\
                 INSERT INTO `itemtypes` VALUES ('LIV','Livros'),('REV','Revistas');\n\
                 CREATE TABLE `biblio_metadata` (`id` int(11) NOT NULL, `biblionumber` int(11) NOT NULL, \
                 `format` varchar(16) NOT NULL, `schema` varchar(16) NOT NULL, `metadata` longtext NOT NULL);\n\
                 INSERT INTO `biblio_metadata` VALUES (1,10,'marcxml','MARC21','{}'),(2,11,'marcxml','MARC21','{}'),\
                 (3,12,'marcxml','MARC21','{}');\n\
                 CREATE TABLE `systempreferences` (`variable` varchar(50) NOT NULL, `value` mediumtext);\n\
                 INSERT INTO `systempreferences` VALUES ('PatronAnonymizeDelay','1095'),('LocalPreference','x');\n\
                 -- Dump completed",
                marcxml(10, "Memórias de um sargento de milícias"),
                marcxml(11, "O cortiço"),
                marcxml(12, "Dom Casmurro")
            ),
        );
    (dump, patrons)
}

#[test]
fn a_koha_dump_switches_to_elzevir_in_one_command() {
    // docs/specs/migracao/troca-do-koha.md.
    let (dump, patrons) = switch_dump();
    let dir = tempfile::tempdir().unwrap();
    let sql = path(dir.path(), "library-2026-11-27.sql");
    std::fs::write(&sql, &dump).unwrap();
    let data = path(dir.path(), "data");
    let switch = elzevir(&["switch-from-koha", "--data-dir", &data, &sql]);
    assert!(switch.status.success(), "{}", text(&switch.stderr));
    let report = text(&switch.stdout);
    for line in [
        "written to elzevir.toml",
        "branches: 2 created, 0 updated",
        "patrons: 3 migrated, 0 already migrated, 3 left out, 1 with staff permissions in Koha",
        // DVD has no row in `itemtypes`: named by its code.
        "item types: 2 created with Koha's names",
        // A branch not in `branches` refuses its record, as the import does.
        "records: 3 read, 2 stored, 1 refused",
        "  biblionumber 12: unknown branch `LUA`",
        "Items: 4 stored, 0 refused",
        "loans: 2 migrated, 4 left out",
        "loans and holds already migrated: 0",
        "search index: 2 documents",
    ] {
        assert!(
            report.lines().any(|found| found == line),
            "{line} not in\n{report}"
        );
    }
    for step in [
        "reading the dump",
        "preferences",
        "catalog",
        "circulation",
        "search index",
    ] {
        assert!(
            report.contains(&format!("== {step} (")),
            "{step} not in\n{report}"
        );
    }
    for private in ["Silva", "A0001", "Acácias", "ana@example.org"] {
        assert!(!report.contains(private), "{private} in the report");
    }
    let toml = std::fs::read_to_string(dir.path().join("data/elzevir.toml")).unwrap();
    assert!(toml.contains("anonymize_expired_days = 1095"), "{toml}");

    // The index is ready before the first `serve`.
    let index = elzevir_search::TantivyIndex::open(
        &dir.path().join("data/index"),
        elzevir_app::parallelism::Parallelism::Serial,
    )
    .unwrap();
    let found = elzevir_app::search::SearchIndex::search(
        &index,
        &elzevir_app::search::SearchQuery::words("cortiço"),
        elzevir_app::search::SearchScope::Staff,
        0,
        10,
    )
    .unwrap();
    assert_eq!(found.hits.len(), 1);
    drop(index);

    // A second run finds a catalog and writes nothing.
    let again = elzevir(&["switch-from-koha", "--data-dir", &data, &sql]);
    assert!(!again.status.success());
    assert!(text(&again.stderr).contains("starts from a new data directory"));
    assert!(!dir.path().join("data/elzevir.koha.toml").exists());

    // A library that already has its own configuration keeps it; the preferences go beside.
    let configured = dir.path().join("configured");
    std::fs::create_dir(&configured).unwrap();
    std::fs::write(configured.join("elzevir.toml"), "# the library's own\n").unwrap();
    let beside = elzevir(&[
        "switch-from-koha",
        "--data-dir",
        configured.to_str().unwrap(),
        &sql,
    ]);
    assert!(beside.status.success(), "{}", text(&beside.stderr));
    assert!(text(&beside.stdout).contains("written to elzevir.koha.toml\n"));
    assert_eq!(
        std::fs::read_to_string(configured.join("elzevir.toml")).unwrap(),
        "# the library's own\n"
    );
    assert!(
        std::fs::read_to_string(configured.join("elzevir.koha.toml"))
            .unwrap()
            .contains("anonymize_expired_days = 1095")
    );

    // A dump without the catalog is refused before anything is written.
    let fresh = path(dir.path(), "fresh");
    let without = path(dir.path(), "without.sql");
    std::fs::write(&without, patrons).unwrap();
    let refused = elzevir(&["switch-from-koha", "--data-dir", &fresh, &without]);
    assert!(!refused.status.success());
    assert!(!dir.path().join("fresh/elzevir.toml").exists());
}
