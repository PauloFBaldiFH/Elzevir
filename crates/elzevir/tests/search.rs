// SPDX-License-Identifier: GPL-3.0-or-later
//! Import → outbox → index → search, end to end (docs/specs/catalogo/busca.md).
#![expect(
    clippy::unwrap_used,
    reason = "integration test: helper functions fail the test on error"
)]

mod common;

use std::collections::BTreeSet;
use std::num::NonZeroUsize;

use elzevir_app::catalog::CatalogReader;
use elzevir_app::parallelism::Parallelism;
use elzevir_app::search::{IndexOptions, SearchIndex, SearchQuery, SearchScope, index_pending};
use elzevir_domain::catalog::InstanceId;
use elzevir_search::TantivyIndex;
use elzevir_store::ElzevirStore;

use common::imported;

fn options(parallelism: Parallelism, batch: usize) -> IndexOptions {
    IndexOptions {
        parallelism,
        batch: NonZeroUsize::new(batch).unwrap(),
    }
}

fn threads(n: usize) -> Parallelism {
    Parallelism::Threads(NonZeroUsize::new(n).unwrap())
}

fn indexed(store: &ElzevirStore, parallelism: Parallelism, batch: usize) -> TantivyIndex {
    let index = TantivyIndex::in_memory(parallelism).unwrap();
    let report = index_pending(store, store, &index, &options(parallelism, batch)).unwrap();
    assert_eq!(
        (report.effects, report.indexed, report.removed),
        (200, 200, 0)
    );
    index
}

fn titles(store: &ElzevirStore, index: &TantivyIndex, query: &str) -> Vec<String> {
    index
        .search(&SearchQuery::words(query), SearchScope::Staff, 0, 10)
        .unwrap()
        .hits
        .into_iter()
        .map(|hit| store.instance(hit.instance).unwrap().unwrap().title)
        .collect()
}

#[test]
fn serial_and_parallel_indexing_store_the_same_documents() {
    let (_dir, store) = imported();
    let serial = indexed(&store, Parallelism::Serial, 1024)
        .stored_documents()
        .unwrap();
    assert_eq!(serial.len(), 200);
    for (parallelism, batch) in [(threads(4), 7), (threads(8), 64), (Parallelism::Serial, 3)] {
        assert_eq!(
            indexed(&store, parallelism, batch)
                .stored_documents()
                .unwrap(),
            serial
        );
    }
}

#[test]
fn indexing_again_finds_nothing_new() {
    let (_dir, store) = imported();
    let index = indexed(&store, threads(4), 50);
    let last: i64 = store
        .readers()
        .read(|c| c.query_row("SELECT max(id) FROM effect_outbox", [], |row| row.get(0)))
        .unwrap();
    assert_eq!(index.cursor().unwrap(), last);
    let again = index_pending(&store, &store, &index, &options(threads(4), 50)).unwrap();
    assert_eq!((again.effects, again.indexed), (0, 0));
}

#[test]
fn words_match_without_accents_or_case() {
    let (_dir, store) = imported();
    let index = indexed(&store, Parallelism::Serial, 1024);
    let found = titles(&store, &index, "anuario ESTATISTICO");
    assert_eq!(
        found.first().map(String::as_str),
        Some("Perfil - Anuário Estatístico")
    );
    // Subjects and contributors are searched too.
    assert!(titles(&store, &index, "iguacu").contains(&"Perfil - Anuário Estatístico".to_owned()));
    assert!(
        titles(&store, &index, "prefeitura municipal foz")
            .contains(&"Perfil - Anuário Estatístico".to_owned())
    );
    // Portuguese plurals meet their singular.
    assert!(
        titles(&store, &index, "anuarios estatisticos")
            .contains(&"Perfil - Anuário Estatístico".to_owned())
    );
    assert!(titles(&store, &index, "zzzzqqq").is_empty());
}

#[test]
fn results_come_in_pages_with_the_total_and_the_list_fields() {
    let (_dir, store) = imported();
    let index = indexed(&store, Parallelism::Serial, 1024);
    let all = index
        .search(&SearchQuery::words("medici"), SearchScope::Staff, 0, 50)
        .unwrap();
    assert_eq!((all.total, all.hits.len()), (5, 5));
    let page = index
        .search(&SearchQuery::words("medici"), SearchScope::Staff, 2, 2)
        .unwrap();
    assert_eq!(page.total, 5);
    assert_eq!(page.hits.len(), 2);
    let first = &index
        .search(
            &SearchQuery::words("anuario estatistico"),
            SearchScope::Staff,
            0,
            1,
        )
        .unwrap()
        .hits[0];
    assert_eq!(first.title, "Perfil - Anuário Estatístico");
    assert_eq!(
        first.main_contributor.as_deref(),
        Some("PREFEITURA MUNICIPAL DE FOZ DO IGUACU")
    );
    assert_eq!(first.date.as_deref(), Some("1995"));
    assert_eq!(first.call_number.as_deref(), Some("310.62 P94"));
    assert!(first.material.content.is_some());
    assert_eq!(
        index
            .search(&SearchQuery::words("medici"), SearchScope::Staff, 0, 0)
            .unwrap()
            .total,
        5
    );
}

#[test]
fn codes_match_exactly_and_come_first() {
    let (_dir, store) = imported();
    let index = indexed(&store, Parallelism::Serial, 1024);
    let first = |query: &str| -> InstanceId {
        index
            .search(&SearchQuery::words(query), SearchScope::Staff, 0, 5)
            .unwrap()
            .hits[0]
            .instance
    };
    let perfil = first("anuario estatistico");
    // Barcode, call number (any case) and classification of the first record.
    assert_eq!(first("1511"), perfil);
    assert_eq!(first("310.62 p94"), perfil);
    assert_eq!(first("310.62"), perfil);
    // ISBN with or without hyphens.
    let by_isbn = first("978-85-60342-01-3");
    assert_eq!(first("9788560342013"), by_isbn);
    let view = store.instance(by_isbn).unwrap().unwrap();
    assert!(
        view.identifiers
            .iter()
            .any(|(_, value)| value == "9788560342013")
    );
}

#[test]
fn a_variant_title_finds_its_record() {
    let (_dir, store) = imported();
    let index = indexed(&store, Parallelism::Serial, 1024);
    // The title page has "&"; only the variant title (246) spells it "e".
    let found = titles(&store, &index, "investimentos e demonstrativos");
    assert_eq!(
        found,
        ["Análise de investimentos & demonstrativos financeiros"]
    );
}

#[test]
fn a_series_finds_its_volumes() {
    let (_dir, store) = imported();
    let index = indexed(&store, Parallelism::Serial, 1024);
    // The series is in 490 and 830 only: no title, subject or note names it.
    let found = titles(&store, &index, "grandes vultos que honraram o senado");
    assert_eq!(found.len(), 1, "{found:?}");
}

#[test]
fn the_place_of_publication_finds_its_records() {
    let (_dir, store) = imported();
    let index = indexed(&store, Parallelism::Serial, 1024);
    // "Tatuí" is only in 260 $a of two records, as Koha's keyword index has it.
    assert_eq!(titles(&store, &index, "tatui").len(), 2);
}

#[test]
fn the_search_box_completes_words_when_asked() {
    let (_dir, store) = imported();
    let mut index = indexed(&store, Parallelism::Serial, 1024);
    let title = ["Perfil - Anuário Estatístico"];
    // "anuar" and "estatis" are no words of any record: only completing them finds it.
    assert!(titles(&store, &index, "anuar estatis").is_empty());
    index.complete_words(true);
    let found = titles(&store, &index, "anuar estatis");
    assert!(
        found.iter().any(|found| title.contains(&found.as_str())),
        "{found:?}"
    );
    // Every completed word must be there, and a phrase in quotes is never completed.
    assert!(titles(&store, &index, "anuar estatis zzzz").is_empty());
    assert!(titles(&store, &index, "\"anuar estatis\"").is_empty());
    // A whole word still finds its other forms through the stem.
    assert!(!titles(&store, &index, "educação").is_empty());
}

#[test]
fn a_classification_prefix_finds_the_class() {
    let (_dir, store) = imported();
    let index = indexed(&store, Parallelism::Serial, 1024);
    let hits = index
        .search(&SearchQuery::words("310*"), SearchScope::Staff, 0, 50)
        .unwrap()
        .hits;
    assert!(!hits.is_empty());
    for hit in hits {
        let classification = store
            .instance(hit.instance)
            .unwrap()
            .unwrap()
            .classification;
        assert!(classification.unwrap().starts_with("310"));
    }
}

#[test]
fn the_index_and_its_cursor_survive_a_restart() {
    let (dir, store) = imported();
    let path = dir.path().join("index");
    std::fs::create_dir(&path).unwrap();
    let before = {
        let index = TantivyIndex::open(&path, threads(2)).unwrap();
        index_pending(&store, &store, &index, &options(threads(2), 64)).unwrap();
        (index.cursor().unwrap(), index.stored_documents().unwrap())
    };
    let index = TantivyIndex::open(&path, threads(2)).unwrap();
    assert_eq!(
        (index.cursor().unwrap(), index.stored_documents().unwrap()),
        before
    );
    let again = index_pending(&store, &store, &index, &options(threads(2), 64)).unwrap();
    assert_eq!(again.effects, 0);
}

/// Reference queries whose results differ from Koha's, with the biblionumbers only Koha
/// finds and only the Elzevir finds (docs/specs/catalogo/busca.md, §4). Every other query
/// must find exactly what Koha finds among the 200 records.
const KNOWN_DIFFERENCES: &[(&str, &[i64], &[i64])] = &[
    // 590 (local note) `reimpressão`: Koha also truncates `rei`.
    ("discurso do rei", &[187], &[]),
    // 520 `gramatical`: Koha cuts it to `gramat`.
    ("gramatica", &[179], &[]),
    // 520 `contextualização histórica`: the Portuguese stemmer joins it to `história`.
    ("historia da arte", &[], &[146]),
];

#[test]
fn reference_queries_find_what_koha_finds() {
    let (_dir, store) = imported();
    let index = indexed(&store, Parallelism::Serial, 1024);
    let ids = |list: &str| -> BTreeSet<i64> {
        list.split(',')
            .filter(|id| !id.is_empty())
            .map(|id| id.parse().unwrap())
            .collect()
    };
    let recorded = include_str!("data/koha-opac-search.tsv");
    assert_eq!(recorded.lines().count(), 50);
    for line in recorded.lines() {
        let [query, _koha_total, koha] = line.split('\t').collect::<Vec<_>>()[..] else {
            panic!("bad line {line:?}");
        };
        let koha = ids(koha);
        // Instance ids follow the file, and the file is biblionumbers 1 to 200 in order.
        let ours: BTreeSet<i64> = index
            .search(&SearchQuery::words(query), SearchScope::Public, 0, 200)
            .unwrap()
            .hits
            .into_iter()
            .map(|hit| hit.instance.0)
            .collect();
        let only_koha: Vec<i64> = koha.difference(&ours).copied().collect();
        let only_ours: Vec<i64> = ours.difference(&koha).copied().collect();
        let expected = KNOWN_DIFFERENCES
            .iter()
            .find(|(known, _, _)| *known == query)
            .map_or((&[][..], &[][..]), |(_, koha, ours)| (*koha, *ours));
        assert_eq!(
            (only_koha.as_slice(), only_ours.as_slice()),
            expected,
            "query {query:?}: (only Koha, only Elzevir)"
        );
    }
}

/// One number from the database.
fn count(store: &ElzevirStore, sql: &str) -> usize {
    let n: i64 = store
        .readers()
        .read(|c| c.query_row(sql, [], |row| row.get(0)))
        .unwrap();
    usize::try_from(n).unwrap()
}

#[test]
fn facets_count_the_families_the_database_has() {
    use elzevir_app::search::{Facet, Filter};

    let (_dir, store) = imported();
    let index = indexed(&store, Parallelism::Serial, 1024);
    let all = SearchQuery::words("");
    let counts = index.facets(&all, SearchScope::Staff).unwrap();
    let families = count(
        &store,
        "SELECT COUNT(DISTINCT COALESCE('h' || w.hub_id, 'w' || w.id))
         FROM instances i JOIN works w ON w.id = i.work_id",
    );
    assert_eq!(counts.families, families);
    let languages = &counts
        .facets
        .iter()
        .find(|(facet, _)| *facet == Facet::Language)
        .unwrap()
        .1;
    let portuguese = languages.iter().find(|v| v.value == "por").unwrap();
    assert_eq!(
        portuguese.families,
        count(
            &store,
            "SELECT COUNT(DISTINCT COALESCE('h' || w.hub_id, 'w' || w.id))
             FROM instances i JOIN works w ON w.id = i.work_id
             JOIN work_languages l ON l.work_id = w.id
             JOIN terms t ON t.id = l.language WHERE t.code = 'por'",
        )
    );
    // Filtering by a value lists exactly the families the facet counted.
    let filters = Filter::parse([("language", "por")]);
    let asked = SearchQuery {
        text: "",
        filters: &filters,
    };
    let narrowed = index.facets(&asked, SearchScope::Staff).unwrap();
    assert_eq!(narrowed.families, portuguese.families);
    let found = index.search(&asked, SearchScope::Staff, 0, 500).unwrap();
    let mut heads = BTreeSet::new();
    for hit in &found.hits {
        heads.insert(hit.hub.map_or((0, hit.work.0), |hub| (1, hub.0)));
    }
    assert_eq!(heads.len(), portuguese.families);
    // Every year counted came from a date.
    assert!(counts.facets.iter().any(|(facet, values)| {
        *facet == Facet::Year && values.iter().all(|v| v.value.len() == 4)
    }));
}
