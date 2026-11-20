// SPDX-License-Identifier: GPL-3.0-or-later
//! What waits for the circulation desk, counted for the staff home
//! (`docs/UI_BIBFRAME_INTERACTION_SPEC.md`, §22 item 14).

use jiff::Timestamp;

/// The desk's pending work. Only counts: no patron is named.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PendingCounts {
    /// Loans past their due time whose Item is not lost.
    pub overdue: u64,
    /// Holds waiting on the hold shelf.
    pub waiting_holds: u64,
    /// Notices queued to be printed.
    pub notices_to_print: u64,
}

/// Counting failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("pending counts unavailable: {0}")]
pub struct PendingError(pub String);

/// Counts the desk's pending work.
pub trait PendingReader: Send + Sync {
    /// The counts at `now`.
    ///
    /// # Errors
    /// [`PendingError`] when storage fails.
    fn pending(&self, now: Timestamp) -> Result<PendingCounts, PendingError>;
}
