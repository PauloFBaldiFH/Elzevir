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
    /// What else the operator needs to know about the failure.
    pub detail: Option<ImportDetail>,
}

/// More about a failed request, escaped as text.
pub enum ImportDetail {
    /// A translated sentence, such as the row and column of a refused cell.
    Explained(String),
    /// A located technical message from a reader, shown collapsed.
    Technical(String),
}

/// Contents of the import workspace.
pub enum ImportView {
    /// Upload and list of the operator's drafts.
    Home(Vec<ImportBatchLink>, Vec<String>, ImportUpload, bool),
    /// Scoped member identities in a private batch.
    Sources(String, u64, String, Option<String>, Vec<SourceMemberRow>),
    /// Structural counts for a private `PostgreSQL` upload.
    Postgres(Vec<PostgresTable>),
    /// A draft or review page.
    Draft(Box<ImportDraft>),
    /// The writer's outcome.
    Result(ImportReceipt),
    /// A refusal without a batch view.
    Error,
}

/// Source identity and size shown to a permitted operator.
pub struct SourceMemberRow {
    /// Position within this batch.
    pub position: usize,
    /// Logical filename.
    pub name: String,
    /// Source byte length.
    pub bytes: u64,
    /// SHA-256 of the original.
    pub sha256: String,
    /// Localized format hint.
    pub format: String,
    /// Position of the first identical original.
    pub duplicate_of: Option<usize>,
}

/// Displayable source metadata and aggregate counts.
pub struct PostgresTable {
    /// Qualified source table.
    pub name: String,
    /// COPY columns in source order.
    pub columns: String,
    /// Number of complete rows.
    pub rows: u64,
    /// Number of null fields.
    pub nulls: u64,
    /// Number of empty fields.
    pub empty: u64,
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
    /// Selectable archive members and worksheets, for an archive source.
    pub archive: Option<Vec<ImportChoice>>,
    /// Whole members available for a private source set.
    pub archive_members: Vec<(usize, String)>,
    /// Column mapping of the selected spreadsheet, when it needs one.
    pub columns: Option<ImportColumns>,
    /// Saved MARC edit rules, in application order, or `None` when the source is
    /// a spreadsheet and edit rules do not apply.
    pub rules: Option<Vec<ImportRule>>,
    /// Saved matching choices.
    pub matching: ImportMatching,
    /// Explicit Biblivre mapping when 949 holdings are available for review.
    pub biblivre: Option<BiblivreForm>,
    /// Source installation supplied by the operator.
    pub origin: String,
    /// Access policy for incoming local notes.
    pub local_notes: &'static str,
    /// One-based records excluded from the batch.
    pub excluded: Vec<u64>,
}

/// Native registries and the saved 949 interpretation shown to staff.
pub struct BiblivreForm {
    /// ID and display name of each branch.
    pub branches: Vec<(i64, String)>,
    /// ID and display name of each circulation policy.
    pub policies: Vec<(i64, String)>,
    /// Saved home branch, holding branch and policy, or zero before selection.
    pub selected: (i64, i64, i64),
    /// Saved date interpretation, or empty before selection.
    pub date: &'static str,
}

/// One saved MARC edit rule.
pub struct ImportRule {
    /// Position in the recipe, used to remove it.
    pub index: usize,
    /// One-based record, or the whole batch.
    pub record: Option<u64>,
    /// MARC tag, such as `245`.
    pub tag: String,
    /// Message key of the operation.
    pub op_key: &'static str,
    /// Subfields or text the rule writes, in the mnemonic form cataloguers type, such as `$aTitle`.
    pub detail: String,
}

/// The saved matching choices, as select values.
pub struct ImportMatching {
    /// `none`, `isbn`, `issn` or `koha`.
    pub rule: &'static str,
    /// `ignore`, `create` or `replace`.
    pub on_match: &'static str,
    /// `create` or `ignore`.
    pub on_no_match: &'static str,
    /// `always`, `matches`, `new`, `replace` or `ignore`.
    pub items: &'static str,
}

/// One selectable archive member, or one worksheet of a workbook member.
pub struct ImportChoice {
    /// Position in the inventory, as `entry` or `entry/sheet`.
    pub value: String,
    /// Member path, worksheet and format, for display.
    pub label: String,
    /// Whether the saved revision uses it.
    pub selected: bool,
}

/// The column mapping form of a custom spreadsheet.
pub struct ImportColumns {
    /// Every column of the sheet, in order.
    pub columns: Vec<ImportColumn>,
    /// Every accepted destination, offered as a list.
    pub targets: Vec<String>,
    /// `none`, `isbn` or `title`.
    pub group_by: &'static str,
}

/// One spreadsheet column and its chosen or suggested destination.
pub struct ImportColumn {
    /// One-based column.
    pub position: usize,
    /// Exact header.
    pub header: String,
    /// Saved or suggested destination, empty when there is no suggestion.
    pub target: String,
    /// Whether surrounding whitespace is trimmed.
    pub trim: bool,
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
    /// Catalog rows affected by this decision.
    pub scope: Option<String>,
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
