// SPDX-License-Identifier: GPL-3.0-or-later
//! Facet counts by family (docs/specs/catalogo/facetas.md, items 2 and 3).
//!
//! The index has one document per Instance, but a facet counts families: a Hub with its
//! Works, or a Work outside any Hub. The collector reads each matching document's family
//! and facet values from their columns and keeps the distinct (value, family) pairs; a
//! family split across segments is counted once when the segments merge.

use std::collections::{HashMap, HashSet};

use elzevir_app::search::{FACET_VALUES, Facet, FacetCounts, FacetValue, Filter};
use tantivy::collector::{Collector, SegmentCollector};
use tantivy::columnar::{Column, StrColumn};
use tantivy::{DocId, Score, SegmentOrdinal, SegmentReader};

use crate::schema::facet_field;

/// Families per value of each facet, and every family seen.
#[derive(Default)]
pub(crate) struct Families {
    all: HashSet<u64>,
    by_value: Vec<HashMap<String, HashSet<u64>>>,
}

/// Counts the families of the documents it is given, for every facet.
pub(crate) struct FacetCollector;

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
            columns.push(fast.str(facet_field(facet))?);
        }
        Ok(SegmentFacets {
            family: fast.u64("family")?,
            pairs: vec![HashSet::new(); columns.len()],
            columns,
            all: HashSet::new(),
        })
    }

    fn requires_scoring(&self) -> bool {
        false
    }

    fn merge_fruits(&self, segments: Vec<Families>) -> tantivy::Result<Families> {
        let mut merged = Families {
            all: HashSet::new(),
            by_value: vec![HashMap::new(); Facet::ALL.len()],
        };
        for segment in segments {
            merged.all.extend(segment.all);
            for (into, from) in merged.by_value.iter_mut().zip(segment.by_value) {
                for (value, families) in from {
                    into.entry(value).or_default().extend(families);
                }
            }
        }
        Ok(merged)
    }
}

/// The pairs of one segment, by term ordinal until harvest names them.
pub(crate) struct SegmentFacets {
    family: Column<u64>,
    columns: Vec<Option<StrColumn>>,
    pairs: Vec<HashSet<(u64, u64)>>,
    all: HashSet<u64>,
}

impl SegmentCollector for SegmentFacets {
    type Fruit = Families;

    fn collect(&mut self, doc: DocId, _: Score) {
        let Some(family) = self.family.first(doc) else {
            return;
        };
        self.all.insert(family);
        for (column, pairs) in self.columns.iter().zip(self.pairs.iter_mut()) {
            if let Some(column) = column {
                pairs.extend(column.term_ords(doc).map(|ord| (ord, family)));
            }
        }
    }

    fn harvest(self) -> Families {
        let mut by_value = Vec::with_capacity(self.pairs.len());
        let mut name = String::new();
        for (column, pairs) in self.columns.iter().zip(self.pairs) {
            let mut values: HashMap<String, HashSet<u64>> = HashMap::new();
            if let Some(column) = column {
                for (ord, family) in pairs {
                    // An ordinal comes from this column, so it names a term; a read error
                    // would only drop the value from the counts.
                    if column.ord_to_str(ord, &mut name).unwrap_or(false) {
                        values.entry(name.clone()).or_default().insert(family);
                    }
                }
            }
            by_value.push(values);
        }
        Families {
            all: self.all,
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
            .map(|(value, families)| FacetValue {
                value,
                families: families.len(),
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
        families: families.all.len(),
        facets,
    }
}
