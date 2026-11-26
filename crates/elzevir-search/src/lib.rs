// SPDX-License-Identifier: GPL-3.0-or-later
//! Full-text search over the catalog with Tantivy (docs/specs/catalogo/busca.md).
//!
//! [`TantivyIndex`] implements the [`SearchIndex`] port. It owns the only index writer
//! (one owner, like the SQLite writer) and one shared [`IndexReader`], reloaded after each
//! commit and never opened per request (docs/arquitetura/03-estrategia-reconstrucao.md,
//! §2.3). Each commit stores the outbox cursor it reaches as the commit payload, so the
//! documents and the cursor can never disagree after a crash.

mod facets;
mod fields;
mod merge;
mod schema;

use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use elzevir_app::material::{CarrierKind, ContentKind, Material};
use elzevir_app::parallelism::Parallelism;
use elzevir_app::search::{
    Facet, FacetCounts, IndexBatch, SearchDocument, SearchError, SearchHit, SearchIndex,
    SearchQuery, SearchResults, SearchScope, normalized_identifier,
};
use elzevir_domain::catalog::{HubId, IdentifierKind, InstanceId, WorkId};
use tantivy::collector::{Count, TopDocs};
use tantivy::directory::MmapDirectory;
use tantivy::query::{
    AllQuery, BooleanQuery, BoostQuery, Occur, Query, QueryParser, RegexQuery, TermQuery,
};
use tantivy::schema::{IndexRecordOption, Schema, Value};
use tantivy::{
    Document, Index, IndexReader, IndexSettings, IndexWriter, Order, ReloadPolicy, Searcher,
    TantivyDocument, Term,
};

use crate::facets::FacetCollector;
use crate::schema::{Fields, HUB_FAMILY};

/// Index writer memory per indexing thread. Tantivy needs at least 15 MB per thread.
const MEMORY_PER_THREAD: usize = 32 * 1024 * 1024;

/// Tantivy refuses more indexing threads than this.
const MAX_THREADS: usize = 8;

/// The catalog's search index.
pub struct TantivyIndex {
    index: Index,
    fields: Fields,
    catalog_query_schema: Schema,
    writer: Mutex<IndexWriter>,
    reader: IndexReader,
    /// The facets of the whole catalog for the staff, by searcher generation: every
    /// browsing page shows them, and they change only with a commit.
    whole: Mutex<Option<(u64, FacetCounts)>>,
    /// The families with more than one Instance, by searcher generation, for counting.
    shared: Mutex<Option<(u64, Arc<HashSet<u64>>)>>,
}

impl TantivyIndex {
    /// Opens the index in `directory`, creating it if empty. `parallelism` sets the
    /// writer's indexing threads (`ELZEVIR_SERIAL=index` gives one).
    ///
    /// # Errors
    ///
    /// When the directory cannot be opened or emptied, or is already locked by another
    /// writer.
    pub fn open(directory: &Path, parallelism: Parallelism) -> Result<Self, SearchError> {
        let (schema, fields) = schema::build();
        let opened = MmapDirectory::open(directory).map_err(error)?;
        if Index::exists(&opened).map_err(error)?
            && Index::open(opened.clone()).map_err(error)?.schema() != schema
        {
            // The index is derived data: an index built with another schema is dropped,
            // and with no cursor the indexer replays the whole outbox into the new one.
            drop(opened);
            std::fs::remove_dir_all(directory).map_err(error)?;
            std::fs::create_dir_all(directory).map_err(error)?;
            let fresh = MmapDirectory::open(directory).map_err(error)?;
            return Self::start(
                Index::create(fresh, schema, IndexSettings::default()).map_err(error)?,
                fields,
                parallelism,
            );
        }
        let index = Index::open_or_create(opened, schema).map_err(error)?;
        Self::start(index, fields, parallelism)
    }

    /// A new index in memory, for tests and tools.
    ///
    /// # Errors
    ///
    /// When the writer or the reader cannot start.
    pub fn in_memory(parallelism: Parallelism) -> Result<Self, SearchError> {
        let (schema, fields) = schema::build();
        Self::start(Index::create_in_ram(schema), fields, parallelism)
    }

    fn start(
        mut index: Index,
        fields: Fields,
        parallelism: Parallelism,
    ) -> Result<Self, SearchError> {
        schema::register_tokenizers(&index);
        let threads = match parallelism {
            Parallelism::Serial => 1,
            Parallelism::Threads(threads) => threads.get().min(MAX_THREADS),
        };
        if threads > 1 {
            // Segments are searched in parallel: a filter that matches most of a large
            // catalog counts its facets on every core (facetas.md, item 12).
            index.set_multithread_executor(threads).map_err(error)?;
        }
        let writer = index
            .writer_with_num_threads(threads, threads.saturating_mul(MEMORY_PER_THREAD))
            .map_err(error)?;
        writer.set_merge_policy(Box::new(merge::CappedMergePolicy::default()));
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()
            .map_err(error)?;
        // Field-qualified catalog queries must not bypass the live repository visibility check.
        // Catalog fields form a prefix, so the parser uses the same Tantivy field IDs.
        let mut catalog_schema = Schema::builder();
        for (_, field) in index
            .schema()
            .fields()
            .take_while(|(id, _)| *id != fields.repository_membership)
        {
            catalog_schema.add_field(field.clone());
        }
        Ok(Self {
            catalog_query_schema: catalog_schema.build(),
            index,
            fields,
            writer: Mutex::new(writer),
            reader,
            whole: Mutex::new(None),
            shared: Mutex::new(None),
        })
    }

    /// Every stored document as JSON, sorted, to compare two indexes built from the same
    /// effects (docs/arquitetura/03-estrategia-reconstrucao.md, §1.2 item 8).
    ///
    /// # Errors
    ///
    /// When the index cannot be read.
    pub fn stored_documents(&self) -> Result<Vec<String>, SearchError> {
        let searcher = self.reader.searcher();
        let schema = self.index.schema();
        let mut documents = Vec::new();
        for segment in searcher.segment_readers() {
            let store = segment.get_store_reader(1).map_err(error)?;
            for doc in segment.doc_ids_alive() {
                let document: TantivyDocument = store.get(doc).map_err(error)?;
                documents.push(document.to_json(&schema));
            }
        }
        documents.sort();
        Ok(documents)
    }

    fn document(&self, source: &SearchDocument) -> TantivyDocument {
        let f = &self.fields;
        let mut document = TantivyDocument::default();
        document.add_u64(f.instance, id(source.instance.0));
        document.add_u64(f.work, id(source.work.0));
        if let Some(hub) = source.hub {
            document.add_u64(f.hub, id(hub.0));
        }
        document.add_u64(
            f.family,
            source
                .hub
                .map_or(id(source.work.0), |hub| id(hub.0) | HUB_FAMILY),
        );
        document.add_u64(f.opac_hidden, u64::from(source.opac_hidden));
        if let Some(deposit) = &source.repository {
            document.add_text(f.repository_membership, "published");
            for (collection, name) in &deposit.collections {
                document.add_text(f.repository_membership, format!("{:032x}", collection.0));
                document.add_text(f.digital_collection, name);
            }
        }
        document.add_text(f.title, &source.title);
        document.add_text(f.work_title, &source.work_title);
        if let Some(main) = &source.main_contributor {
            document.add_text(f.main_contributor, main);
        }
        let texts = [
            (f.other_title, &source.other_titles),
            (f.contributor, &source.contributors),
            (f.subject, &source.subjects),
            (f.note, &source.notes),
            (f.identifier, &source.identifiers),
            (f.barcode, &source.barcodes),
            (f.inventory_number, &source.inventory_numbers),
            (f.location, &source.locations),
            (f.language, &source.languages),
            (f.branch, &source.branches),
            (f.genre, &source.genres),
            (f.subject_heading, &source.subjects),
        ];
        for (field, values) in texts {
            for value in values {
                document.add_text(field, value);
            }
        }
        for call_number in &source.call_numbers {
            document.add_text(f.call_number, call_number);
            document.add_text(f.call_number_key, call_number.to_lowercase());
        }
        let optional = [
            (f.publisher, &source.publisher),
            (f.summary, &source.summary),
            (f.date, &source.date),
            (f.classification, &source.classification),
            (f.audience, &source.audience),
            (f.year, &source.year),
        ];
        for (field, value) in optional {
            if let Some(value) = value {
                document.add_text(field, value);
            }
        }
        if let Some(content) = source.material.content {
            document.add_text(f.content, content.name());
        }
        if let Some(carrier) = source.material.carrier {
            document.add_text(f.carrier, carrier.name());
        }
        document
    }

    /// The query for what a person typed: words over the descriptive fields, plus exact
    /// matches on codes (ISBN, barcode, tombo, call number) and a classification prefix
    /// (`82*`).
    fn query(&self, text: &str, scope: SearchScope) -> Box<dyn Query> {
        let f = &self.fields;
        let mut fields = vec![
            f.title,
            f.work_title,
            f.other_title,
            f.contributor,
            f.subject,
            f.publisher,
            f.summary,
            f.note,
        ];
        let schema = if matches!(scope, SearchScope::Repository(_)) {
            fields.push(f.digital_collection);
            self.index.schema()
        } else {
            self.catalog_query_schema.clone()
        };
        let mut parser = QueryParser::new(schema, fields, self.index.tokenizers().clone());
        parser.set_conjunction_by_default();
        parser.set_field_boost(f.title, 2.0);
        parser.set_field_boost(f.work_title, 2.0);
        parser.set_field_boost(f.other_title, 2.0);
        // User input is never an error: whatever the grammar cannot read is dropped.
        let (words, _ignored) = parser.parse_query_lenient(text);
        let typed = text.trim();
        let mut clauses: Vec<(Occur, Box<dyn Query>)> = vec![(Occur::Should, words)];
        let exact = [
            (
                f.identifier,
                normalized_identifier(IdentifierKind::Isbn, typed),
            ),
            (f.identifier, typed.to_owned()),
            (f.barcode, typed.to_owned()),
            (f.inventory_number, typed.to_owned()),
            (f.call_number_key, typed.to_lowercase()),
            (f.classification, typed.to_owned()),
        ];
        for (field, value) in exact {
            if !value.is_empty() {
                let term = TermQuery::new(
                    Term::from_field_text(field, &value),
                    IndexRecordOption::Basic,
                );
                // An exact code beats any word match.
                clauses.push((
                    Occur::Should,
                    Box::new(BoostQuery::new(Box::new(term), 10.0)),
                ));
            }
        }
        if let Some(prefix) = classification_prefix(typed)
            && let Ok(regex) = RegexQuery::from_pattern(&format!("{prefix}.*"), f.classification)
        {
            clauses.push((Occur::Should, Box::new(regex)));
        }
        Box::new(BooleanQuery::new(clauses))
    }

    /// The query for `asked`: its words (everything when there are none), and every filter.
    fn filtered(&self, asked: &SearchQuery<'_>, scope: SearchScope) -> Box<dyn Query> {
        let words = if asked.text.trim().is_empty() {
            Box::new(AllQuery)
        } else {
            self.query(asked.text, scope)
        };
        if asked.filters.is_empty() {
            return words;
        }
        let mut clauses: Vec<(Occur, Box<dyn Query>)> = vec![(Occur::Must, words)];
        for filter in asked.filters {
            let field = self.facet(filter.facet);
            clauses.push((
                Occur::Must,
                Box::new(TermQuery::new(
                    Term::from_field_text(field, &filter.value),
                    IndexRecordOption::Basic,
                )),
            ));
        }
        Box::new(BooleanQuery::new(clauses))
    }

    const fn facet(&self, facet: Facet) -> tantivy::schema::Field {
        let f = &self.fields;
        match facet {
            Facet::Branch => f.branch,
            Facet::Content => f.content,
            Facet::Carrier => f.carrier,
            Facet::Genre => f.genre,
            Facet::Audience => f.audience,
            Facet::Language => f.language,
            Facet::Subject => f.subject_heading,
            Facet::Year => f.year,
        }
    }

    /// The families with more than one Instance in what `searcher` reads, computed once
    /// per generation.
    fn shared_families(&self, searcher: &Searcher) -> Result<Arc<HashSet<u64>>, SearchError> {
        let generation = searcher.generation().generation_id();
        // A poisoned cache only holds a derived set; it is computed again.
        let mut cached = self.shared.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((at, shared)) = cached.as_ref()
            && *at == generation
        {
            return Ok(Arc::clone(shared));
        }
        let shared = Arc::new(facets::shared_families(searcher).map_err(error)?);
        *cached = Some((generation, Arc::clone(&shared)));
        Ok(shared)
    }

    /// The OPAC leaves out the Instances hidden from it; the staff sees them all.
    fn scoped(&self, query: Box<dyn Query>, scope: SearchScope) -> Box<dyn Query> {
        if scope == SearchScope::Staff {
            return query;
        }
        let hidden = TermQuery::new(
            Term::from_field_u64(self.fields.opac_hidden, 1),
            IndexRecordOption::Basic,
        );
        let mut clauses: Vec<(Occur, Box<dyn Query>)> =
            vec![(Occur::Must, query), (Occur::MustNot, Box::new(hidden))];
        if let SearchScope::Repository(collection) = scope {
            let key =
                collection.map_or_else(|| "published".to_owned(), |id| format!("{:032x}", id.0));
            clauses.push((
                Occur::Must,
                Box::new(TermQuery::new(
                    Term::from_field_text(self.fields.repository_membership, &key),
                    IndexRecordOption::Basic,
                )),
            ));
        }
        Box::new(BooleanQuery::new(clauses))
    }

    fn hit(&self, score: f32, document: &TantivyDocument) -> Option<SearchHit> {
        let number = |field| {
            document
                .get_first(field)
                .and_then(|value| value.as_u64())
                .and_then(|n| i64::try_from(n).ok())
        };
        let text = |field| {
            document
                .get_first(field)
                .and_then(|value| value.as_str())
                .map(str::to_owned)
        };
        Some(SearchHit {
            instance: InstanceId(number(self.fields.instance)?),
            work: WorkId(number(self.fields.work)?),
            hub: number(self.fields.hub).map(HubId),
            score,
            title: text(self.fields.title).unwrap_or_default(),
            main_contributor: text(self.fields.main_contributor),
            date: text(self.fields.date),
            material: Material {
                content: text(self.fields.content).and_then(|name| ContentKind::from_name(&name)),
                carrier: text(self.fields.carrier).and_then(|name| CarrierKind::from_name(&name)),
            },
            call_number: text(self.fields.call_number),
        })
    }
    /// Runs `query`: by relevance, or in Instance order when every score is the same.
    fn run(
        &self,
        query: &dyn Query,
        by_instance: bool,
        offset: usize,
        limit: usize,
    ) -> Result<SearchResults, SearchError> {
        let searcher = self.reader.searcher();
        if limit == 0 {
            let total = searcher.search(query, &Count).map_err(error)?;
            return Ok(SearchResults {
                total,
                hits: Vec::new(),
            });
        }
        let top = TopDocs::with_limit(limit).and_offset(offset);
        let (found, total) = if by_instance {
            // Without words every score is the same: Instance order keeps pages stable.
            let (found, total) = searcher
                .search(
                    query,
                    &(top.order_by_u64_field("instance", Order::Asc), Count),
                )
                .map_err(error)?;
            let found = found
                .into_iter()
                .map(|(_, address)| (1.0, address))
                .collect();
            (found, total)
        } else {
            searcher
                .search(query, &(top.order_by_score(), Count))
                .map_err(error)?
        };
        let mut hits = Vec::with_capacity(found.len());
        for (score, address) in found {
            let document: TantivyDocument = searcher.doc(address).map_err(error)?;
            if let Some(hit) = self.hit(score, &document) {
                hits.push(hit);
            }
        }
        hits.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then(a.instance.0.cmp(&b.instance.0))
        });
        Ok(SearchResults { total, hits })
    }
}

impl SearchIndex for TantivyIndex {
    fn cursor(&self) -> Result<i64, SearchError> {
        let metas = self.index.load_metas().map_err(error)?;
        metas.payload.map_or(Ok(0), |payload| {
            payload
                .parse()
                .map_err(|_| SearchError(format!("index cursor `{payload}` is not a number")))
        })
    }

    fn apply(&self, batch: IndexBatch) -> Result<(), SearchError> {
        // A poisoned lock means an earlier apply panicked; Tantivy discards uncommitted
        // work on the next commit or rollback, so the writer is still usable.
        let mut writer = self.writer.lock().unwrap_or_else(PoisonError::into_inner);
        let replaced = batch
            .upserts
            .iter()
            .map(|document| document.instance)
            .chain(batch.removals.iter().copied());
        for instance in replaced {
            writer.delete_term(Term::from_field_u64(self.fields.instance, id(instance.0)));
        }
        for document in &batch.upserts {
            writer
                .add_document(self.document(document))
                .map_err(error)?;
        }
        let mut commit = writer.prepare_commit().map_err(error)?;
        commit.set_payload(&batch.cursor.to_string());
        commit.commit().map_err(error)?;
        drop(writer);
        self.reader.reload().map_err(error)
    }

    fn search(
        &self,
        asked: &SearchQuery<'_>,
        scope: SearchScope,
        offset: usize,
        limit: usize,
    ) -> Result<SearchResults, SearchError> {
        let query = self.scoped(self.filtered(asked, scope), scope);
        self.run(&*query, asked.text.trim().is_empty(), offset, limit)
    }

    fn facets(
        &self,
        asked: &SearchQuery<'_>,
        scope: SearchScope,
    ) -> Result<FacetCounts, SearchError> {
        let searcher = self.reader.searcher();
        let whole =
            asked.text.trim().is_empty() && asked.filters.is_empty() && scope == SearchScope::Staff;
        let generation = searcher.generation().generation_id();
        if whole {
            // A poisoned cache only holds counts; they are recomputed below.
            let cached = self.whole.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some((at, counts)) = cached.as_ref()
                && *at == generation
            {
                return Ok(counts.clone());
            }
        }
        let query = self.scoped(self.filtered(asked, scope), scope);
        let collector = FacetCollector {
            shared: self.shared_families(&searcher)?,
        };
        let families = searcher.search(&*query, &collector).map_err(error)?;
        let counts = facets::counts(families, asked.filters);
        if whole {
            *self.whole.lock().unwrap_or_else(PoisonError::into_inner) =
                Some((generation, counts.clone()));
        }
        Ok(counts)
    }
}

/// `82*` or `869.3*` (digits and dots before a final `*`) asks for a classification prefix.
fn classification_prefix(typed: &str) -> Option<String> {
    let prefix = typed.strip_suffix('*')?;
    (!prefix.is_empty() && prefix.bytes().all(|b| b.is_ascii_digit() || b == b'.'))
        .then(|| prefix.replace('.', "\\."))
}

/// Ids are positive SQLite rowids; a negative one would be a bug upstream, stored as 0.
fn id(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

fn error(e: impl std::fmt::Display) -> SearchError {
    SearchError(e.to_string())
}

#[cfg(test)]
mod tests {
    use elzevir_app::search::{FACET_VALUES, Filter};

    use super::*;

    #[test]
    fn classification_prefixes_are_digits_and_dots() {
        assert_eq!(classification_prefix("82*").as_deref(), Some("82"));
        assert_eq!(classification_prefix("869.3*").as_deref(), Some("869\\.3"));
        assert_eq!(classification_prefix("82"), None);
        assert_eq!(classification_prefix("*"), None);
        assert_eq!(classification_prefix("dom*"), None);
    }

    /// An Instance of `work` (in `hub`) in `languages`, with Items at `branches`.
    fn document(
        instance: i64,
        work: i64,
        hub: Option<i64>,
        languages: &[&str],
        branches: &[&str],
    ) -> SearchDocument {
        let strings = |values: &[&str]| values.iter().map(|v| (*v).to_owned()).collect();
        SearchDocument {
            repository: None,
            instance: InstanceId(instance),
            work: WorkId(work),
            hub: hub.map(HubId),
            title: format!("Livro {instance}"),
            work_title: format!("Livro {instance}"),
            other_titles: Vec::new(),
            main_contributor: None,
            contributors: Vec::new(),
            subjects: vec!["Estatística".to_owned()],
            publisher: None,
            summary: None,
            notes: Vec::new(),
            date: Some("c1998".to_owned()),
            material: Material::default(),
            classification: None,
            identifiers: Vec::new(),
            call_numbers: Vec::new(),
            locations: Vec::new(),
            barcodes: Vec::new(),
            inventory_numbers: Vec::new(),
            languages: strings(languages),
            branches: strings(branches),
            genres: vec!["Novels".to_owned()],
            audience: None,
            year: Some("1998".to_owned()),
            opac_hidden: false,
        }
    }

    fn indexed(documents: Vec<SearchDocument>) -> TantivyIndex {
        let index = TantivyIndex::in_memory(Parallelism::Serial).unwrap();
        index
            .apply(IndexBatch {
                upserts: documents,
                removals: Vec::new(),
                cursor: 1,
            })
            .unwrap();
        index
    }

    fn values(counts: &FacetCounts, facet: Facet) -> Vec<(String, usize)> {
        counts
            .facets
            .iter()
            .find(|(f, _)| *f == facet)
            .map(|(_, values)| {
                values
                    .iter()
                    .map(|v| (v.value.clone(), v.families))
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn facets_count_families_not_instances() {
        let index = indexed(vec![
            // A Hub with two Works, three Instances in Portuguese.
            document(1, 10, Some(5), &["por"], &["1"]),
            document(2, 10, Some(5), &["por", "eng"], &["1", "2"]),
            document(3, 11, Some(5), &["por"], &[]),
            // A Work alone with the Hub's number: another family.
            document(4, 5, None, &["por"], &["2"]),
            document(5, 12, None, &["spa"], &["1"]),
        ]);
        let all = SearchQuery::words("");
        let counts = index.facets(&all, SearchScope::Staff).unwrap();
        assert_eq!(counts.families, 3);
        assert_eq!(
            values(&counts, Facet::Language),
            [
                ("por".to_owned(), 2),
                ("eng".to_owned(), 1),
                ("spa".to_owned(), 1)
            ]
        );
        assert_eq!(
            values(&counts, Facet::Branch),
            [("1".to_owned(), 2), ("2".to_owned(), 2)]
        );
        assert_eq!(values(&counts, Facet::Year), [("1998".to_owned(), 3)]);
        assert_eq!(values(&counts, Facet::Genre), [("Novels".to_owned(), 3)]);
        assert_eq!(
            values(&counts, Facet::Subject),
            [("Estatística".to_owned(), 3)]
        );
        assert!(values(&counts, Facet::Audience).is_empty());
        assert_eq!(
            index.search(&all, SearchScope::Staff, 0, 10).unwrap().total,
            5
        );
    }

    #[test]
    fn filters_narrow_both_hits_and_counts() {
        let index = indexed(vec![
            document(1, 10, None, &["por"], &["1"]),
            document(2, 11, None, &["por", "eng"], &["2"]),
            document(3, 12, None, &["eng"], &["2"]),
        ]);
        let one = Filter::parse([("language", "por")]);
        let asked = SearchQuery {
            text: "",
            filters: &one,
        };
        let found = index.search(&asked, SearchScope::Staff, 0, 10).unwrap();
        let ids: Vec<i64> = found.hits.iter().map(|hit| hit.instance.0).collect();
        assert_eq!((found.total, ids), (2, vec![1, 2]));
        let counts = index.facets(&asked, SearchScope::Staff).unwrap();
        assert_eq!(counts.families, 2);
        assert_eq!(
            values(&counts, Facet::Branch),
            [("1".to_owned(), 1), ("2".to_owned(), 1)]
        );
        // Two values of one facet: both must hold.
        let both = Filter::parse([("language", "por"), ("language", "eng")]);
        let asked = SearchQuery {
            text: "livro",
            filters: &both,
        };
        let found = index.search(&asked, SearchScope::Staff, 0, 10).unwrap();
        assert_eq!(
            found.hits.iter().map(|h| h.instance.0).collect::<Vec<_>>(),
            [2]
        );
        let none = Filter::parse([("branch", "1"), ("language", "eng")]);
        let asked = SearchQuery {
            text: "",
            filters: &none,
        };
        assert_eq!(
            index
                .search(&asked, SearchScope::Staff, 0, 10)
                .unwrap()
                .total,
            0
        );
        assert_eq!(
            index.facets(&asked, SearchScope::Staff).unwrap(),
            FacetCounts::default()
        );
    }

    #[test]
    fn a_facet_shows_twenty_values_and_the_active_one() {
        let documents = (1..=25)
            .map(|n| {
                let year = format!("{}", 1900 + n);
                let mut document = document(n, n, None, &["por"], &[]);
                document.year = Some(year);
                document
            })
            .collect();
        let index = indexed(documents);
        let all = SearchQuery::words("");
        let counts = index.facets(&all, SearchScope::Staff).unwrap();
        let years = values(&counts, Facet::Year);
        assert_eq!(years.len(), FACET_VALUES);
        // Ties go by value.
        assert_eq!(years.first().map(|(v, _)| v.as_str()), Some("1901"));
        let late = vec![Filter {
            facet: Facet::Year,
            value: "1925".to_owned(),
        }];
        let asked = SearchQuery {
            text: "",
            filters: &late,
        };
        let counts = index.facets(&asked, SearchScope::Staff).unwrap();
        assert_eq!(values(&counts, Facet::Year), [("1925".to_owned(), 1)]);
        let searcher = index.reader.searcher();
        let collector = FacetCollector {
            shared: index.shared_families(&searcher).unwrap(),
        };
        let counts = facets::counts(searcher.search(&AllQuery, &collector).unwrap(), &late);
        let years = values(&counts, Facet::Year);
        assert_eq!(years.len(), FACET_VALUES + 1);
        assert_eq!(years.last().map(|(v, _)| v.as_str()), Some("1925"));
    }

    #[test]
    fn a_family_split_across_segments_counts_once() {
        let index = indexed(vec![document(1, 10, Some(5), &["por"], &["1"])]);
        // A second commit makes a second segment with the same Hub.
        index
            .apply(IndexBatch {
                upserts: vec![
                    document(2, 11, Some(5), &["por"], &["1"]),
                    document(3, 12, None, &["por"], &["1"]),
                ],
                removals: Vec::new(),
                cursor: 2,
            })
            .unwrap();
        assert!(index.reader.searcher().segment_readers().len() >= 2);
        let filters = Filter::parse([("branch", "1")]);
        let asked = SearchQuery {
            text: "",
            filters: &filters,
        };
        let counts = index.facets(&asked, SearchScope::Staff).unwrap();
        assert_eq!(counts.families, 2);
        assert_eq!(values(&counts, Facet::Language), [("por".to_owned(), 2)]);
    }

    #[test]
    fn the_whole_catalog_counts_follow_each_commit() {
        let index = indexed(vec![document(1, 10, None, &["por"], &[])]);
        let all = SearchQuery::words("");
        assert_eq!(index.facets(&all, SearchScope::Staff).unwrap().families, 1);
        assert_eq!(index.facets(&all, SearchScope::Staff).unwrap().families, 1);
        index
            .apply(IndexBatch {
                upserts: vec![document(2, 11, None, &["eng"], &[])],
                removals: Vec::new(),
                cursor: 2,
            })
            .unwrap();
        let counts = index.facets(&all, SearchScope::Staff).unwrap();
        assert_eq!(counts.families, 2);
        assert_eq!(values(&counts, Facet::Language).len(), 2);
    }

    #[test]
    fn an_index_with_another_schema_is_rebuilt() {
        let dir = tempfile::tempdir().unwrap();
        let mut old = Schema::builder();
        old.add_text_field("title", tantivy::schema::STORED);
        let directory = MmapDirectory::open(dir.path()).unwrap();
        drop(Index::create(directory, old.build(), IndexSettings::default()).unwrap());
        let index = TantivyIndex::open(dir.path(), Parallelism::Serial).unwrap();
        assert_eq!(index.cursor().unwrap(), 0);
        assert_eq!(index.index.schema(), schema::build().0);
    }
}
