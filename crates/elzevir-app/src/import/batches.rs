// SPDX-License-Identifier: GPL-3.0-or-later
//! The application boundary for prepared imports.

use std::io::Read;

use serde::{Deserialize, Serialize};

use super::edits::{Recipe, RenderReport};
use super::preview::PreviewReport;
use crate::catalog::CodeLists;
use crate::import_detection::ImportFormat;
use crate::staff::StaffId;

/// Largest accepted recipe and stored manifest, in bytes.
pub const MAX_BATCH_MANIFEST_BYTES: u64 = 1024 * 1024;

/// Durable state of a private batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchState {
    /// The source has not been captured completely.
    Preparing,
    /// The revision needs review.
    Review,
    /// This revision passed preparation.
    Ready,
    /// Confirmation began and may have stored records.
    Confirming,
    /// Only cleanup can remain.
    Closed,
}

/// Metadata safe to show to the batch owner.
pub struct BatchSummary {
    /// Opaque batch identity.
    pub id: String,
    /// Current recipe revision.
    pub revision: u64,
    /// Current state.
    pub state: BatchState,
    /// Format detected at capture.
    pub format: String,
    /// Format explicitly selected by the operator.
    pub selected: Option<ImportFormat>,
}

/// Results of applying the recipe and inspecting a catalog snapshot.
pub struct BatchPreview {
    /// Original positions and edits.
    pub rendered: RenderReport,
    /// Validation and matching predictions.
    pub report: PreviewReport,
    /// The registries read with the preview, to show names in place of identifiers
    /// and edge codes. `None` when the catalog could not be read.
    pub codes: Option<CodeLists>,
}

/// The operator's prepared revision.
pub struct BatchView {
    /// Batch metadata.
    pub batch: BatchSummary,
    /// Saved edit and matching choices.
    pub recipe: Recipe,
    /// Validated archive leaves and worksheet names, when applicable.
    pub archive: Option<super::containers::Inventory>,
    /// Preview when requested.
    pub preview: Option<BatchPreview>,
}

/// The location of a refusal in the original file.
#[derive(Serialize)]
pub struct Refusal {
    /// One-based record position.
    pub record: u64,
    /// Original byte offset.
    pub offset: u64,
    /// One-based item position, if the refusal affects an item.
    pub item: Option<usize>,
    /// Native item validation detail, with the exact barcode when supplied.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Actual counts from the writer and locations of refusals, without source values.
#[derive(Serialize)]
pub struct Receipt {
    /// Source records read.
    pub read: u64,
    /// Records prepared.
    pub prepared: u64,
    /// Records explicitly excluded.
    pub excluded: u64,
    /// Records stored.
    pub stored: u64,
    /// Records matched.
    pub matched: usize,
    /// Records replaced.
    pub replaced: u64,
    /// Records skipped.
    pub skipped: u64,
    /// Records refused.
    pub failed: usize,
    /// Items added.
    pub items_stored: u64,
    /// Items replaced.
    pub items_replaced: u64,
    /// Items refused.
    pub items_refused: usize,
}

/// What is known about a completion.
pub struct Completion {
    /// Counts, when the writer returned a report.
    pub receipt: Option<Receipt>,
    /// Refusal locations in original order.
    pub refusals: Vec<Refusal>,
    /// Cleanup or interruption error, if the batch was retained.
    pub error: Option<String>,
}

/// How an edit changes the source format selection.
pub enum FormatChoice {
    /// Keep the current selection.
    Keep,
    /// Detect from the immutable source again.
    Detect,
    /// Validate the source with the selected reader during preparation.
    Use(ImportFormat),
}

/// Change to, or read of, the current institution's staged imports.
pub enum BatchCommand {
    /// List only this operator's batches.
    List,
    /// Capture the source through the size-limited staging path.
    Stage(Box<dyn Read + Send>, Option<ImportFormat>),
    /// Read a batch and optionally show a page of its preview.
    View(String, Option<u64>),
    /// Replace the recipe if the revision is current.
    Edit(String, u64, Recipe, FormatChoice),
    /// Validate the whole batch and stamp the revision if successful.
    Preview(String, Option<u64>, Option<u64>),
    /// Confirm a previously reviewed revision.
    Confirm(String, u64),
    /// Remove a batch, explicitly acknowledging an interrupted confirmation when applicable.
    Cancel(String, Option<u64>, bool),
}

/// Result of the requested operation.
pub enum BatchReply {
    /// Visible batch summaries.
    List(Vec<BatchSummary>),
    /// The prepared revision, possibly including a preview.
    Batch(Box<BatchView>),
    /// The confirmation status.
    Completed(Completion),
    /// The batch was removed explicitly.
    Cancelled,
}

/// A request that could not proceed.
#[derive(Debug, thiserror::Error)]
pub enum BatchError {
    /// Missing or inaccessible batch.
    #[error("{0}")]
    Missing(String),
    /// The current state does not allow this action.
    #[error("{0}")]
    Conflict(String),
    /// Invalid source or recipe.
    #[error("{0}")]
    Invalid(String),
    /// The service could not perform the operation.
    #[error("{0}")]
    Unavailable(String),
}

/// A prepared-import service bound to one institution.
pub trait BatchPort: Send + Sync {
    /// Runs a read or action as the authenticated operator. This is blocking work.
    ///
    /// # Errors
    /// Refuses unavailable, inaccessible, invalid or stale requests.
    fn execute(&self, operator: StaffId, command: BatchCommand) -> Result<BatchReply, BatchError>;
}
