// SPDX-License-Identifier: GPL-3.0-or-later
//! Local operator interface to staged catalog imports.

use std::path::PathBuf;

use clap::{Args, Subcommand};
use elzevir_app::import::batches::BatchPreview;
use elzevir_app::import::batches::{BatchCommand, BatchReply, FormatChoice};
use elzevir_app::import_detection::ImportFormat;

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
    Stage {
        file: PathBuf,
        /// Choose the tabular reader when the sample is inconclusive.
        #[arg(long)]
        table: bool,
        /// Choose the partial Koha SQL reader when the sample is inconclusive.
        #[arg(long, conflicts_with = "table")]
        koha_sql: bool,
    },
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
        Action::Stage {
            file,
            table,
            koha_sql,
        } => BatchCommand::Stage(
            Box::new(open_path(&file)?),
            table
                .then_some(ImportFormat::Delimited)
                .or(koha_sql.then_some(ImportFormat::KohaSqlDump)),
        ),
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
            if let Some(inventory) = view.archive {
                for entry in inventory.entries {
                    println!(
                        "{}",
                        serde_json::to_string(&entry).map_err(|error| error.to_string())?
                    );
                }
            }
            if let Some(preview) = view.preview {
                return print_preview(&preview, view.recipe.matching);
            }
            Ok(())
        }
        BatchReply::Completed(completion) => {
            for refusal in completion.refusals {
                let (record, offset) = (refusal.record, refusal.offset);
                if let Some(detail) = refusal.detail {
                    eprintln!("elzevir: record {record} (byte {offset}), {detail}");
                } else if let Some(item) = refusal.item {
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

pub(crate) fn model(tsv: bool) -> Result<(), String> {
    use elzevir_app::import::table::{Delimiter, write_model_header};
    let delimiter = if tsv {
        Delimiter::Tab
    } else {
        Delimiter::Comma
    };
    write_model_header(std::io::stdout().lock(), delimiter).map_err(|error| error.to_string())
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
