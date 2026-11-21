// SPDX-License-Identifier: GPL-3.0-or-later
//! Catalog search: what goes into the index, and the indexer that follows the effect
//! outbox (docs/specs/catalogo/busca.md).
//!
//! The index is built from the model, never from MARC (vision A.1): the indexer reads an
//! Instance through [`CatalogReader`] and turns it into a [`SearchDocument`].

use std::collections::BTreeSet;
use std::num::NonZeroUsize;

use elzevir_domain::catalog::{HubId, IdentifierKind, InstanceId, WorkId};

use crate::catalog::{CatalogReadError, CatalogReader, InstanceView, ItemView};
use crate::material::Material;
use crate::parallelism::Parallelism;

/// Effect kind asking the index to rebuild one Instance's document after commit.
pub const REINDEX_INSTANCE: &str = "catalog.reindex_instance";

/// One effect from the outbox, as the indexer understands it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// Rebuild (or remove, if it no longer exists) this Instance's document.
    ReindexInstance(InstanceId),
    /// An effect for another worker (notices, caches); the indexer skips it.
    Other,
}

/// An effect with its place in the outbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoggedEffect {
    /// Its id in the outbox; ids grow with commit order.
    pub id: i64,
    /// What it asks for.
    pub effect: Effect,
}

/// Port: the committed effects, in order.
pub trait EffectLog: Send + Sync {
    /// Up to `limit` effects with an id greater than `cursor`, by id.
    ///
    /// # Errors
    ///
    /// When storage fails or an effect payload cannot be read.
    fn after(&self, cursor: i64, limit: usize) -> Result<Vec<LoggedEffect>, EffectLogError>;
}

/// The outbox could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("cannot read the effect outbox: {0}")]
pub struct EffectLogError(pub String);

/// Everything the index knows about one Instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchDocument {
    /// The Instance; one document per Instance.
    pub instance: InstanceId,
    /// Its Work.
    pub work: WorkId,
    /// The Work's Hub, if any.
    pub hub: Option<HubId>,
    /// The Instance's title.
    pub title: String,
    /// The Work's preferred title.
    pub work_title: String,
    /// The other titles of the Work and of the Instance: variant, parallel, abbreviated.
    pub other_titles: Vec<String>,
    /// The first contributor (the main entry when the record has one).
    pub main_contributor: Option<String>,
    /// Every contributor heading, in order.
    pub contributors: Vec<String>,
    /// Every subject heading, in order.
    pub subjects: Vec<String>,
    /// Publisher.
    pub publisher: Option<String>,
    /// The Work's summary.
    pub summary: Option<String>,
    /// The notes: general, contents and local, then the typed notes, in order.
    pub notes: Vec<String>,
    /// Date of publication, as written.
    pub date: Option<String>,
    /// The content and carrier its description gives.
    pub material: Material,
    /// Classification number of the Work, such as `869.3`.
    pub classification: Option<String>,
    /// Identifiers, normalized for exact match (ISBN and ISSN without hyphens).
    pub identifiers: Vec<String>,
    /// Call numbers of the Items, distinct, in Item order.
    pub call_numbers: Vec<String>,
    /// Shelving locations of the Items, distinct, in Item order.
    pub locations: Vec<String>,
    /// Barcodes of the Items.
    pub barcodes: Vec<String>,
    /// Inventory numbers (tombos) of the Items.
    pub inventory_numbers: Vec<String>,
    /// Language codes of the Work.
    pub languages: Vec<String>,
    /// The home branches of the Items, by number, distinct.
    pub branches: Vec<String>,
    /// The labels of the Work's genres and forms.
    pub genres: Vec<String>,
    /// The Work's target audience, as written.
    pub audience: Option<String>,
    /// The year of publication, read from the date.
    pub year: Option<String>,
    /// Hidden from the public catalogue: only the staff finds it.
    pub opac_hidden: bool,
}

/// Who searches: the public catalogue leaves out hidden Instances, the staff finds them
/// all (docs/specs/catalogo/ocultar-no-opac.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchScope {
    /// The OPAC.
    Public,
    /// The staff screens.
    Staff,
}

/// Builds the document of one Instance. Pure.
#[must_use]
pub fn document_of(instance: &InstanceView, items: &[ItemView]) -> SearchDocument {
    SearchDocument {
        instance: instance.id,
        work: instance.work,
        hub: instance.hub,
        title: instance.title.clone(),
        work_title: instance.work_title.clone(),
        other_titles: instance.other_titles.clone(),
        main_contributor: instance
            .contributors
            .first()
            .map(|(_, heading)| heading.clone()),
        contributors: instance
            .contributors
            .iter()
            .map(|(_, heading)| heading.clone())
            .collect(),
        subjects: instance
            .subjects
            .iter()
            .map(|(_, heading)| heading.clone())
            .collect(),
        publisher: instance.publisher.clone(),
        summary: instance.summary.clone(),
        notes: instance
            .notes
            .iter()
            .map(|(_, text)| text.clone())
            .chain(instance.typed_notes.iter().cloned())
            .collect(),
        date: instance.date.clone(),
        material: instance.material,
        classification: instance.classification.clone(),
        identifiers: instance
            .identifiers
            .iter()
            .map(|(kind, value)| normalized_identifier(*kind, value))
            .collect(),
        call_numbers: distinct(items.iter().filter_map(|item| item.call_number.as_deref())),
        // The shelving place's id: stage 2.10 turns it into a facet with its name.
        locations: items
            .iter()
            .filter_map(|item| item.location.map(|id| id.0.to_string()))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
        barcodes: items
            .iter()
            .filter_map(|item| item.barcode.clone())
            .collect(),
        inventory_numbers: distinct(
            items
                .iter()
                .filter_map(|item| item.inventory_number.as_deref()),
        ),
        languages: instance.languages.clone(),
        branches: items
            .iter()
            .map(|item| item.home_branch.0.to_string())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
        genres: instance.genres.clone(),
        audience: instance.audience.clone(),
        year: instance.date.as_deref().and_then(year_of),
        opac_hidden: instance.opac_hidden,
    }
}

/// The first run of four digits in `date` that is a year from 1000 to 2999, so
/// `c1998`, `[2001?]` and `1987-1990` give `1998`, `2001` and `1987`.
#[must_use]
pub fn year_of(date: &str) -> Option<String> {
    let bytes = date.as_bytes();
    let mut start = 0;
    while start < bytes.len() {
        let run = bytes
            .get(start..)
            .unwrap_or_default()
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count();
        if run == 4 && matches!(bytes.get(start), Some(b'1' | b'2')) {
            return date.get(start..start + 4).map(str::to_owned);
        }
        start += run.max(1);
    }
    None
}

/// ISBN and ISSN lose hyphens and spaces and get an upper-case `X`, so `85-7232-040-x` and
/// `857232040X` meet; other identifiers are kept as they are.
#[must_use]
pub fn normalized_identifier(kind: IdentifierKind, value: &str) -> String {
    match kind {
        IdentifierKind::Isbn | IdentifierKind::Issn => value
            .chars()
            .filter(|c| !matches!(c, '-' | ' '))
            .map(|c| c.to_ascii_uppercase())
            .collect(),
        _ => value.to_owned(),
    }
}

fn distinct<'a>(values: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    values
        .filter(|value| seen.insert(*value))
        .map(str::to_owned)
        .collect()
}

/// One change to the index, committed together with the outbox position it reaches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexBatch {
    /// Documents to add or replace (by Instance).
    pub upserts: Vec<SearchDocument>,
    /// Instances whose documents go away.
    pub removals: Vec<InstanceId>,
    /// The last outbox id this batch covers; saved with the commit.
    pub cursor: i64,
}

/// A way to narrow the listing (docs/specs/catalogo/facetas.md, item 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Facet {
    /// The home branch of an Item.
    Branch,
    /// The content (`bf:content`).
    Content,
    /// The carrier (`bf:carrier`).
    Carrier,
    /// A genre or form of the Work.
    Genre,
    /// The Work's target audience.
    Audience,
    /// A language of the Work.
    Language,
    /// A subject of the Work.
    Subject,
    /// The year of publication.
    Year,
}

impl Facet {
    /// Every facet, in the order the listing shows them.
    pub const ALL: [Self; 8] = [
        Self::Branch,
        Self::Content,
        Self::Carrier,
        Self::Genre,
        Self::Audience,
        Self::Language,
        Self::Subject,
        Self::Year,
    ];

    /// The stable name, used in URLs.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Branch => "branch",
            Self::Content => "content",
            Self::Carrier => "carrier",
            Self::Genre => "genre",
            Self::Audience => "audience",
            Self::Language => "language",
            Self::Subject => "subject",
            Self::Year => "year",
        }
    }

    /// The facet called `name`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|facet| facet.name() == name)
    }
}

/// Values a facet returns at most (item 3).
pub const FACET_VALUES: usize = 20;

/// Filters a request keeps at most (item 4).
pub const MAX_FILTERS: usize = 16;

/// Longest filter value kept, in characters (item 4).
pub const MAX_FILTER_CHARS: usize = 200;

/// One filter: a facet and a value, compared whole.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Filter {
    /// The facet.
    pub facet: Facet,
    /// The value, as the index holds it.
    pub value: String,
}

impl Filter {
    /// The filters of `pairs` (facet name, value) that are known and fit: unknown facets,
    /// empty or too long values and those past [`MAX_FILTERS`] are left out, and repeats
    /// are kept once.
    pub fn parse<'a>(pairs: impl IntoIterator<Item = (&'a str, &'a str)>) -> Vec<Self> {
        let mut filters: Vec<Self> = Vec::new();
        for (name, value) in pairs {
            let value = value.trim();
            let Some(facet) = Facet::from_name(name) else {
                continue;
            };
            if value.is_empty() || value.chars().count() > MAX_FILTER_CHARS {
                continue;
            }
            let filter = Self {
                facet,
                value: value.to_owned(),
            };
            if !filters.contains(&filter) {
                filters.push(filter);
            }
            if filters.len() == MAX_FILTERS {
                break;
            }
        }
        filters
    }
}

/// What to look for: words typed and filters; with no words, everything that passes the
/// filters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchQuery<'a> {
    /// What a person typed.
    pub text: &'a str,
    /// Filters, all of which must hold.
    pub filters: &'a [Filter],
}

impl<'a> SearchQuery<'a> {
    /// Words alone, without filters.
    #[must_use]
    pub const fn words(text: &'a str) -> Self {
        Self { text, filters: &[] }
    }
}

/// One value of a facet and the families that have it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FacetValue {
    /// The value, as the index holds it.
    pub value: String,
    /// Families with at least one matching Instance that has it.
    pub families: usize,
}

/// The facets of what a query matches, counted by family (item 2).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FacetCounts {
    /// Families matching in all.
    pub families: usize,
    /// The values of each facet that has any, in [`Facet::ALL`] order.
    pub facets: Vec<(Facet, Vec<FacetValue>)>,
}

/// One search result.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    /// The Instance found.
    pub instance: InstanceId,
    /// Its Work.
    pub work: WorkId,
    /// Its Hub, if any.
    pub hub: Option<HubId>,
    /// Relevance; higher is better.
    pub score: f32,
    /// The Instance's title, as stored in the index.
    pub title: String,
    /// The first contributor.
    pub main_contributor: Option<String>,
    /// Date of publication.
    pub date: Option<String>,
    /// The content and carrier its description gives.
    pub material: Material,
    /// The first call number of its Items.
    pub call_number: Option<String>,
}

/// One page of results.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchResults {
    /// How many Instances match in all.
    pub total: usize,
    /// The requested page, best first.
    pub hits: Vec<SearchHit>,
}

/// Port: the full-text index.
pub trait SearchIndex: Send + Sync {
    /// The last outbox id already in the index (0 for a new index).
    ///
    /// # Errors
    ///
    /// When the index cannot be read.
    fn cursor(&self) -> Result<i64, SearchError>;

    /// Applies `batch` and commits it with its cursor, atomically: after a crash the index
    /// holds either the whole batch and its cursor, or neither.
    ///
    /// # Errors
    ///
    /// When the index cannot be written.
    fn apply(&self, batch: IndexBatch) -> Result<(), SearchError>;

    /// The Instances matching `query`, best first, skipping `offset` and returning at most
    /// `limit`, with the total count. With no words, every Instance that passes the
    /// filters matches, in Instance order.
    ///
    /// # Errors
    ///
    /// When the index cannot be read.
    fn search(
        &self,
        query: &SearchQuery<'_>,
        scope: SearchScope,
        offset: usize,
        limit: usize,
    ) -> Result<SearchResults, SearchError>;

    /// The facets of what `query` matches, counted by family, at most [`FACET_VALUES`]
    /// values each besides the filters of `query`.
    ///
    /// # Errors
    ///
    /// When the index cannot be read.
    fn facets(
        &self,
        query: &SearchQuery<'_>,
        scope: SearchScope,
    ) -> Result<FacetCounts, SearchError>;
}

/// The index failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("search index: {0}")]
pub struct SearchError(pub String);

/// Settings of one indexing run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexOptions {
    /// How documents are built.
    pub parallelism: Parallelism,
    /// Outbox effects read and committed together.
    pub batch: NonZeroUsize,
}

impl Default for IndexOptions {
    fn default() -> Self {
        Self {
            parallelism: Parallelism::Serial,
            batch: NonZeroUsize::new(1024).unwrap_or(NonZeroUsize::MIN),
        }
    }
}

/// What an indexing run did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IndexReport {
    /// Outbox effects read.
    pub effects: u64,
    /// Documents written.
    pub indexed: u64,
    /// Documents removed because their Instance no longer exists.
    pub removed: u64,
}

/// Why indexing stopped. What was committed before stays, with its cursor.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IndexError {
    /// The outbox could not be read.
    #[error(transparent)]
    Effects(#[from] EffectLogError),
    /// The catalog could not be read.
    #[error(transparent)]
    Catalog(#[from] CatalogReadError),
    /// The index failed.
    #[error(transparent)]
    Index(#[from] SearchError),
}

/// Brings the index up to the end of the outbox.
///
/// Effects are read in outbox order, from the cursor saved in the index. Each batch reads
/// its Instances (in parallel when `options` say so), builds their documents and commits
/// them with the new cursor. Running it twice is harmless: the second run finds nothing.
///
/// # Errors
///
/// [`IndexError`] when the outbox, the catalog or the index fails.
pub fn index_pending<L, C, I>(
    log: &L,
    catalog: &C,
    index: &I,
    options: &IndexOptions,
) -> Result<IndexReport, IndexError>
where
    L: EffectLog + ?Sized,
    C: CatalogReader + ?Sized,
    I: SearchIndex + ?Sized,
{
    index_pending_until(log, catalog, index, options, || false)
}

/// Indexes pending effects until the outbox is empty or `should_stop` returns true.
/// The check runs before each batch. Cancellation leaves completed batches and their
/// cursors committed and returns their report.
///
/// # Errors
///
/// [`IndexError`] when the outbox, the catalog or the index fails.
pub fn index_pending_until<L, C, I>(
    log: &L,
    catalog: &C,
    index: &I,
    options: &IndexOptions,
    mut should_stop: impl FnMut() -> bool,
) -> Result<IndexReport, IndexError>
where
    L: EffectLog + ?Sized,
    C: CatalogReader + ?Sized,
    I: SearchIndex + ?Sized,
{
    let mut report = IndexReport::default();
    while !should_stop() {
        let cursor = index.cursor()?;
        let effects = log.after(cursor, options.batch.get())?;
        let Some(last) = effects.last().map(|logged| logged.id) else {
            break;
        };
        report.effects = report.effects.saturating_add(len(effects.len()));
        let mut seen = BTreeSet::new();
        let instances: Vec<InstanceId> = effects
            .iter()
            .filter_map(|logged| match logged.effect {
                Effect::ReindexInstance(instance) => Some(instance),
                Effect::Other => None,
            })
            .filter(|instance| seen.insert(*instance))
            .collect();
        let built = options
            .parallelism
            .map(instances, |instance| build(catalog, instance));
        let mut batch = IndexBatch {
            upserts: Vec::new(),
            removals: Vec::new(),
            cursor: last,
        };
        for (instance, document) in built {
            match document? {
                Some(document) => batch.upserts.push(document),
                None => batch.removals.push(instance),
            }
        }
        report.indexed = report.indexed.saturating_add(len(batch.upserts.len()));
        report.removed = report.removed.saturating_add(len(batch.removals.len()));
        index.apply(batch)?;
    }
    Ok(report)
}

type Built = (InstanceId, Result<Option<SearchDocument>, CatalogReadError>);

fn build<C: CatalogReader + ?Sized>(catalog: &C, instance: InstanceId) -> Built {
    let document = catalog.instance(instance).and_then(|view| {
        view.map(|view| Ok(document_of(&view, &catalog.items(instance)?)))
            .transpose()
    });
    (instance, document)
}

fn len(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    /// An outbox, a catalog holding only `existing`, and an index that records batches.
    struct Fake {
        effects: Vec<LoggedEffect>,
        existing: Vec<InstanceId>,
        batches: Mutex<Vec<IndexBatch>>,
    }

    impl EffectLog for Fake {
        fn after(&self, cursor: i64, limit: usize) -> Result<Vec<LoggedEffect>, EffectLogError> {
            Ok(self
                .effects
                .iter()
                .filter(|logged| logged.id > cursor)
                .take(limit)
                .copied()
                .collect())
        }
    }

    impl CatalogReader for Fake {
        fn instance(&self, id: InstanceId) -> Result<Option<InstanceView>, CatalogReadError> {
            Ok(self.existing.contains(&id).then(|| InstanceView {
                id,
                version: 1,
                work: WorkId(id.0),
                work_version: 1,
                work_title: format!("Work {}", id.0),
                hub: None,
                material: Material::default(),
                relations: Vec::new(),
                title: format!("Title {}", id.0),
                other_titles: Vec::new(),
                typed_notes: Vec::new(),
                publisher: None,
                date: None,
                classification: None,
                summary: None,
                audience: None,
                genres: Vec::new(),
                contributors: Vec::new(),
                subjects: Vec::new(),
                identifiers: Vec::new(),
                notes: Vec::new(),
                languages: Vec::new(),
                opac_hidden: false,
            }))
        }

        fn items(&self, _: InstanceId) -> Result<Vec<ItemView>, CatalogReadError> {
            Ok(Vec::new())
        }

        fn record_version(&self, _: InstanceId) -> Result<Option<i64>, CatalogReadError> {
            Ok(None)
        }

        fn item_statuses(
            &self,
            _: &[InstanceId],
        ) -> Result<Vec<(InstanceId, crate::catalog::ItemState)>, CatalogReadError> {
            Ok(Vec::new())
        }

        fn codes(&self) -> Result<crate::catalog::CodeLists, CatalogReadError> {
            Ok(crate::catalog::CodeLists::default())
        }

        fn recent_instances(
            &self,
            _: usize,
        ) -> Result<Vec<crate::catalog::RecentInstance>, CatalogReadError> {
            Ok(Vec::new())
        }
    }

    impl SearchIndex for Fake {
        fn cursor(&self) -> Result<i64, SearchError> {
            Ok(self
                .batches
                .lock()
                .map_err(|e| SearchError(e.to_string()))?
                .last()
                .map_or(0, |batch| batch.cursor))
        }

        fn apply(&self, batch: IndexBatch) -> Result<(), SearchError> {
            self.batches
                .lock()
                .map_err(|e| SearchError(e.to_string()))?
                .push(batch);
            Ok(())
        }

        fn search(
            &self,
            _: &SearchQuery<'_>,
            _: SearchScope,
            _: usize,
            _: usize,
        ) -> Result<SearchResults, SearchError> {
            Ok(SearchResults {
                total: 0,
                hits: Vec::new(),
            })
        }

        fn facets(&self, _: &SearchQuery<'_>, _: SearchScope) -> Result<FacetCounts, SearchError> {
            Ok(FacetCounts::default())
        }
    }

    fn reindex(id: i64, instance: i64) -> LoggedEffect {
        LoggedEffect {
            id,
            effect: Effect::ReindexInstance(InstanceId(instance)),
        }
    }

    #[test]
    fn batches_follow_the_outbox_once_per_instance() {
        let fake = Fake {
            effects: vec![
                reindex(1, 10),
                LoggedEffect {
                    id: 2,
                    effect: Effect::Other,
                },
                reindex(3, 10),
                reindex(4, 11),
                reindex(7, 12),
            ],
            existing: vec![InstanceId(10), InstanceId(12)],
            batches: Mutex::new(Vec::new()),
        };
        let options = IndexOptions {
            parallelism: Parallelism::Serial,
            batch: NonZeroUsize::new(3).unwrap_or(NonZeroUsize::MIN),
        };
        let report = index_pending(&fake, &fake, &fake, &options).unwrap_or_default();
        assert_eq!(
            report,
            IndexReport {
                effects: 5,
                indexed: 2,
                removed: 1
            }
        );
        let batches = fake.batches.into_inner().unwrap_or_default();
        let summary: Vec<_> = batches
            .iter()
            .map(|batch| {
                (
                    batch.cursor,
                    batch
                        .upserts
                        .iter()
                        .map(|d| d.instance.0)
                        .collect::<Vec<_>>(),
                    batch.removals.iter().map(|i| i.0).collect::<Vec<_>>(),
                )
            })
            .collect();
        // First batch: effects 1-3 (Instance 10 twice, built once); then 4 and 7.
        assert_eq!(summary, [(3, vec![10], vec![]), (7, vec![12], vec![11])]);
    }

    #[test]
    fn the_year_is_the_first_four_digits_that_make_one() {
        assert_eq!(year_of("c1998").as_deref(), Some("1998"));
        assert_eq!(year_of("[2001?]").as_deref(), Some("2001"));
        assert_eq!(year_of("1987-1990").as_deref(), Some("1987"));
        assert_eq!(year_of("19--"), None);
        assert_eq!(year_of("12345 3021 0999"), None);
        assert_eq!(year_of("n. 345, 2010").as_deref(), Some("2010"));
        assert_eq!(year_of(""), None);
    }

    #[test]
    fn filters_keep_known_facets_and_fitting_values_once() {
        let long = "x".repeat(MAX_FILTER_CHARS + 1);
        let filters = Filter::parse([
            ("language", " por "),
            ("itemtype", "BK"),
            ("language", "por"),
            ("year", ""),
            ("subject", long.as_str()),
            ("year", "1998"),
        ]);
        assert_eq!(
            filters,
            [
                Filter {
                    facet: Facet::Language,
                    value: "por".to_owned()
                },
                Filter {
                    facet: Facet::Year,
                    value: "1998".to_owned()
                },
            ]
        );
        let many: Vec<String> = (0..40).map(|n| n.to_string()).collect();
        let pairs = many.iter().map(|value| ("year", value.as_str()));
        assert_eq!(Filter::parse(pairs).len(), MAX_FILTERS);
        for facet in Facet::ALL {
            assert_eq!(Facet::from_name(facet.name()), Some(facet));
        }
    }

    #[test]
    fn isbn_and_issn_are_compared_without_hyphens() {
        assert_eq!(
            normalized_identifier(IdentifierKind::Isbn, "85-7232-040-x"),
            "857232040X"
        );
        assert_eq!(
            normalized_identifier(IdentifierKind::Issn, "0378-5955"),
            "03785955"
        );
        assert_eq!(
            normalized_identifier(IdentifierKind::Lccn, "2001-123"),
            "2001-123"
        );
    }
}
