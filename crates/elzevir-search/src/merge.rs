// SPDX-License-Identifier: GPL-3.0-or-later
//! How the index merges its segments: as Tantivy's log policy does, but never into a
//! segment past [`SEGMENT_DOCS`] documents.
//!
//! Segments are what the search executor spreads over the cores. A filter that matches
//! most of a large catalog counts its facets over every matching document
//! (docs/specs/catalogo/facetas.md, item 12); with one segment of 800 000 documents that
//! is one core's work; in segments of 64 000 every core gets a share of about the same size.

use tantivy::index::SegmentMeta;
use tantivy::merge_policy::{LogMergePolicy, MergeCandidate, MergePolicy};

/// Most documents a merge may produce.
pub(crate) const SEGMENT_DOCS: u32 = 64_000;

/// Deleted documents, as a share of a segment, past which a full segment is rewritten
/// alone to drop them.
const DELETED_SHARE: f32 = 0.3;

/// The log policy, with each of its merges cut into merges of at most [`SEGMENT_DOCS`].
#[derive(Debug)]
pub(crate) struct CappedMergePolicy {
    log: LogMergePolicy,
}

impl Default for CappedMergePolicy {
    fn default() -> Self {
        let mut log = LogMergePolicy::default();
        log.set_max_docs_before_merge(SEGMENT_DOCS as usize);
        log.set_del_docs_ratio_before_merge(DELETED_SHARE);
        Self { log }
    }
}

impl MergePolicy for CappedMergePolicy {
    fn compute_merge_candidates(&self, segments: &[SegmentMeta]) -> Vec<MergeCandidate> {
        let docs = |id| {
            segments
                .iter()
                .find(|segment| segment.id() == id)
                .map_or(0, SegmentMeta::num_docs)
        };
        // Integers: the share is three tenths, as [`DELETED_SHARE`] tells the log policy.
        let deleted = |id| {
            segments
                .iter()
                .find(|segment| segment.id() == id)
                .is_some_and(|segment| {
                    u64::from(segment.num_deleted_docs()) * 10 > u64::from(segment.max_doc()) * 3
                })
        };
        let mut capped = Vec::new();
        for MergeCandidate(ids) in self.log.compute_merge_candidates(segments) {
            let mut chunk = Vec::new();
            let mut total = 0_u32;
            for id in ids {
                let size = docs(id);
                if !chunk.is_empty() && total.saturating_add(size) > SEGMENT_DOCS {
                    capped.push(std::mem::take(&mut chunk));
                    total = 0;
                }
                chunk.push(id);
                total = total.saturating_add(size);
            }
            capped.push(chunk);
        }
        // A lone segment is only worth rewriting to drop its deleted documents.
        capped
            .into_iter()
            .filter(|chunk| chunk.len() > 1 || chunk.first().is_some_and(|id| deleted(*id)))
            .map(MergeCandidate)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use tantivy::index::{Index, SegmentId};
    use tantivy::schema::Schema;

    use super::*;

    fn segment(index: &Index, docs: u32, deleted: u32) -> SegmentMeta {
        index
            .new_segment_meta(SegmentId::generate_random(), docs)
            .with_delete_meta(deleted, 0)
    }

    #[test]
    fn merges_stop_at_the_cap_and_a_full_segment_is_left_alone() {
        let index = Index::create_in_ram(Schema::builder().build());
        // Sixteen segments of 16 000: the log policy would merge eight at a time.
        let small: Vec<SegmentMeta> = (0..16).map(|_| segment(&index, 16_000, 0)).collect();
        let merges = CappedMergePolicy::default().compute_merge_candidates(&small);
        assert!(!merges.is_empty());
        for MergeCandidate(ids) in &merges {
            assert!(ids.len() > 1 && ids.len() * 16_000 <= SEGMENT_DOCS as usize);
        }
        // Full segments are never merged together, unless deletions make one worth
        // rewriting alone.
        let full: Vec<SegmentMeta> = (0..8)
            .map(|n| segment(&index, SEGMENT_DOCS, if n == 0 { 30_000 } else { 0 }))
            .collect();
        let merges = CappedMergePolicy::default().compute_merge_candidates(&full);
        assert_eq!(merges.len(), 1);
        assert_eq!(merges[0].0, [full[0].id()]);
    }
}
