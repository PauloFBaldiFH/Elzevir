// SPDX-License-Identifier: GPL-3.0-or-later
//! Facet counts by family (docs/specs/catalogo/facetas.md, items 2 and 3).
//!
//! The index has one document per Instance, but a facet counts families: a Hub with its
//! Works, or a Work outside any Hub. Most families have one Instance: their values are
//! counted with a plain counter per term ordinal, with no set at all. Only the families
//! with more than one Instance ([`shared_families`], kept per searcher generation) go
//! through sets of (value, family), so each is counted once even across segments.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use elzevir_app::search::{FACET_VALUES, Facet, FacetCounts, FacetValue, Filter};
use tantivy::collector::{Collector, SegmentCollector};
use tantivy::columnar::{Column, StrColumn};
use tantivy::{DocId, Score, Searcher, SegmentOrdinal, SegmentReader};

use crate::schema::facet_field;

/// The families with more than one live Instance in the index `searcher` reads.
pub(crate) fn shared_families(searcher: &Searcher) -> tantivy::Result<HashSet<u64>> {
    let mut seen = HashSet::new();
    let mut shared = HashSet::new();
    for segment in searcher.segment_readers() {
        let family = segment.fast_fields().u64("family")?;
        for doc in segment.doc_ids_alive() {
            if let Some(family) = family.first(doc)
                && !seen.insert(family)
            {
                shared.insert(family);
            }
        }
    }
    Ok(shared)
}

/// What one value of a facet counts: families with one Instance, plus the shared ones.
#[derive(Default)]
struct Tally {
    single: usize,
    shared: HashSet<u64>,
}

/// Every facet's values, and the families seen.
pub(crate) struct Families {
    single: usize,
    shared: HashSet<u64>,
    by_value: Vec<HashMap<String, Tally>>,
}

/// Counts the families of the documents it is given, for every facet.
pub(crate) struct FacetCollector {
    /// The families with more than one Instance.
    pub(crate) shared: Arc<HashSet<u64>>,
}

impl Collector for FacetCollector {
    type Fruit = Families;
    type Child = SegmentFacets;

    fn for_segment(
        &self,
        _: SegmentOrdinal,
        segment: &SegmentReader,
    ) -> tantivy::Result<SegmentFacets> {
        let fast = segment.fast_fields();
        let mut columns = Vec::with_capacity(Facet::ALL.len());
        for facet in Facet::ALL {
            let column = fast.str(facet_field(facet))?;
            let terms = column.as_ref().map_or(0, |column| column.num_terms());
            columns.push(SegmentColumn {
                column,
                single: vec![0; terms],
                shared: HashSet::new(),
            });
        }
        Ok(SegmentFacets {
            family: fast.u64("family")?,
            shared_families: Arc::clone(&self.shared),
            columns,
            single: 0,
            shared: HashSet::new(),
        })
    }

    fn requires_scoring(&self) -> bool {
        false
    }

    fn merge_fruits(&self, segments: Vec<Families>) -> tantivy::Result<Families> {
        let mut merged = Families {
            single: 0,
            shared: HashSet::new(),
            by_value: (0..Facet::ALL.len()).map(|_| HashMap::new()).collect(),
        };
        for segment in segments {
            merged.single += segment.single;
            merged.shared.extend(segment.shared);
            for (into, from) in merged.by_value.iter_mut().zip(segment.by_value) {
                for (value, tally) in from {
                    let entry = into.entry(value).or_default();
                    entry.single += tally.single;
                    entry.shared.extend(tally.shared);
                }
            }
        }
        Ok(merged)
    }
}

/// One facet's column in a segment and what it counted, by term ordinal.
struct SegmentColumn {
    column: Option<StrColumn>,
    single: Vec<usize>,
    shared: HashSet<(u64, u64)>,
}

/// The counts of one segment, named at harvest.
pub(crate) struct SegmentFacets {
    family: Column<u64>,
    shared_families: Arc<HashSet<u64>>,
    columns: Vec<SegmentColumn>,
    single: usize,
    shared: HashSet<u64>,
}

impl SegmentCollector for SegmentFacets {
    type Fruit = Families;

    fn collect(&mut self, doc: DocId, _: Score) {
        let Some(family) = self.family.first(doc) else {
            return;
        };
        let shared = self.shared_families.contains(&family);
        if shared {
            self.shared.insert(family);
        } else {
            self.single += 1;
        }
        for column in &mut self.columns {
            let Some(values) = &column.column else {
                continue;
            };
            for ord in values.term_ords(doc) {
                if shared {
                    column.shared.insert((ord, family));
                } else if let Some(count) = usize::try_from(ord)
                    .ok()
                    .and_then(|ord| column.single.get_mut(ord))
                {
                    *count += 1;
                }
            }
        }
    }

    fn harvest(self) -> Families {
        let mut by_value = Vec::with_capacity(self.columns.len());
        let mut name = String::new();
        for column in self.columns {
            let mut values: HashMap<String, Tally> = HashMap::new();
            if let Some(terms) = &column.column {
                // Each ordinal is named once; an ordinal comes from this column, so it
                // names a term, and a read error would only drop the value.
                for (ord, count) in column.single.iter().enumerate() {
                    if *count == 0 {
                        continue;
                    }
                    if let Ok(ord) = u64::try_from(ord)
                        && terms.ord_to_str(ord, &mut name).unwrap_or(false)
                    {
                        values.entry(name.clone()).or_default().single += count;
                    }
                }
                for (ord, family) in column.shared {
                    if terms.ord_to_str(ord, &mut name).unwrap_or(false) {
                        values
                            .entry(name.clone())
                            .or_default()
                            .shared
                            .insert(family);
                    }
                }
            }
            by_value.push(values);
        }
        Families {
            single: self.single,
            shared: self.shared,
            by_value,
        }
    }
}

/// The counts to show: per facet, the [`FACET_VALUES`] values with most families (ties
/// by value), plus any value `filters` names; facets without values are left out.
pub(crate) fn counts(families: Families, filters: &[Filter]) -> FacetCounts {
    let mut facets = Vec::new();
    for (facet, values) in Facet::ALL.into_iter().zip(families.by_value) {
        let mut ranked: Vec<FacetValue> = values
            .into_iter()
            .map(|(value, tally)| FacetValue {
                value,
                families: tally.single + tally.shared.len(),
            })
            .collect();
        ranked.sort_by(|a, b| b.families.cmp(&a.families).then(a.value.cmp(&b.value)));
        let active = |value: &FacetValue| {
            filters
                .iter()
                .any(|filter| filter.facet == facet && filter.value == value.value)
        };
        let mut shown: Vec<FacetValue> = Vec::new();
        for (position, value) in ranked.into_iter().enumerate() {
            if position < FACET_VALUES || active(&value) {
                shown.push(value);
            }
        }
        if !shown.is_empty() {
            facets.push((facet, shown));
        }
    }
    FacetCounts {
        families: families.single + families.shared.len(),
        facets,
    }
}
