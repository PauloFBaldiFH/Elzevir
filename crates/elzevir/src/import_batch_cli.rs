// SPDX-License-Identifier: GPL-3.0-or-later
//! Local operator interface to staged MARC imports.

use std::path::PathBuf;

use clap::{Args, Subcommand};
use elzevir_app::import::batches::BatchPreview;
use elzevir_app::import::batches::{BatchCommand, BatchReply, FormatChoice};

use crate::import_batches::{open_path, read_recipe};

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
    let command = match args.action {
        Action::Stage { file } => BatchCommand::Stage(Box::new(open_path(&file)?), None),
        Action::Edit {
            id,
            revision,
            recipe,
        } => BatchCommand::Edit(id, revision, read_recipe(&recipe)?, FormatChoice::Keep),
        Action::Preview { id } => BatchCommand::Preview(id, None, None),
        Action::Confirm { id, revision } => BatchCommand::Confirm(id, revision),
        Action::Cancel {
            id,
            acknowledge_interrupted,
        } => BatchCommand::Cancel(id, None, acknowledge_interrupted),
    };
    match crate::import_service::execute(&args.data_dir, args.operator, None, command)
        .map_err(|error| error.to_string())?
    {
        BatchReply::Batch(view) => {
            println!(
                "batch {} revision {} {:?}",
                view.batch.id, view.batch.revision, view.batch.state
            );
            if let Some(preview) = view.preview {
                return print_preview(&preview, view.recipe.matching);
            }
            Ok(())
        }
        BatchReply::Completed(completion) => {
            for refusal in completion.refusals {
                let (record, offset) = (refusal.record, refusal.offset);
                if let Some(item) = refusal.item {
                    eprintln!("elzevir: record {record} (byte {offset}), item {item} refused");
                } else {
                    eprintln!("elzevir: record {record} (byte {offset}) refused");
                }
            }
            if let Some(receipt) = completion.receipt {
                println!(
                    "{}",
                    serde_json::to_string(&receipt).map_err(|error| error.to_string())?
                );
            }
            completion.error.map_or(Ok(()), Err)
        }
        BatchReply::Cancelled | BatchReply::List(_) => Ok(()),
    }
}

fn print_preview(
    preview: &BatchPreview,
    matching: Option<elzevir_app::matching::Matching>,
) -> Result<(), String> {
    let BatchPreview { rendered, report } = preview;
    println!(
        "{} original records, {} excluded, {} edited, {} items to submit, matching {:?}",
        rendered.read, rendered.excluded, rendered.changed, report.items, matching
    );
    for sample in &rendered.samples {
        if sample.fields.is_empty() {
            println!("record {}: field order changed", sample.record);
        }
        for field in &sample.fields {
            println!(
                "record {}, field {}: {:?} -> {:?}",
                sample.record, field.tag, field.before, field.after
            );
        }
    }
    crate::catalog_cli::print_preview(report);
    if report.issues.is_empty() {
        Ok(())
    } else {
        Err("preview requires review".into())
    }
}
