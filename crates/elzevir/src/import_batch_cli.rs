// SPDX-License-Identifier: GPL-3.0-or-later
//! Local operator interface to staged MARC imports.

use std::fs::File;
use std::io::{BufReader, Seek};
use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use elzevir_app::import::edits::{Recipe, RenderReport};
use elzevir_app::import::preview::PreviewReport;
use elzevir_app::import::{
    ImportOptions, ImportReport, RecordFailure, RecordRejected, import_marc21,
};
use elzevir_app::koha_types::TypeTranslationSource;

use crate::import_batches::{Batch, Batches, State, read_recipe};

#[derive(Args)]
pub(crate) struct BatchArgs {
    /// Existing institution data directory.
    #[arg(long)]
    data_dir: PathBuf,
    /// Operator identity supplied by the local administrator.
    #[arg(long)]
    operator: u64,
    #[command(subcommand)]
    action: Action,
}

#[derive(Subcommand)]
enum Action {
    /// Capture the original file into a new batch.
    Stage { file: PathBuf },
    /// Save a complete recipe for an existing revision.
    Edit {
        id: String,
        revision: u64,
        recipe: PathBuf,
    },
    /// Preview the current revision and mark it ready when it passes validation.
    Preview { id: String },
    /// Explicitly confirm a previously previewed revision.
    Confirm { id: String, revision: u64 },
    /// Remove the private batch and its source.
    Cancel {
        id: String,
        /// Confirm that the catalog was checked after a possibly partial import.
        #[arg(long)]
        acknowledge_interrupted: bool,
    },
}

pub(crate) fn run(args: BatchArgs) -> Result<(), String> {
    if crate::restore_journal::pending(&args.data_dir) {
        return Err("interrupted restore requires recovery before managing import batches".into());
    }
    let batches = Batches::open(
        &args.data_dir,
        args.operator,
        matches!(args.action, Action::Stage { .. }),
    )?;
    match args.action {
        Action::Stage { file } => {
            let batch = batches.stage(&file)?;
            show_revision(&batch);
            Ok(())
        }
        Action::Edit {
            id,
            revision,
            recipe,
        } => {
            let mut batch = batches.load(&id)?;
            batch.check_review()?;
            require_revision(&batch, revision)?;
            batch.recipe = read_recipe(&recipe)?;
            batch.revision = batch
                .revision
                .checked_add(1)
                .ok_or("batch revision exhausted")?;
            batch.preview_sha256 = None;
            batch.state = State::Review;
            batches.save(&batch)?;
            show_revision(&batch);
            Ok(())
        }
        Action::Preview { id } => preview(&args.data_dir, &batches, batches.load(&id)?),
        Action::Confirm { id, revision } => {
            confirm(&args.data_dir, &batches, batches.load(&id)?, revision)
        }
        Action::Cancel {
            id,
            acknowledge_interrupted,
        } => {
            let mut batch = batches.load(&id)?;
            if batch.state == State::Confirming && !acknowledge_interrupted {
                return Err(
                    "interrupted confirmation: check the catalog and acknowledge before cancelling"
                        .into(),
                );
            }
            batch.state = State::Closed;
            batch.recipe = Recipe::default();
            batch.preview_sha256 = None;
            batches.save(&batch)?;
            batches.clean(&batch)
        }
    }
}

fn require_revision(batch: &Batch, revision: u64) -> Result<(), String> {
    if batch.revision == revision {
        return Ok(());
    }
    Err("stale batch revision".into())
}

fn show_revision(batch: &Batch) {
    println!(
        "batch {} revision {} {:?}",
        batch.id(),
        batch.revision,
        batch.state
    );
}

fn options(batch: &Batch) -> ImportOptions {
    ImportOptions {
        matching: batch.recipe.matching,
        ..ImportOptions::default()
    }
}

fn inspect(data_dir: &Path, file: &mut File, batch: &Batch) -> Result<PreviewReport, String> {
    let report = elzevir_store::preview_import(
        &data_dir.join("elzevir.db"),
        BufReader::new(&mut *file),
        options(batch),
    )
    .map_err(|error| error.to_string())?;
    file.rewind().map_err(|error| error.to_string())?;
    Ok(report)
}

fn preview(data_dir: &Path, batches: &Batches, mut batch: Batch) -> Result<(), String> {
    batch.check_review()?;
    let (mut file, render, stamp) = batches.render(&batch)?;
    let mut report = inspect(data_dir, &mut file, &batch)?;
    drop(file);
    render.review(&mut report, &batch.recipe);
    let ready = report.issues.is_empty();
    batch.state = if ready { State::Ready } else { State::Review };
    batch.preview_sha256 = ready.then_some(stamp);
    batches.save(&batch)?;
    show_revision(&batch);
    println!(
        "{} original records, {} excluded, {} edited, {} items to submit, matching {:?}",
        render.read, render.excluded, render.changed, report.items, batch.recipe.matching
    );
    for sample in render.samples {
        if sample.fields.is_empty() {
            println!("record {}: field order changed", sample.record);
        }
        for field in sample.fields {
            println!(
                "record {}, field {}: {:?} -> {:?}",
                sample.record, field.tag, field.before, field.after
            );
        }
    }
    crate::catalog_cli::print_preview(&report);
    if ready {
        Ok(())
    } else {
        Err("preview requires review".into())
    }
}

fn confirm(
    data_dir: &Path,
    batches: &Batches,
    mut batch: Batch,
    revision: u64,
) -> Result<(), String> {
    require_revision(&batch, revision)?;
    if batch.state != State::Ready {
        return Err(format!(
            "batch is {:?} and cannot be confirmed",
            batch.state
        ));
    }
    let (mut file, render, stamp) = batches.render(&batch)?;
    if batch.preview_sha256.as_deref() != Some(stamp.as_str()) {
        return Err("preview identity changed; review the batch again".into());
    }
    let mut preview = inspect(data_dir, &mut file, &batch)?;
    render.review(&mut preview, &batch.recipe);
    if !preview.issues.is_empty() {
        return Err("catalog validation changed; review the batch again".into());
    }
    let report = if preview.read == 0 {
        ImportReport::default()
    } else {
        submit(data_dir, batches, &mut batch, file)?
    };
    print_failures(&report, &render);
    let receipt = serde_json::json!({
        "read": render.read, "prepared": preview.prepared, "excluded": render.excluded,
        "stored": report.imported, "matched": report.matched.len(), "replaced": report.replaced,
        "skipped": report.skipped, "failed": report.failures.len(), "items_stored": report.items,
        "items_replaced": report.replaced_items, "items_refused": report.item_failures.len()
    });
    println!("{receipt}");
    if report.failures.iter().any(|failure| {
        matches!(
            failure.failure,
            RecordFailure::Rejected(RecordRejected::Storage(_) | RecordRejected::WriterGone)
        )
    }) {
        return Err(
            "confirmation interrupted; check the catalog before cancelling the batch".into(),
        );
    }
    batch.state = State::Closed;
    batch.recipe = Recipe::default();
    batch.preview_sha256 = None;
    batch.receipt = Some(receipt);
    batches.save(&batch)?;
    batches.clean(&batch)
}

fn submit(
    data_dir: &Path,
    batches: &Batches,
    batch: &mut Batch,
    file: File,
) -> Result<ImportReport, String> {
    let data = crate::open_store(data_dir)?;
    let mut options = options(batch);
    options.translations = data
        .store
        .type_translations()
        .map_err(|error| error.to_string())?;
    batch.state = State::Confirming;
    batches.save_reserved(
        batch,
        file.metadata().map_err(|error| error.to_string())?.len(),
    )?;
    import_marc21(BufReader::new(file), &data.store, &options).map_err(|error| {
        format!("confirmation interrupted: {error}. Check the catalog before cancelling the batch")
    })
}

fn print_failures(report: &ImportReport, render: &RenderReport) {
    let records = report
        .failures
        .iter()
        .map(|failure| (failure.index, failure.offset, None));
    let items = report
        .item_failures
        .iter()
        .map(|failure| (failure.index, 0, Some(failure.position)));
    let mut failures: Vec<_> = records.chain(items).collect();
    failures.sort_unstable();
    for (index, offset, item) in failures {
        let (index, offset) = render.source_position(index, offset);
        if let Some(item) = item {
            eprintln!(
                "elzevir: record {} (byte {offset}), item {} refused",
                index + 1,
                item + 1
            );
        } else {
            eprintln!("elzevir: record {} (byte {offset}) refused", index + 1);
        }
    }
}
