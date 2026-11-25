// SPDX-License-Identifier: GPL-3.0-or-later
//! Discovery reads metadata only. Search hits are candidates until checked against SQLite.
use super::{DepositId, DigitalCollectionId, InstanceId, RepositoryError, Result};
use crate::{
    opac::{MAX_PAGE, MAX_QUERY_CHARS, PAGE_SIZE},
    search::{SearchIndex, SearchQuery, SearchScope},
};
use std::collections::BTreeMap;

/// A published deposit and its public collection memberships.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedDeposit {
    /// Stable public URL identity.
    pub id: DepositId,
    /// Collection identifiers and their display names.
    pub collections: Vec<(DigitalCollectionId, String)>,
}

/// Batch projection shared by the outbox indexer and public discovery.
pub trait DiscoveryReader: Send + Sync {
    /// Only published deposits on currently visible instances, from one snapshot.
    ///
    /// # Errors
    /// The metadata store could not read this batch.
    fn published_deposits(
        &self,
        instances: &[InstanceId],
    ) -> Result<BTreeMap<InstanceId, PublishedDeposit>>;
}

/// One public results page.
pub struct DiscoveryPage {
    /// Search text after the OPAC input limits.
    pub query: String,
    /// One-based page.
    pub page: usize,
    /// Each authorized deposit and its bibliographic title.
    pub rows: Vec<(PublishedDeposit, String)>,
    /// Whether the index has further candidates.
    pub has_next: bool,
}

/// Search public bibliographic terms and digital collection names, optionally within one collection.
///
/// # Errors
/// Searching or confirming live publication failed.
pub fn discover(
    index: &dyn SearchIndex,
    reader: &dyn DiscoveryReader,
    text: &str,
    collection: Option<DigitalCollectionId>,
    page: usize,
) -> Result<DiscoveryPage> {
    let query: String = text.trim().chars().take(MAX_QUERY_CHARS).collect();
    let page = page.clamp(1, MAX_PAGE);
    let offset = (page - 1) * PAGE_SIZE;
    let results = index
        .search(
            &SearchQuery::words(&query),
            SearchScope::Repository(collection),
            offset,
            PAGE_SIZE,
        )
        .map_err(|_| RepositoryError::Unavailable)?;
    let ids: Vec<_> = results.hits.iter().map(|hit| hit.instance).collect();
    let mut live = reader.published_deposits(&ids)?;
    let rows = results
        .hits
        .into_iter()
        .filter_map(|hit| {
            let deposit = live.remove(&hit.instance)?;
            collection
                .is_none_or(|id| deposit.collections.iter().any(|(member, _)| *member == id))
                .then_some((deposit, hit.title))
        })
        .collect();
    Ok(DiscoveryPage {
        rows,
        query,
        page,
        has_next: page < MAX_PAGE && offset + PAGE_SIZE < results.total,
    })
}
