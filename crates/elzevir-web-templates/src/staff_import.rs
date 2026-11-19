// SPDX-License-Identifier: GPL-3.0-or-later
//! Prepared import views, with plain-text source data escaped by Askama.

use askama::Template;

use crate::I18n;

/// The authenticated import workspace.
#[derive(Template)]
#[template(path = "staff_import.html")]
pub struct ImportPage<'a> {
    /// Translation bundle.
    pub t: &'a I18n,
    /// Session-bound form token.
    pub csrf: &'a str,
    /// Page body.
    pub view: ImportView,
    /// Message key for a failed request, when applicable.
    pub notice: Option<&'static str>,
    /// Technical detail for the operator, escaped as text.
    pub detail: Option<String>,
}

/// Contents of the import workspace.
pub enum ImportView {
    /// Upload and list of the operator's drafts.
    Home(Vec<ImportBatchLink>, Vec<String>, ImportUpload),
    /// A draft or review page.
    Draft(Box<ImportDraft>),
    /// The writer's outcome.
    Result(ImportReceipt),
    /// A refusal without a batch view.
    Error,
}

/// Source initially selected in the upload form.
#[derive(Clone, Copy)]
pub enum ImportUpload {
    /// Choose from the source evidence.
    Auto,
    /// Load a table or download a model.
    Table,
    /// Prepare a partial Koha SQL dump.
    KohaSql,
    /// Inventory compressed input or a workbook.
    Archive,
}

impl ImportUpload {
    /// Option sent by the form.
    #[must_use]
    pub const fn value(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Table => "table",
            Self::KohaSql => "sql",
            Self::Archive => "archive",
        }
    }

    /// Fluent key for the format and size guidance.
    #[must_use]
    pub const fn limit(self) -> &'static str {
        match self {
            Self::Auto => "import-limit",
            Self::Table => "import-table-limit",
            Self::KohaSql => "import-sql-limit",
            Self::Archive => "import-archive-limit",
        }
    }
}

/// One visible batch.
pub struct ImportBatchLink {
    /// Opaque batch id.
    pub id: String,
    /// Localized format and state.
    pub label: String,
}

/// Editable batch revision.
pub struct ImportDraft {
    /// Batch link.
    pub link: ImportBatchLink,
    /// Current revision.
    pub revision: u64,
    /// Saved recipe JSON or text retained after a refusal.
    pub recipe: String,
    /// Currently selected format or automatic detection.
    pub format: &'static str,
    /// Whether the recipe can be edited.
    pub editable: bool,
    /// Whether the displayed current revision can be confirmed.
    pub ready: bool,
    /// Whether cancellation requires checking the catalog.
    pub interrupted: bool,
    /// Validation of the whole batch and the selected page.
    pub preview: Option<ImportPreview>,
    /// Member paths with their detected format and worksheet names.
    pub archive: Option<Vec<ImportMember>>,
    /// Selected path and worksheet for the saved revision.
    pub archive_selection: Option<String>,
}

/// One file in a container inventory.
pub struct ImportMember {
    /// Exact JSON path for the recipe.
    pub path: String,
    /// Operator-facing detected format.
    pub format: String,
    /// Available worksheets.
    pub sheets: Vec<String>,
}

/// Review results for one page.
pub struct ImportPreview {
    /// Localized count summary.
    pub counts: String,
    /// Localized detection evidence.
    pub evidence: String,
    /// Whether the selected matching action can replace existing items.
    pub replace_items: bool,
    /// Selected source records and their prepared descriptions.
    pub records: Vec<ImportSample>,
    /// Diagnostics for this page with their destination.
    pub diagnostics: Vec<(String, String, Option<String>)>,
    /// Previous and next links for records.
    pub record_pages: (Option<String>, Option<String>),
    /// Previous and next links for diagnostics.
    pub diagnostic_pages: (Option<String>, Option<String>),
}

/// One record on the review screen.
pub struct ImportSample {
    /// One-based source position.
    pub record: u64,
    /// The prepared Work, Instance and Items.
    pub blocks: Option<RecordBlocks>,
    /// Possible stored match.
    pub candidate: Option<i64>,
    /// Original MARC.
    pub before: String,
    /// MARC after an edit.
    pub after: Option<String>,
}

/// A prepared record in the blocks of the catalog (UI spec §21.1, step 4).
pub struct RecordBlocks {
    /// The Work's filled fields, rendered with the catalog's field markup.
    pub work: String,
    /// The Instance's filled fields, rendered with the same markup.
    pub instance: String,
    /// The Items table, when the record has Items and the registries were read.
    pub items: Option<String>,
    /// How many Items the record carries.
    pub item_count: usize,
}

/// Actual write report for the operator.
pub struct ImportReceipt {
    /// Download endpoint.
    pub download: String,
    /// Label and count pairs.
    pub counts: Vec<(&'static str, u64)>,
    /// Localized refusal locations.
    pub refusals: Vec<String>,
    /// More refusal locations are available in the download.
    pub more: bool,
    /// Whether the source and recipe were removed.
    pub removed: bool,
}
