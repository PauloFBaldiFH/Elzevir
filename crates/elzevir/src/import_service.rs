// SPDX-License-Identifier: GPL-3.0-or-later
//! Shared prepared import operations for the local CLI and the running institution.

use std::fs::File;
use std::io::{BufRead, BufReader, Cursor, Seek};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use elzevir_app::catalog::{CatalogReader, CatalogRefusal, CodeLists};
use elzevir_app::import::batches::{
    BatchCommand, BatchError, BatchPort, BatchPreview, BatchRefusal, BatchReply,
    BatchState as State, BatchView, Completion, FormatChoice, Receipt, Refusal, TableSource,
};
use elzevir_app::import::containers::{self, Selected};
use elzevir_app::import::edits::{Recipe, RenderReport};
use elzevir_app::import::preview::PreviewError;
use elzevir_app::import::sheet::{self, PreparedTable, SheetError, TableSettings};
use elzevir_app::import::table::{Delimiter, TableReader};
use elzevir_app::import::{
    ImportError, ImportOptions, ImportReport, RecordFailure, RecordRejected, RecordSink,
    import_marc21,
};
use elzevir_app::import_detection::{Evidence, ImportFormat, table_delimiter};
use elzevir_app::koha_types::TypeTranslationSource;
use elzevir_app::staff::{StaffId, StaffIdentity};
use elzevir_store::ElzevirStore;

use crate::import_batches::{Batch, Batches, is_delimited};

pub(crate) struct WebImports {
    pub(crate) data_dir: PathBuf,
    pub(crate) store: Arc<ElzevirStore>,
}

impl BatchPort for WebImports {
    fn execute(
        &self,
        actor: &StaffIdentity,
        command: BatchCommand,
    ) -> Result<BatchReply, BatchError> {
        let operator = actor.id;
        let id = u64::try_from(operator.0)
            .map_err(|_| BatchError::Missing("invalid import operator".into()))?;
        execute(
            &self.data_dir,
            id,
            Some((&self.store, operator)),
            actor.can_inventory_sources(),
            command,
        )
    }
}

type LiveStore<'a> = Option<(&'a ElzevirStore, StaffId)>;

pub(crate) fn execute(
    data_dir: &Path,
    operator: u64,
    live: LiveStore<'_>,
    mixed_access: bool,
    command: BatchCommand,
) -> Result<BatchReply, BatchError> {
    if crate::restore_journal::pending(data_dir) {
        return Err(BatchError::Unavailable(
            "interrupted restore requires recovery before managing import batches".into(),
        ));
    }
    let create = matches!(
        command,
        BatchCommand::Stage(..) | BatchCommand::StageSet(..) | BatchCommand::List
    );
    let batches = Batches::open(data_dir, operator, create).map_err(BatchError::Unavailable)?;
    let load = |id: &str| {
        let batch = batches.load(id).map_err(BatchError::Missing)?;
        if !mixed_access && batch.is_source_set() {
            return Err(BatchError::Missing("inaccessible batch".into()));
        }
        Ok(batch)
    };
    match command {
        BatchCommand::List => Ok(BatchReply::List(
            batches
                .list()
                .map_err(BatchError::Unavailable)?
                .into_iter()
                .filter(|batch| mixed_access || batch.format != "SourceSet")
                .collect(),
        )),
        BatchCommand::StageSet(origin, inputs) => {
            if !mixed_access {
                return Err(BatchError::Missing("inaccessible batch".into()));
            }
            Ok(view(
                &batches,
                batches
                    .stage_set(origin, inputs)
                    .map_err(BatchError::Invalid)?,
                None,
            ))
        }
        BatchCommand::ArchiveSet(parent, revision, origin, positions) => {
            if !mixed_access {
                return Err(BatchError::Missing("inaccessible batch".into()));
            }
            let batch = load(&parent)?;
            check_revision(&batch, revision)?;
            batch.check_review().map_err(BatchError::Conflict)?;
            let child = batches
                .stage_archive_set(&batch, origin, &positions)
                .map_err(BatchError::Invalid)?;
            Ok(view(&batches, child, None))
        }
        BatchCommand::Stage(input, selected) => Ok(view(
            &batches,
            batches
                .stage(input, selected)
                .map_err(BatchError::Invalid)?,
            None,
        )),
        BatchCommand::Relations(id, revision, selection, from) => {
            let batch = load(&id)?;
            check_revision(&batch, revision)?;
            batches
                .relations(batch, selection, from)
                .map(BatchReply::Relations)
                .map_err(BatchError::Invalid)
        }
        BatchCommand::View(id, from) => {
            let batch = load(&id)?;
            let preview = if let Some(from) = from.filter(|_| batch.check_review().is_ok()) {
                Some(prepare(data_dir, live, &batches, &batch, Some(from))?.preview)
            } else {
                None
            };
            Ok(view(&batches, batch, preview))
        }
        BatchCommand::Edit(id, revision, recipe, format) => {
            edit(&batches, load(&id)?, revision, recipe, format)
        }
        BatchCommand::Preview(id, revision, from) => {
            preview(data_dir, live, &batches, load(&id)?, revision, from)
        }
        BatchCommand::Confirm(id, revision) => {
            let batch = load(&id)?;
            check_revision(&batch, revision)?;
            confirm(data_dir, live, &batches, batch).map(BatchReply::Completed)
        }
        BatchCommand::Cancel(id, revision, acknowledge) => {
            cancel(&batches, load(&id)?, revision, acknowledge)
        }
    }
}

fn cancel(
    batches: &Batches,
    mut batch: Batch,
    revision: Option<u64>,
    acknowledge: bool,
) -> Result<BatchReply, BatchError> {
    if let Some(revision) = revision {
        check_revision(&batch, revision)?;
    }
    if batch.state == State::Confirming && !acknowledge {
        return Err(BatchError::Conflict(
            "interrupted confirmation: check the catalog and acknowledge before cancelling".into(),
        ));
    }
    close(&mut batch);
    batches
        .save(&batch)
        .and_then(|()| batches.clean(&batch))
        .map_err(BatchError::Unavailable)?;
    Ok(BatchReply::Cancelled)
}

fn edit(
    batches: &Batches,
    mut batch: Batch,
    revision: u64,
    recipe: Recipe,
    format: FormatChoice,
) -> Result<BatchReply, BatchError> {
    check_revision(&batch, revision)?;
    batch.check_review().map_err(BatchError::Conflict)?;
    recipe
        .validate()
        .map_err(|error| BatchError::Invalid(error.to_string()))?;
    let selection = match format {
        FormatChoice::Keep => Ok(()),
        FormatChoice::Detect => batches.choose_format(&mut batch, None),
        FormatChoice::Use(format) => batches.choose_format(&mut batch, Some(format)),
    };
    selection.map_err(BatchError::Invalid)?;
    if batch.archive.is_none()
        && (recipe.archive.is_some()
            || (batch.is_table() && (!recipe.edits.is_empty() || recipe.biblivre.is_some()))
            || (!batch.is_table() && recipe.table.is_some()))
    {
        return Err(BatchError::Refused(BatchRefusal::IncompatibleRecipe));
    }
    batch.recipe = recipe;
    batch.revision = batch
        .revision
        .checked_add(1)
        .ok_or_else(|| BatchError::Conflict("batch revision exhausted".into()))?;
    batch.preview_sha256 = None;
    batch.state = State::Review;
    batches.save(&batch).map_err(BatchError::Unavailable)?;
    Ok(view(batches, batch, None))
}

fn preview(
    data_dir: &Path,
    live: LiveStore<'_>,
    batches: &Batches,
    mut batch: Batch,
    revision: Option<u64>,
    from: Option<u64>,
) -> Result<BatchReply, BatchError> {
    if let Some(revision) = revision {
        check_revision(&batch, revision)?;
    }
    if let Some(set) = &batch.sources {
        batches
            .verify_set(&batch, set)
            .map_err(BatchError::Invalid)?;
        return Ok(view(batches, batch, None));
    }
    batch.check_review().map_err(BatchError::Conflict)?;
    let Prepared { preview, stamp, .. } = prepare(data_dir, live, batches, &batch, from)?;
    let ready = preview.report.issues.is_empty();
    batch.state = if ready { State::Ready } else { State::Review };
    batch.preview_sha256 = ready.then_some(stamp);
    batches.save(&batch).map_err(BatchError::Unavailable)?;
    Ok(view(batches, batch, Some(preview)))
}

fn view(batches: &Batches, batch: Batch, preview: Option<BatchPreview>) -> BatchReply {
    let table = table_source(batches, &batch);
    BatchReply::Batch(Box::new(BatchView {
        batch: batch.summary(),
        sources: batch.sources,
        recipe: batch.recipe,
        archive: batch.archive,
        table,
        preview,
    }))
}

/// Reads only the header of the selected spreadsheet, for the column mapping form.
///
/// Any failure yields `None`: the header only feeds that form, and the preview
/// reports the same failure with its location.
fn table_source(batches: &Batches, batch: &Batch) -> Option<TableSource> {
    let Some(selection) = &batch.recipe.archive else {
        if batch.archive.is_some() || !batch.is_table() {
            return None;
        }
        let settings = batch.table_settings().ok()?;
        let header = batches
            .with_source(batch, |input| first_row(input, &settings))
            .ok()?;
        return Some(TableSource { header, settings });
    };
    let source = batches.archive_source(batch).ok()?;
    match containers::select(&source, selection).ok()? {
        Selected::Workbook(rows) => {
            let header = rows.into_iter().next()?.cells;
            let settings = batch.settings_for(Some(Delimiter::Comma)).ok()?;
            Some(TableSource { header, settings })
        }
        Selected::File(bytes, detected) if is_delimited(detected, selection.format) => {
            let separator = table_delimiter(&bytes).or_else(|| {
                (detected.evidence == Evidence::EmptyInput).then_some(Delimiter::Comma)
            });
            let settings = batch.settings_for(separator).ok()?;
            let header = first_row(&mut bytes.as_slice(), &settings).ok()?;
            Some(TableSource { header, settings })
        }
        Selected::File(..) => None,
    }
}

fn first_row(input: &mut dyn BufRead, settings: &TableSettings) -> Result<Vec<String>, String> {
    TableReader::new(input, settings.delimiter, settings.encoding)
        .map_err(|error| error.to_string())?
        .next()
        .transpose()
        .map_err(|error| error.to_string())?
        .map(|row| row.cells)
        .ok_or_else(|| "empty spreadsheet".to_owned())
}

fn check_revision(batch: &Batch, revision: u64) -> Result<(), BatchError> {
    if batch.revision == revision {
        Ok(())
    } else {
        Err(BatchError::Conflict("stale batch revision".into()))
    }
}

fn close(batch: &mut Batch) {
    batch.state = State::Closed;
    batch.recipe = Recipe::default();
    batch.preview_sha256 = None;
}

fn options(batch: &Batch) -> ImportOptions {
    ImportOptions {
        biblivre: batch.recipe.biblivre.as_deref().cloned(),
        matching: batch.recipe.matching,
        origin: batch.recipe.origin.clone(),
        local_note_visibility: batch.recipe.local_note_visibility,
        ..ImportOptions::default()
    }
}

struct Prepared {
    input: PreparedInput,
    preview: BatchPreview,
    stamp: String,
}

enum PreparedInput {
    Marc(File),
    Table(PreparedTable),
}

impl PreparedInput {
    fn reserved(&self) -> Result<u64, BatchError> {
        match self {
            Self::Marc(file) => file
                .metadata()
                .map(|metadata| metadata.len())
                .map_err(|error| BatchError::Unavailable(error.to_string())),
            Self::Table(_) => Ok(0),
        }
    }

    fn submit<S: RecordSink>(
        self,
        sink: &S,
        options: &ImportOptions,
    ) -> Result<ImportReport, ImportError> {
        match self {
            Self::Marc(file) => import_marc21(BufReader::new(file), sink, options),
            Self::Table(mut table) => {
                table.apply_review(options);
                table.submit(sink, options.matching)
            }
        }
    }
}

fn prepare(
    data_dir: &Path,
    live: LiveStore<'_>,
    batches: &Batches,
    batch: &Batch,
    from: Option<u64>,
) -> Result<Prepared, BatchError> {
    if batch.archive.is_some() {
        return prepare_archive(data_dir, live, batches, batch, from);
    }
    if batch.is_table() {
        let settings = batch.table_settings().map_err(BatchError::Invalid)?;
        // The outer result is the source read; the inner one keeps the typed refusal.
        return batches
            .with_source(batch, |input| {
                Ok(prepare_table(
                    data_dir,
                    live,
                    batch,
                    from,
                    Evidence::DelimitedRows,
                    |codes| {
                        sheet::prepare(
                            input,
                            &settings,
                            codes,
                            &batch.recipe.exclude,
                            &batch.recipe.accept_missing_barcodes,
                        )
                    },
                ))
            })
            .map_err(BatchError::Invalid)?;
    }
    let rendered = batches.render(batch, from).map_err(BatchError::Invalid)?;
    prepare_marc(data_dir, live, batch, from, rendered)
}

fn prepare_archive(
    data_dir: &Path,
    live: LiveStore<'_>,
    batches: &Batches,
    batch: &Batch,
    from: Option<u64>,
) -> Result<Prepared, BatchError> {
    let recipe = &batch.recipe;
    let selection = recipe
        .archive
        .as_ref()
        .ok_or(BatchError::Refused(BatchRefusal::ChooseMember))?;
    let source = batches.archive_source(batch).map_err(BatchError::Invalid)?;
    let selected = containers::select(&source, selection)
        .map_err(|error| BatchError::Invalid(error.to_string()))?;
    drop(source);
    match selected {
        Selected::Workbook(rows) => {
            check_table_recipe(recipe)?;
            let settings = batch
                .settings_for(Some(Delimiter::Comma))
                .map_err(BatchError::Invalid)?;
            prepare_table(
                data_dir,
                live,
                batch,
                from,
                Evidence::WorkbookSheet,
                |codes| {
                    sheet::prepare_rows(
                        rows.into_iter().map(Ok),
                        &settings,
                        codes,
                        &recipe.exclude,
                        &recipe.accept_missing_barcodes,
                    )
                },
            )
        }
        Selected::File(bytes, detected) if is_delimited(detected, selection.format) => {
            check_table_recipe(recipe)?;
            let separator = table_delimiter(&bytes).or_else(|| {
                (detected.evidence == Evidence::EmptyInput).then_some(Delimiter::Comma)
            });
            let settings = batch.settings_for(separator).map_err(BatchError::Invalid)?;
            prepare_table(
                data_dir,
                live,
                batch,
                from,
                Evidence::DelimitedRows,
                |codes| {
                    sheet::prepare(
                        bytes.as_slice(),
                        &settings,
                        codes,
                        &recipe.exclude,
                        &recipe.accept_missing_barcodes,
                    )
                },
            )
        }
        Selected::File(bytes, _) => {
            if recipe.table.is_some() {
                return Err(BatchError::Refused(BatchRefusal::MappingNotForMember));
            }
            let rendered = batches
                .render_member(batch, from, Cursor::new(bytes), selection.format)
                .map_err(BatchError::Invalid)?;
            prepare_marc(data_dir, live, batch, from, rendered)
        }
    }
}

fn check_table_recipe(recipe: &Recipe) -> Result<(), BatchError> {
    if !recipe.edits.is_empty() || recipe.biblivre.is_some() {
        return Err(BatchError::Refused(BatchRefusal::EditsNotForSpreadsheet));
    }
    Ok(())
}

fn prepare_table(
    data_dir: &Path,
    live: LiveStore<'_>,
    batch: &Batch,
    from: Option<u64>,
    evidence: Evidence,
    source: impl FnOnce(&CodeLists) -> Result<PreparedTable, SheetError>,
) -> Result<Prepared, BatchError> {
    let first = from.unwrap_or_default();
    let (table, mut report) = match live {
        Some((store, _)) => store
            .readers()
            .preview_table_source(&batch.recipe, first, source),
        None => elzevir_store::preview_table_source(
            &data_dir.join("elzevir.db"),
            &batch.recipe,
            first,
            source,
        ),
    }
    .map_err(|error| match error {
        // The header needs a mapping, or the saved one no longer matches it: both
        // are fixed in the column form, which the refusal points to.
        PreviewError::Sheet(SheetError::Profile(detail)) => {
            BatchError::Refused(BatchRefusal::ColumnMapping { detail })
        }
        PreviewError::Sheet(SheetError::Cell {
            row,
            column,
            reason,
            ..
        }) => BatchError::Refused(BatchRefusal::Cell {
            row,
            column,
            reason,
        }),
        PreviewError::Sheet(SheetError::DuplicateBarcode {
            row,
            first,
            barcode,
        }) => BatchError::Refused(BatchRefusal::RepeatedBarcode {
            row,
            first,
            barcode,
        }),
        error => BatchError::Invalid(error.to_string()),
    })?;
    let stamp = Batches::table_stamp(batch, &table).map_err(BatchError::Invalid)?;
    let stamp = catalog_stamp(&stamp, &report)?;
    let mut rendered = RenderReport::for_table(&table, from);
    rendered.detection.evidence = evidence;
    rendered.review(&mut report, &batch.recipe);
    Ok(Prepared {
        input: PreparedInput::Table(table),
        preview: BatchPreview {
            rendered,
            report,
            codes: registry_names(live),
        },
        stamp,
    })
}

/// The registries the review screen shows by name. A failed read only costs the
/// names: the blocks then show identifiers and edge codes, and the preview stays valid.
fn registry_names(live: LiveStore<'_>) -> Option<CodeLists> {
    live.and_then(|(store, _)| store.codes().ok())
}

fn prepare_marc(
    data_dir: &Path,
    live: LiveStore<'_>,
    batch: &Batch,
    from: Option<u64>,
    (mut file, rendered, stamp): (File, RenderReport, String),
) -> Result<Prepared, BatchError> {
    let input = BufReader::new(&mut file);
    let mut report = match live {
        Some((store, _)) => {
            store
                .readers()
                .preview_import_page(input, options(batch), from.unwrap_or(0))
        }
        None => elzevir_store::preview_import(&data_dir.join("elzevir.db"), input, options(batch)),
    }
    .map_err(|error| BatchError::Invalid(error.to_string()))?;
    let stamp = catalog_stamp(&stamp, &report)?;
    file.rewind()
        .map_err(|error| BatchError::Unavailable(error.to_string()))?;
    rendered.review(&mut report, &batch.recipe);
    Ok(Prepared {
        input: PreparedInput::Marc(file),
        preview: BatchPreview {
            rendered,
            report,
            codes: registry_names(live),
        },
        stamp,
    })
}

fn catalog_stamp(
    source: &str,
    report: &elzevir_app::import::preview::PreviewReport,
) -> Result<String, BatchError> {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    hash.update(source.as_bytes());
    serde_json::to_writer(&mut hash, &report.scopes)
        .map_err(|error| BatchError::Invalid(error.to_string()))?;
    Ok(format!("{:x}", hash.finalize()))
}

fn confirm(
    data_dir: &Path,
    live: LiveStore<'_>,
    batches: &Batches,
    mut batch: Batch,
) -> Result<Completion, BatchError> {
    if batch.is_source_set() {
        return Err(BatchError::Conflict(
            "source sets are inventory only".into(),
        ));
    }
    if batch.state != State::Ready {
        return Err(BatchError::Conflict(format!(
            "batch is {:?} and cannot be confirmed",
            batch.state
        )));
    }
    let Prepared {
        input,
        mut preview,
        stamp,
    } = prepare(data_dir, live, batches, &batch, None)?;
    if batch.preview_sha256.as_deref() != Some(stamp.as_str()) {
        return Err(BatchError::Conflict(
            "preview identity changed; review the batch again".into(),
        ));
    }
    if !preview.report.issues.is_empty() {
        return Err(BatchError::Conflict(
            "catalog validation changed; review the batch again".into(),
        ));
    }
    let report = if preview.report.read == 0 {
        Ok(ImportReport::default())
    } else {
        let local;
        let (store, actor) = if let Some((store, actor)) = live {
            (store, Some(actor))
        } else {
            local = crate::open_store(data_dir).map_err(BatchError::Unavailable)?;
            (&local.store, None)
        };
        let mut options = options(&batch);
        options.originals = std::mem::take(&mut preview.rendered.originals);
        options.reviewed = preview
            .report
            .scopes
            .iter()
            .map(|(_, scope)| scope.clone())
            .collect();
        if matches!(input, PreparedInput::Marc(_)) {
            options.translations = store
                .type_translations()
                .map_err(|error| BatchError::Unavailable(error.to_string()))?;
        }
        let reserved = input.reserved()?;
        batch.state = State::Confirming;
        batches
            .save_reserved(&batch, reserved)
            .map_err(BatchError::Unavailable)?;
        match actor {
            Some(actor) => input.submit(&store.import_as(actor), &options),
            None => input.submit(store, &options),
        }
    };
    let report = match report {
        Ok(report) => report,
        Err(error) => {
            return Ok(Completion {
                receipt: None,
                refusals: Vec::new(),
                error: Some(format!(
                    "confirmation interrupted: {error}. Check the catalog before cancelling the batch"
                )),
            });
        }
    };
    let mut completion = outcome(&preview, &report);
    if report.failures.iter().any(|failure| {
        matches!(
            failure.failure,
            RecordFailure::Rejected(RecordRejected::Storage(_) | RecordRejected::WriterGone)
        )
    }) {
        completion.error =
            Some("confirmation interrupted; check the catalog before cancelling the batch".into());
        return Ok(completion);
    }
    close(&mut batch);
    batch.receipt = serde_json::to_value(&completion.receipt)
        .map(Some)
        .map_err(|_| BatchError::Unavailable("cannot encode receipt".into()))?;
    if let Err(error) = batches.save(&batch).and_then(|()| batches.clean(&batch)) {
        completion.error = Some(error);
    }
    Ok(completion)
}

fn outcome(preview: &BatchPreview, report: &ImportReport) -> Completion {
    let receipt = Receipt {
        read: preview.rendered.read,
        prepared: preview.report.prepared,
        excluded: preview.rendered.excluded,
        stored: report.imported,
        matched: report.matched.len(),
        replaced: report.replaced,
        skipped: report.skipped,
        failed: report.failures.len(),
        items_stored: report.items,
        items_replaced: report.replaced_items,
        items_refused: report.item_failures.len()
            + report
                .failures
                .iter()
                .filter(|failure| {
                    matches!(
                        failure.failure,
                        RecordFailure::Rejected(RecordRejected::Refused(
                            CatalogRefusal::NativeItem { .. }
                        ))
                    )
                })
                .count(),
    };
    let records = report.failures.iter().map(|failure| {
        let target = match &failure.failure {
            RecordFailure::Rejected(RecordRejected::Refused(refusal)) => native_detail(refusal),
            _ => None,
        };
        let (item, detail) = match (&failure.failure, target) {
            (_, Some((item, detail))) => (Some(item), Some(detail)),
            (
                RecordFailure::Rejected(RecordRejected::Refused(CatalogRefusal::ImportConflict(
                    reason,
                ))),
                None,
            ) => (None, Some(reason.clone())),
            _ => (None, None),
        };
        (failure.index, failure.offset, item, detail)
    });
    let items = report.item_failures.iter().map(|failure| {
        (
            failure.index,
            0,
            Some(failure.position + 1),
            native_detail(&failure.refusal).map(|(_, detail)| detail),
        )
    });
    let mut failures: Vec<_> = records.chain(items).collect();
    failures.sort_unstable();
    let refusals = failures
        .into_iter()
        .map(|(index, offset, item, detail)| {
            let (index, offset) = if preview.rendered.detection.format == ImportFormat::Delimited {
                preview
                    .rendered
                    .selected_index(index)
                    .map_or((index, offset), |selected| {
                        preview.rendered.source_position(selected as u64, offset)
                    })
            } else {
                preview.rendered.source_position(index, offset)
            };
            Refusal {
                record: index + 1,
                offset,
                item,
                detail,
            }
        })
        .collect();
    Completion {
        receipt: Some(receipt),
        refusals,
        error: None,
    }
}

fn native_detail(refusal: &CatalogRefusal) -> Option<(usize, String)> {
    if let CatalogRefusal::NativeItem { position, .. } = refusal {
        Some((*position, refusal.to_string()))
    } else {
        None
    }
}
