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

fn count(created: usize, renamed: usize, unchanged: usize) -> NamingCount {
    NamingCount {
        created,
        renamed,
        unchanged,
    }
}

fn names(store: &ElzevirStore) -> Vec<(String, String)> {
    let codes = store.codes().unwrap();
    [
        codes.branches,
        codes.item_types,
        codes.locations,
        codes.collections,
    ]
    .into_iter()
    .flatten()
    .map(|entry| (entry.code, entry.name))
    .collect()
}

#[test]
fn codes_are_created_renamed_or_kept() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(&dir);
    run(
        &store,
        CatalogCommand::CreateBranch {
            code: "MAIN".to_owned(),
            name: "MAIN".to_owned(),
        },
    )
    .unwrap();
    let first = run(
        &store,
        CatalogCommand::NameCodes(vec![
            named(CodeTable::Branch, "MAIN", "Castro Alves"),
            named(CodeTable::ItemType, "BK", "Livros"),
            named(CodeTable::Location, " GEN ", " (Estantes gerais) "),
            named(CodeTable::Collection, "GEN", "Geral"),
        ]),
    )
    .unwrap();
    assert_eq!(
        counts(first),
        [
            count(0, 1, 0),
            count(1, 0, 0),
            count(1, 0, 0),
            count(1, 0, 0)
        ]
    );
    // The same code in two vocabularies keeps two names.
    assert_eq!(
        names(&store),
        [
            ("MAIN".to_owned(), "Castro Alves".to_owned()),
            ("BK".to_owned(), "Livros".to_owned()),
            ("GEN".to_owned(), "(Estantes gerais)".to_owned()),
            ("GEN".to_owned(), "Geral".to_owned()),
        ]
    );
    let again = run(
        &store,
        CatalogCommand::NameCodes(vec![
            named(CodeTable::Branch, "MAIN", "Castro Alves"),
            named(CodeTable::Location, "GEN", "Estantes gerais"),
        ]),
    )
    .unwrap();
    assert_eq!(
        counts(again),
        [
            count(0, 0, 1),
            count(0, 0, 0),
            count(0, 1, 0),
            count(0, 0, 0)
        ]
    );
    assert_eq!(store.codes().unwrap().location("GEN"), "Estantes gerais");
}

#[test]
fn one_bad_name_stores_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(&dir);
    let refused = run(
        &store,
        CatalogCommand::NameCodes(vec![
            named(CodeTable::Branch, "MAIN", "Castro Alves"),
            named(CodeTable::ItemType, "BK", "  "),
        ]),
    )
    .unwrap_err();
    assert!(matches!(
        refused,
        CommandFailed::Rejected(WriteRefusal::Catalog(CatalogRefusal::BadCodeName(1)))
    ));
    assert!(names(&store).is_empty());
}
