// SPDX-License-Identifier: GPL-3.0-or-later
//! Shared prepared import operations for the local CLI and the running institution.

use std::fs::File;
use std::io::{BufReader, Seek};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use elzevir_app::catalog::CatalogRefusal;
use elzevir_app::import::batches::{
    BatchCommand, BatchError, BatchPort, BatchPreview, BatchReply, BatchState as State, BatchView,
    Completion, FormatChoice, Receipt, Refusal,
};
use elzevir_app::import::edits::{Recipe, RenderReport};
use elzevir_app::import::sheet::PreparedTable;
use elzevir_app::import::{
    ImportError, ImportOptions, ImportReport, RecordFailure, RecordRejected, RecordSink,
    import_marc21,
};
use elzevir_app::import_detection::ImportFormat;
use elzevir_app::koha_types::TypeTranslationSource;
use elzevir_app::staff::StaffId;
use elzevir_store::ElzevirStore;

use crate::import_batches::{Batch, Batches};

pub(crate) struct WebImports {
    pub(crate) data_dir: PathBuf,
    pub(crate) store: Arc<ElzevirStore>,
}

impl BatchPort for WebImports {
    fn execute(&self, operator: StaffId, command: BatchCommand) -> Result<BatchReply, BatchError> {
        let id = u64::try_from(operator.0)
            .map_err(|_| BatchError::Missing("invalid import operator".into()))?;
        execute(&self.data_dir, id, Some((&self.store, operator)), command)
    }
}

type LiveStore<'a> = Option<(&'a ElzevirStore, StaffId)>;

pub(crate) fn execute(
    data_dir: &Path,
    operator: u64,
    live: LiveStore<'_>,
    command: BatchCommand,
) -> Result<BatchReply, BatchError> {
    if crate::restore_journal::pending(data_dir) {
        return Err(BatchError::Unavailable(
            "interrupted restore requires recovery before managing import batches".into(),
        ));
    }
    let create = matches!(command, BatchCommand::Stage(..) | BatchCommand::List);
    let batches = Batches::open(data_dir, operator, create).map_err(BatchError::Unavailable)?;
    let load = |id: &str| batches.load(id).map_err(BatchError::Missing);
    match command {
        BatchCommand::List => Ok(BatchReply::List(
            batches.list().map_err(BatchError::Unavailable)?,
        )),
        BatchCommand::Stage(input, selected) => Ok(view(
            batches
                .stage(input, selected)
                .map_err(BatchError::Invalid)?,
            None,
        )),
        BatchCommand::View(id, from) => {
            let batch = load(&id)?;
            let preview = if let Some(from) = from.filter(|_| batch.check_review().is_ok()) {
                Some(prepare(data_dir, live, &batches, &batch, Some(from))?.preview)
            } else {
                None
            };
            Ok(view(batch, preview))
        }
        BatchCommand::Edit(id, revision, recipe, format) => {
            let mut batch = load(&id)?;
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
            if (batch.is_table() && !recipe.edits.is_empty())
                || (!batch.is_table() && recipe.table.is_some())
            {
                return Err(BatchError::Invalid(
                    "recipe is incompatible with the selected source format".into(),
                ));
            }
            batch.recipe = recipe;
            batch.revision = batch
                .revision
                .checked_add(1)
                .ok_or_else(|| BatchError::Conflict("batch revision exhausted".into()))?;
            batch.preview_sha256 = None;
            batch.state = State::Review;
            batches.save(&batch).map_err(BatchError::Unavailable)?;
            Ok(view(batch, None))
        }
        BatchCommand::Preview(id, revision, from) => {
            let mut batch = load(&id)?;
            if let Some(revision) = revision {
                check_revision(&batch, revision)?;
            }
            batch.check_review().map_err(BatchError::Conflict)?;
            let Prepared { preview, stamp, .. } = prepare(data_dir, live, &batches, &batch, from)?;
            let ready = preview.report.issues.is_empty();
            batch.state = if ready { State::Ready } else { State::Review };
            batch.preview_sha256 = ready.then_some(stamp);
            batches.save(&batch).map_err(BatchError::Unavailable)?;
            Ok(view(batch, Some(preview)))
        }
        BatchCommand::Confirm(id, revision) => {
            let batch = load(&id)?;
            check_revision(&batch, revision)?;
            confirm(data_dir, live, &batches, batch).map(BatchReply::Completed)
        }
        BatchCommand::Cancel(id, revision, acknowledge) => {
            let mut batch = load(&id)?;
            if let Some(revision) = revision {
                check_revision(&batch, revision)?;
            }
            if batch.state == State::Confirming && !acknowledge {
                return Err(BatchError::Conflict(
                    "interrupted confirmation: check the catalog and acknowledge before cancelling"
                        .into(),
                ));
            }
            close(&mut batch);
            batches
                .save(&batch)
                .and_then(|()| batches.clean(&batch))
                .map_err(BatchError::Unavailable)?;
            Ok(BatchReply::Cancelled)
        }
    }
}

fn view(batch: Batch, preview: Option<BatchPreview>) -> BatchReply {
    BatchReply::Batch(Box::new(BatchView {
        batch: batch.summary(),
        recipe: batch.recipe,
        preview,
    }))
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
        matching: batch.recipe.matching,
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
            Self::Table(table) => table.submit(sink, options.matching),
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
    if batch.is_table() {
        let settings = batch.table_settings().map_err(BatchError::Invalid)?;
        let (table, mut report) = batches
            .with_source(batch, |input| {
                match live {
                    Some((store, _)) => store.readers().preview_sheet(
                        input,
                        &settings,
                        &batch.recipe,
                        from.unwrap_or_default(),
                    ),
                    None => elzevir_store::preview_sheet(
                        &data_dir.join("elzevir.db"),
                        input,
                        &settings,
                        &batch.recipe,
                        from.unwrap_or_default(),
                    ),
                }
                .map_err(|error| error.to_string())
            })
            .map_err(BatchError::Invalid)?;
        let stamp = Batches::table_stamp(batch, &table).map_err(BatchError::Invalid)?;
        let rendered = RenderReport::for_table(&table, from);
        rendered.review(&mut report, &batch.recipe);
        return Ok(Prepared {
            input: PreparedInput::Table(table),
            preview: BatchPreview { rendered, report },
            stamp,
        });
    }
    let (mut file, rendered, stamp) = batches.render(batch, from).map_err(BatchError::Invalid)?;
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
    file.rewind()
        .map_err(|error| BatchError::Unavailable(error.to_string()))?;
    rendered.review(&mut report, &batch.recipe);
    Ok(Prepared {
        input: PreparedInput::Marc(file),
        preview: BatchPreview { rendered, report },
        stamp,
    })
}

fn confirm(
    data_dir: &Path,
    live: LiveStore<'_>,
    batches: &Batches,
    mut batch: Batch,
) -> Result<Completion, BatchError> {
    if batch.state != State::Ready {
        return Err(BatchError::Conflict(format!(
            "batch is {:?} and cannot be confirmed",
            batch.state
        )));
    }
    let Prepared {
        input,
        preview,
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
        let (item, detail) =
            target.map_or((None, None), |(item, detail)| (Some(item), Some(detail)));
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
