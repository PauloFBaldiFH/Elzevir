// SPDX-License-Identifier: GPL-3.0-or-later
//! Staged revision, source, writer and cleanup contracts.
#![expect(
    clippy::unwrap_used,
    reason = "test fixtures, CLI processes and database setup must succeed"
)]

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use elzevir_app::catalog::CatalogCommand;
use elzevir_app::codes::{CodeTable, NamedCode};
use elzevir_app::commands::WriteCommand;
use elzevir_app::marc_mapping::marc_to_bibframe;
use elzevir_marc::{Field, Limits, MarcReader, Record, Subfield, Tag};
use elzevir_store::{ElzevirHandler, ElzevirStore, StoreConfig};

const FIXTURE: &[u8] = include_bytes!("../../elzevir-app/tests/data/koha-staff-bibs-1-200.mrc");

fn command(data: &Path, operator: &str) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_elzevir"));
    command
        .args(["import-batch", "--data-dir"])
        .arg(data)
        .args(["--operator", operator]);
    command
}

fn run(data: &Path, args: &[&str]) -> Output {
    command(data, "1").args(args).output().unwrap()
}

fn success(output: Output) -> String {
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout).unwrap()
}

fn refusal(output: &Output, message: &str) {
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(message),
        "{output:?}"
    );
}

fn stage(dir: &Path, input: &Path) -> String {
    let output = success(command(dir, "1").arg("stage").arg(input).output().unwrap());
    let id = output.split_ascii_whitespace().nth(1).unwrap().to_owned();
    assert_eq!(id.len(), 32);
    id
}

fn export(dir: &Path) -> Vec<u8> {
    let output = Command::new(env!("CARGO_BIN_EXE_elzevir"))
        .args(["export", "--data-dir"])
        .arg(dir)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    output.stdout
}

fn record(title: &str) -> Vec<u8> {
    let source = Record {
        leader: *b"00000nam a2200000 a 4500",
        fields: vec![
            Field::Control {
                tag: Tag::new(*b"001"),
                value: b"synthetic identity".to_vec(),
            },
            Field::Data {
                tag: Tag::new(*b"245"),
                indicators: *b"10",
                subfields: vec![Subfield {
                    code: b'a',
                    value: title.as_bytes().to_vec(),
                }],
            },
            Field::Data {
                tag: Tag::new(*b"999"),
                indicators: *b"  ",
                subfields: vec![
                    Subfield {
                        code: b'c',
                        value: b"123".to_vec(),
                    },
                    Subfield {
                        code: b'd',
                        value: b"321".to_vec(),
                    },
                ],
            },
        ],
    };
    let mut bytes = Vec::new();
    source.write_iso2709(&mut bytes).unwrap();
    bytes
}

fn recipe(dir: &Path, value: &str) -> String {
    let path = dir.join("recipe.json");
    fs::write(&path, value).unwrap();
    path.to_str().unwrap().to_owned()
}

#[test]
fn partial_koha_sql_uses_preview_revision_and_the_same_iso_writer() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path();
    let path = data.join("catalog.txt");
    let original = record("Synthetic Koha record");
    let expected = record("Reviewed Koha record");
    let mut xml = String::new();
    let parsed = elzevir_marc::RecordRef::parse(&original, &Limits::default()).unwrap();
    Record::from_ref(&parsed)
        .write_marcxml_document(&mut xml)
        .unwrap();
    let source = format!(
        "INSERT INTO borrower_attributes (note) VALUES ('SYNTHETIC_PRIVATE');\n\
         INSERT INTO biblio_metadata (metadata,biblionumber) VALUES ('{xml}',123);"
    );
    fs::write(&path, &source).unwrap();
    let preview = success(
        Command::new(env!("CARGO_BIN_EXE_elzevir"))
            .args(["import", "--preview", "--data-dir"])
            .arg(data)
            .arg(&path)
            .output()
            .unwrap(),
    );
    assert!(preview.contains("KohaSqlDump") && preview.contains("Synthetic Koha record"));
    assert!(!preview.contains("SYNTHETIC_PRIVATE"));
    assert!(!data.join("elzevir.db").exists());

    // The first SQL declaration falls outside the automatic detection sample.
    fs::write(&path, format!("# {}\n{source}", "x".repeat(8192))).unwrap();
    let staged = success(
        command(data, "1")
            .args(["stage", "--koha-sql"])
            .arg(&path)
            .output()
            .unwrap(),
    );
    let id = staged.split_ascii_whitespace().nth(1).unwrap();
    let rules = recipe(
        data,
        r#"{"edits":[{"record":1,"tag":"245","change":{"op":"replace_subfield","code":"a","value":"Reviewed Koha record"}}]}"#,
    );
    success(run(data, &["edit", id, "1", &rules]));
    refusal(&run(data, &["confirm", id, "1"]), "stale batch revision");
    refusal(&run(data, &["confirm", id, "2"]), "cannot be confirmed");
    let reviewed = success(run(data, &["preview", id]));
    assert!(reviewed.contains("KohaSqlDump") && reviewed.contains("Reviewed Koha record"));
    assert!(!reviewed.contains("SYNTHETIC_PRIVATE"));
    assert!(!data.join("elzevir.db").exists());
    let receipt: serde_json::Value =
        serde_json::from_str(&success(run(data, &["confirm", id, "2"]))).unwrap();
    assert_eq!(receipt["stored"], 1);
    assert_eq!(export(data), expected);
    assert!(!data.join("imports").join(id).exists());
}

#[test]
fn positional_toml_prepares_a_sheet_and_refuses_duplicate_barcodes_in_a_new_batch() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path();
    let database = data.join("elzevir.db");
    let store = ElzevirStore::open(&database, &StoreConfig::default(), ElzevirHandler).unwrap();
    for setup in [
        CatalogCommand::CreateBranch {
            name: "Reading room".into(),
        },
        CatalogCommand::CreatePolicy {
            name: "Standard loan".into(),
        },
    ] {
        store
            .submit(WriteCommand::Catalog(setup))
            .unwrap()
            .wait()
            .unwrap();
    }
    drop(store);
    let profile = data.join("profile.toml");
    fs::write(
        &profile,
        r#"
[table]
delimiter = "comma"
encoding = "utf8"
[table.profile]
version = 1
[[table.profile.columns]]
position = 1
header = "Title"
target = "Work.title"
[[table.profile.columns]]
position = 2
header = "Title"
target = "ignore"
[[table.profile.columns]]
position = 3
header = "Barcode"
target = "Item.catalog.barcode"
[[table.profile.columns]]
position = 4
header = "Home"
target = "Item.catalog.home_branch"
[[table.profile.columns]]
position = 5
header = "Current"
target = "Item.catalog.holding_branch"
[[table.profile.columns]]
position = 6
header = "Policy"
target = "Item.catalog.policy"
"#,
    )
    .unwrap();
    let input = data.join("sheet.csv");
    let stage_sheet = |second_barcode: &str| {
        fs::write(
            &input,
            format!(
                "Title,Title,Barcode,Home,Current,Policy\n\
                 One,Ignored,native-01,Reading room,Reading room,Standard loan\n\
                 Two,Ignored,{second_barcode},Reading room,Reading room,Standard loan\n"
            ),
        )
        .unwrap();
        stage(data, &input)
    };
    let id = stage_sheet("native-02");
    refusal(
        &run(data, &["preview", &id]),
        "column 3 -> Item.catalog.barcode",
    );
    let edit = |id: &str| success(run(data, &["edit", id, "1", profile.to_str().unwrap()]));
    edit(&id);
    let preview = success(run(data, &["preview", &id]));
    assert!(preview.contains("One") && preview.contains("Two"));
    assert!(!preview.contains("Ignored"));
    refusal(&run(data, &["confirm", &id, "1"]), "stale batch revision");
    let receipt: serde_json::Value =
        serde_json::from_str(&success(run(data, &["confirm", &id, "2"]))).unwrap();
    assert_eq!(receipt["stored"], 2);
    assert_eq!(receipt["items_stored"], 2);
    let marc = export(data);

    let duplicate = stage_sheet("native-01");
    edit(&duplicate);
    refusal(
        &run(data, &["preview", &duplicate]),
        "row 3: item barcode \"native-01\" is repeated from row 2",
    );
    refusal(
        &run(data, &["confirm", &duplicate, "2"]),
        "cannot be confirmed",
    );
    assert_eq!(export(data), marc);
}

#[test]
fn preview_revision_and_original_are_required_then_only_the_edited_field_changes() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path();
    let input = data.join("input");
    let original = record("Beginning");
    fs::write(&input, &original).unwrap();
    let id = stage(data, &input);
    let batch = data.join("imports").join(&id);
    refusal(&run(data, &["confirm", &id, "1"]), "cannot be confirmed");
    refusal(
        &command(data, "2").args(["preview", &id]).output().unwrap(),
        "operator",
    );
    success(run(data, &["preview", &id]));
    let path = recipe(
        data,
        r#"{"edits":[{"record":1,"tag":"245","change":{"op":"replace_subfield","code":"a","value":"Revised title"}}]}"#,
    );
    success(run(data, &["edit", &id, "1", &path]));
    refusal(&run(data, &["confirm", &id, "1"]), "stale batch revision");
    refusal(&run(data, &["confirm", &id, "2"]), "cannot be confirmed");
    assert!(!data.join("elzevir.db").exists());
    let preview = success(run(data, &["preview", &id]));
    assert!(preview.contains("Beginning") && preview.contains("Revised title"));
    assert_eq!(fs::read(batch.join("original")).unwrap(), original);
    let receipt: serde_json::Value =
        serde_json::from_str(&success(run(data, &["confirm", &id, "2"]))).unwrap();
    assert_eq!(receipt["stored"], 1);
    assert!(
        export(data) == record("Revised title"),
        "the export changed beyond the edit"
    );
    assert!(!batch.exists());
    assert_eq!(run(data, &["confirm", &id, "2"]).status.code(), Some(1));
}

#[test]
fn tampering_and_protected_identifiers_block_confirmation() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path();
    let input = data.join("input");
    fs::write(&input, record("Original title")).unwrap();
    let id = stage(data, &input);
    let path = recipe(
        data,
        r#"{"edits":[{"tag":"999","change":{"op":"remove_subfield","code":"d"}}]}"#,
    );
    success(run(data, &["edit", &id, "1", &path]));
    refusal(
        &run(data, &["preview", &id]),
        "protected source identifiers",
    );
    let path = recipe(data, "{}");
    success(run(data, &["edit", &id, "2", &path]));
    success(run(data, &["preview", &id]));
    let batch = data.join("imports").join(&id);
    let path = batch.join("manifest.json");
    let original_manifest = fs::read(&path).unwrap();
    let mut manifest: serde_json::Value = serde_json::from_slice(&original_manifest).unwrap();
    manifest["recipe"]["accept_missing_barcodes"] = serde_json::json!([1]);
    fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    refusal(
        &run(data, &["confirm", &id, "3"]),
        "preview identity changed",
    );
    fs::write(&path, original_manifest).unwrap();
    fs::write(batch.join("original"), record("Tampered title")).unwrap();
    refusal(
        &run(data, &["confirm", &id, "3"]),
        "original identity changed",
    );
    assert!(!data.join("elzevir.db").exists());
    success(run(data, &["cancel", &id]));
    assert!(!batch.exists());
}

#[test]
fn concurrent_confirmation_can_submit_the_batch_only_once() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path();
    let input = data.join("input");
    fs::write(&input, record("One confirmation")).unwrap();
    let id = stage(data, &input);
    success(run(data, &["preview", &id]));
    let successes = std::thread::scope(|scope| {
        let first = scope.spawn(|| run(data, &["confirm", &id, "1"]));
        let second = scope.spawn(|| run(data, &["confirm", &id, "1"]));
        [first.join().unwrap(), second.join().unwrap()]
            .into_iter()
            .filter(|out| out.status.success())
            .count()
    });
    assert_eq!(successes, 1);
    assert_eq!(export(data), record("One confirmation"));
}

#[test]
fn interruption_refuses_replay_until_explicit_cleanup() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path();
    let input = data.join("input");
    fs::write(&input, record("Interrupted confirmation")).unwrap();
    let id = stage(data, &input);
    success(run(data, &["preview", &id]));
    let batch = data.join("imports").join(&id);
    let path = batch.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["state"] = serde_json::json!("confirming");
    fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    refusal(&run(data, &["confirm", &id, "1"]), "Confirming");
    refusal(&run(data, &["cancel", &id]), "acknowledge");
    assert!(batch.join("original").is_file());
    success(run(data, &["cancel", &id, "--acknowledge-interrupted"]));
    assert!(!batch.exists());
}

#[test]
fn textual_sources_show_the_changed_field_and_keep_original_positions() {
    let mrk = concat!(
        "=LDR  00000nam a2200000 a 4500\n=245  10$aExcluded\n\n",
        "=LDR  00000nam a2200000 a 4500\n=001  Old value\n=245  10$aIncluded\n",
    );
    let xml = r#"<collection xmlns="http://www.loc.gov/MARC21/slim">
    <record><leader>00000nam a2200000 a 4500</leader>
    <datafield tag="245" ind1="1" ind2="0"><subfield code="a">Excluded</subfield></datafield>
    </record>
    <record><leader>00000nam a2200000 a 4500</leader><controlfield tag="001">Old value</controlfield>
    <datafield tag="245" ind1="1" ind2="0"><subfield code="a">Included</subfield></datafield>
    </record></collection>"#;
    for (format, source) in [("MarcText", mrk), ("MarcXml", xml)] {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path();
        let input = data.join("input");
        fs::write(&input, source).unwrap();
        let id = stage(data, &input);
        let path = recipe(
            data,
            r#"{"exclude":[1],"edits":[{"record":2,"tag":"001","change":{"op":"replace_text","from":"Old","to":"New"}}]}"#,
        );
        success(run(data, &["edit", &id, "1", &path]));
        let preview = success(run(data, &["preview", &id]));
        assert!(preview.contains(format) && preview.contains("1 excluded"));
        assert!(preview.contains("record 2") && preview.contains("Old value"));
        assert!(preview.contains("New value") && !preview.contains("Excluded"));
        let receipt: serde_json::Value =
            serde_json::from_str(&success(run(data, &["confirm", &id, "2"]))).unwrap();
        assert_eq!(receipt["stored"], 1);
        assert_eq!(receipt["excluded"], 1);
    }
}

#[test]
fn batch_count_limit_preserves_existing_batches_until_explicit_cancel() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path();
    let input = data.join("input");
    fs::write(&input, record("Limited batch count")).unwrap();
    let ids: Vec<_> = (0..10).map(|_| stage(data, &input)).collect();
    refusal(
        &command(data, "2")
            .arg("stage")
            .arg(&input)
            .output()
            .unwrap(),
        "batch count limit exceeded",
    );
    for id in &ids {
        assert!(data.join("imports").join(id).join("original").is_file());
    }
    let id = ids.first().unwrap();
    success(run(data, &["cancel", id]));
    assert_ne!(stage(data, &input), *id);
    assert!(!data.join("elzevir.db").exists());
}

#[test]
fn all_excluded_records_do_not_create_a_catalog() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path();
    let input = data.join("input");
    fs::write(&input, record("Excluded")).unwrap();
    let id = stage(data, &input);
    let path = recipe(data, r#"{"exclude":[1]}"#);
    success(run(data, &["edit", &id, "1", &path]));
    success(run(data, &["preview", &id]));
    let receipt: serde_json::Value =
        serde_json::from_str(&success(run(data, &["confirm", &id, "2"]))).unwrap();
    assert_eq!(receipt["stored"], 0);
    assert_eq!(receipt["excluded"], 1);
    assert!(!data.join("elzevir.db").exists());
}

#[test]
fn missing_barcode_requires_an_explicit_choice_for_its_source_record() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path();
    let catalog = store(data);
    register_codes(
        &catalog,
        [
            (CodeTable::Branch, "test-branch".to_owned()),
            (CodeTable::ItemType, "test-type".to_owned()),
        ],
    );
    catalog.shutdown().unwrap();
    let input = data.join("input");
    fs::write(
        &input,
        concat!(
            "=LDR  00000nam a2200000 a 4500\n=245  10$aOne copy\n",
            "=952  \\\\$atest-branch$ytest-type$9123\n",
        ),
    )
    .unwrap();
    let id = stage(data, &input);
    refusal(&run(data, &["preview", &id]), "missing barcode");
    let path = recipe(data, r#"{"accept_missing_barcodes":[1]}"#);
    success(run(data, &["edit", &id, "1", &path]));
    success(run(data, &["preview", &id]));
    let receipt: serde_json::Value =
        serde_json::from_str(&success(run(data, &["confirm", &id, "2"]))).unwrap();
    assert_eq!(receipt["stored"], 1);
    assert_eq!(receipt["items_stored"], 1);
}

#[cfg(unix)]
#[test]
fn staging_and_batch_access_refuse_symlinks() {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let dir = tempfile::tempdir().unwrap();
    let data = dir.path();
    let input = data.join("input");
    fs::write(&input, record("Synthetic source")).unwrap();
    let alias = data.join("alias");
    symlink(&input, &alias).unwrap();
    refusal(
        &command(data, "1")
            .arg("stage")
            .arg(&alias)
            .output()
            .unwrap(),
        "regular file",
    );
    let id = stage(data, &input);
    let batch = data.join("imports").join(&id);
    assert_eq!(
        fs::metadata(&batch).unwrap().permissions().mode() & 0o777,
        0o700
    );
    let private = batch.join("original");
    assert_eq!(
        fs::metadata(&private).unwrap().permissions().mode() & 0o777,
        0o600
    );
    fs::remove_file(&private).unwrap();
    symlink(&input, &private).unwrap();
    refusal(&run(data, &["preview", &id]), "regular file");
    assert_eq!(fs::read(input).unwrap(), record("Synthetic source"));
}

fn store(dir: &Path) -> ElzevirStore {
    ElzevirStore::open(
        &dir.join("elzevir.db"),
        &StoreConfig::default(),
        ElzevirHandler,
    )
    .unwrap()
}

fn register_fixture_codes(store: &ElzevirStore) {
    let (mut branches, mut types) = (BTreeSet::new(), BTreeSet::new());
    let mut input = MarcReader::new(FIXTURE, Limits::default());
    while let Some(record) = input.next_record() {
        let mut record = Record::from_ref(&record.unwrap());
        record.marc21_to_utf8();
        for item in marc_to_bibframe(&record, None).unwrap().items.into_marc() {
            branches.insert(item.home_branch);
            branches.insert(item.holding_branch);
            types.insert(item.item_type);
        }
    }
    register_codes(
        store,
        branches
            .into_iter()
            .map(|code| (CodeTable::Branch, code))
            .chain(types.into_iter().map(|code| (CodeTable::ItemType, code))),
    );
}

fn register_codes(store: &ElzevirStore, codes: impl IntoIterator<Item = (CodeTable, String)>) {
    for (table, code) in codes {
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

#[test]
fn an_unedited_batch_keeps_the_i1_digest_and_the_original_iso_records() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path();
    let catalog = store(data);
    register_fixture_codes(&catalog);
    let before = catalog.digest().unwrap();
    catalog.shutdown().unwrap();
    let input = data.join("input");
    fs::write(&input, FIXTURE).unwrap();
    let id = stage(data, &input);
    success(run(data, &["preview", &id]));
    let catalog = store(data);
    assert_eq!(catalog.digest().unwrap(), before);
    catalog.shutdown().unwrap();
    success(run(data, &["confirm", &id, "1"]));
    let catalog = store(data);
    assert_eq!(catalog.digest().unwrap(), "bf89658626085174");
    catalog.shutdown().unwrap();
    assert!(
        export(data) == FIXTURE,
        "the export must match the original ISO byte for byte"
    );
}
