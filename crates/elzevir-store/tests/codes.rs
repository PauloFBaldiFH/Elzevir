// SPDX-License-Identifier: GPL-3.0-or-later
//! Names for the code tables (docs/specs/catalogo/tabelas-de-codigos.md, items 2 to 5).
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "integration test: helper functions fail the test on error"
)]

use elzevir_app::catalog::{CatalogCommand, CatalogOutput, CatalogReader, CatalogRefusal};
use elzevir_app::codes::{CodeTable, NamedCode, NamingCount};
use elzevir_app::commands::{WriteCommand, WriteOutput, WriteRefusal};
use elzevir_store::{CommandFailed, ElzevirHandler, ElzevirStore, StoreConfig};

fn store(dir: &tempfile::TempDir) -> ElzevirStore {
    ElzevirStore::open(
        &dir.path().join("elzevir.db"),
        &StoreConfig::default(),
        ElzevirHandler,
    )
    .unwrap()
}

fn run(
    store: &ElzevirStore,
    command: CatalogCommand,
) -> Result<CatalogOutput, CommandFailed<WriteRefusal>> {
    store
        .submit(WriteCommand::Catalog(command))
        .unwrap()
        .wait()
        .map(|output| {
            let WriteOutput::Catalog(output) = output else {
                panic!("unexpected output {output:?}");
            };
            output
        })
}

fn named(table: CodeTable, code: &str, name: &str) -> NamedCode {
    NamedCode {
        table,
        code: code.to_owned(),
        name: name.to_owned(),
    }
}

fn counts(output: CatalogOutput) -> [NamingCount; 4] {
    match output {
        CatalogOutput::CodesNamed(counts) => counts,
        other => panic!("unexpected output {other:?}"),
    }
}

fn count(created: usize, renamed: usize, unchanged: usize, unused: usize) -> NamingCount {
    NamingCount {
        created,
        renamed,
        unchanged,
        unused,
    }
}

/// The edge codes with the names of their rows.
fn names(store: &ElzevirStore) -> Vec<(String, String)> {
    let codes = store.codes().unwrap();
    codes
        .edge
        .into_iter()
        .map(|entry| (entry.code, entry.name))
        .collect()
}

#[test]
fn codes_are_created_renamed_or_kept() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(&dir);
    run(
        &store,
        CatalogCommand::NameCodes(vec![named(CodeTable::Branch, "B1", "B1")]),
    )
    .unwrap();
    let first = run(
        &store,
        CatalogCommand::NameCodes(vec![
            named(CodeTable::Branch, "B1", "Biblioteca Central"),
            named(CodeTable::ItemType, "T1", "Livros"),
            named(CodeTable::Location, " S1 ", " (Estantes gerais) "),
            named(CodeTable::Collection, "S1", "Geral"),
        ]),
    )
    .unwrap();
    // A shelving place or collection exists per branch once an Item uses it, so a code
    // no branch uses yet names nothing.
    assert_eq!(
        counts(first),
        [
            count(0, 1, 0, 0),
            count(1, 0, 0, 0),
            count(0, 0, 0, 1),
            count(0, 0, 0, 1)
        ]
    );
    assert_eq!(
        names(&store),
        [
            ("B1".to_owned(), "Biblioteca Central".to_owned()),
            ("T1".to_owned(), "Livros".to_owned()),
        ]
    );
    let again = run(
        &store,
        CatalogCommand::NameCodes(vec![named(CodeTable::Branch, "B1", "Biblioteca Central")]),
    )
    .unwrap();
    assert_eq!(
        counts(again),
        [
            count(0, 0, 1, 0),
            count(0, 0, 0, 0),
            count(0, 0, 0, 0),
            count(0, 0, 0, 0)
        ]
    );
    let codes = store.codes().unwrap();
    assert_eq!(codes.branch(codes.branches[0].id), "Biblioteca Central");
}

#[test]
fn one_bad_name_stores_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(&dir);
    let refused = run(
        &store,
        CatalogCommand::NameCodes(vec![
            named(CodeTable::Branch, "B1", "Biblioteca Central"),
            named(CodeTable::ItemType, "T1", "  "),
        ]),
    )
    .unwrap_err();
    assert!(matches!(
        refused,
        CommandFailed::Rejected(WriteRefusal::Catalog(CatalogRefusal::BadCodeName(1)))
    ));
    assert!(names(&store).is_empty());
}
