// SPDX-License-Identifier: GPL-3.0-or-later
//! `MarcEdit` import preserves the I1 digest, original ISO bytes and Koha matching keys.
#![expect(
    clippy::unwrap_used,
    reason = "integration test setup and writer failures must fail the test"
)]

use std::collections::BTreeSet;
use std::fmt::Write;
use std::num::NonZeroUsize;

use elzevir_app::catalog::CatalogCommand;
use elzevir_app::codes::{CodeTable, NamedCode};
use elzevir_app::commands::WriteCommand;
use elzevir_app::export::{ExportOptions, export_marc21};
use elzevir_app::import::{
    ImportError, ImportOptions, Parallelism, RecordFailure, import_marc_text,
};
use elzevir_app::marc_mapping::marc_to_bibframe;
use elzevir_app::matching::Matching;
use elzevir_domain::catalog::MatchRule;
use elzevir_marc::{Field, Limits, MarcReader, Record};
use elzevir_store::{ElzevirHandler, ElzevirStore, StoreConfig};

const SOURCE: &[u8] = include_bytes!("../../elzevir-app/tests/data/koha-staff-bibs-1-200.mrc");
const I1_DIGEST: &str = "c8e9150313d5b95f";
const LEADER: &str = "=LDR  00000nam a2200000 a 4500\n";

fn records(iso: &[u8]) -> Vec<Record> {
    let mut reader = MarcReader::new(iso, Limits::default());
    let mut result = Vec::new();
    while let Some(record) = reader.next_record() {
        result.push(Record::from_ref(&record.unwrap()));
    }
    result
}

fn mrk(records: &[Record]) -> String {
    let mut output = String::new();
    for record in records {
        let leader = std::str::from_utf8(&record.leader)
            .unwrap()
            .replace(' ', "\\");
        writeln!(output, "=LDR  {leader}").unwrap();
        for field in &record.fields {
            write!(output, "={}  ", field.tag()).unwrap();
            match field {
                Field::Control { value, .. } => append_value(&mut output, value, true),
                Field::Data {
                    indicators: [first, second],
                    subfields,
                    ..
                } => {
                    for indicator in [*first, *second] {
                        output.push(if indicator == b' ' {
                            '\\'
                        } else {
                            char::from(indicator)
                        });
                    }
                    for subfield in subfields {
                        write!(output, "${}", char::from(subfield.code)).unwrap();
                        append_value(&mut output, &subfield.value, false);
                    }
                }
            }
            output.push('\n');
        }
        output.push('\n');
    }
    output
}

fn append_value(output: &mut String, value: &[u8], fixed: bool) {
    for ch in std::str::from_utf8(value).unwrap().chars() {
        match ch {
            '$' => output.push_str("{dollar}"),
            '\\' => output.push_str("{bsol}"),
            '{' => output.push_str("{lcub}"),
            '}' => output.push_str("{rcub}"),
            ' ' if fixed => output.push('\\'),
            _ => output.push(ch),
        }
    }
}

fn open(directory: &tempfile::TempDir) -> ElzevirStore {
    ElzevirStore::open(
        &directory.path().join("elzevir.db"),
        &StoreConfig::default(),
        ElzevirHandler,
    )
    .unwrap()
}

fn register_fixture_codes(store: &ElzevirStore, source: &[Record]) {
    let (mut branches, mut types) = (BTreeSet::new(), BTreeSet::new());
    for record in source {
        for item in marc_to_bibframe(record, None).unwrap().items.into_marc() {
            branches.insert(item.home_branch);
            branches.insert(item.holding_branch);
            types.insert(item.item_type);
        }
    }
    // Keep the same command order as the fixed I1 test.
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

fn identity_fields(records: &[Record]) -> (usize, usize, usize) {
    let (mut biblionumbers, mut biblioitems, mut items) = (0, 0, 0);
    for field in records.iter().flat_map(|record| &record.fields) {
        if let Field::Data { tag, subfields, .. } = field {
            for subfield in subfields {
                match (tag.as_bytes(), subfield.code) {
                    (b"999", b'c') => biblionumbers += 1,
                    (b"999", b'd') => biblioitems += 1,
                    (b"952", b'9') => items += 1,
                    _ => {}
                }
            }
        }
    }
    (biblionumbers, biblioitems, items)
}

#[test]
fn generated_mrk_matches_i1_and_exports_the_original_iso_bytes() {
    let source = records(SOURCE);
    let input = mrk(&source);
    for (parallelism, chunk) in [
        (Parallelism::Serial, 7),
        (Parallelism::Threads(NonZeroUsize::new(4).unwrap()), 32),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let store = open(&directory);
        register_fixture_codes(&store, &source);
        let mut options = ImportOptions {
            parallelism,
            chunk: NonZeroUsize::new(chunk).unwrap(),
            ..ImportOptions::default()
        };
        let report = import_marc_text(input.as_bytes(), &store, &options).unwrap();
        assert_eq!(
            (report.read, report.imported, report.items),
            (200, 200, 253)
        );
        assert!(report.failures.is_empty());
        assert!(report.item_failures.is_empty());
        assert_eq!(store.digest().unwrap(), I1_DIGEST);
        let mut exported = Vec::new();
        let report = export_marc21(&store, &mut exported, &ExportOptions::default()).unwrap();
        assert_eq!(report.written, 200);
        assert!(report.failures.is_empty());
        assert_eq!(identity_fields(&records(&exported)), (200, 200, 253));
        assert!(exported == SOURCE, "the exported ISO bytes changed");

        options.matching = Some(Matching::new(MatchRule::KohaBiblionumber));
        let repeated = import_marc_text(input.as_bytes(), &store, &options).unwrap();
        assert_eq!(
            (repeated.read, repeated.imported, repeated.items),
            (200, 0, 0)
        );
        assert!(repeated.failures.is_empty());
        assert_eq!(repeated.matched.len(), 200);
        assert_eq!(repeated.item_failures.len(), 253);
    }
}

#[test]
fn source_and_mapping_refusals_do_not_stop_the_next_record() {
    let directory = tempfile::tempdir().unwrap();
    let store = open(&directory);
    let first = format!("{LEADER}=001  {}\n\n", "x".repeat(10_000));
    let input = format!("{first}{LEADER}\n{LEADER}=245  10$aSynthetic title\n");
    let report = import_marc_text(input.as_bytes(), &store, &ImportOptions::default()).unwrap();
    assert_eq!(
        (report.read, report.imported, report.failures.len()),
        (3, 1, 2)
    );
    let mut failures = report.failures.iter();
    let conversion = failures.next().unwrap();
    assert_eq!((conversion.index, conversion.offset), (0, 0));
    assert!(matches!(conversion.failure, RecordFailure::Source(_)));
    let mapping = failures.next().unwrap();
    assert_eq!((mapping.index, mapping.offset), (1, first.len() as u64));
    assert!(matches!(mapping.failure, RecordFailure::Mapping(_)));
}

#[test]
fn structural_failure_preserves_previously_confirmed_records() {
    let directory = tempfile::tempdir().unwrap();
    let store = open(&directory);
    let input = format!(
        "{LEADER}=245  10$aFirst title\n\n{LEADER}=245  10$aSecond title\n\n\
         {LEADER}=245  10$a{{unknown}}\n"
    );
    let options = ImportOptions {
        chunk: NonZeroUsize::MIN,
        ..ImportOptions::default()
    };
    let error = import_marc_text(input.as_bytes(), &store, &options).unwrap_err();
    assert!(matches!(error, ImportError::MarcText(problem) if problem.index == 2));
    let mut exported = Vec::new();
    export_marc21(&store, &mut exported, &ExportOptions::default()).unwrap();
    assert_eq!(
        records(&exported)
            .first()
            .unwrap()
            .fields
            .first()
            .unwrap()
            .tag()
            .as_bytes(),
        b"245"
    );
    assert!(
        exported
            .windows(b"First title".len())
            .any(|value| value == b"First title")
    );
}
