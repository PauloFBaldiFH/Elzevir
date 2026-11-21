// SPDX-License-Identifier: GPL-3.0-or-later
//! Local operator interface to staged catalog imports.

use std::path::PathBuf;

use clap::{Args, Subcommand};
use elzevir_app::import::batches::BatchPreview;
use elzevir_app::import::batches::{BatchCommand, BatchReply, FormatChoice};
use elzevir_app::import_detection::ImportFormat;

use crate::import_batches::{open_path, read_recipe};

/// `elzevir import`: either a direct ISO 2709 import of `FILE`, or one step of a
/// private batch that is staged, edited, previewed and only then confirmed.
#[derive(Args)]
#[command(args_conflicts_with_subcommands = true, subcommand_negates_reqs = true)]
pub(crate) struct ImportCommand {
    #[command(subcommand)]
    step: Option<Step>,
    // `None` only when a step is given: the direct arguments are required otherwise.
    #[command(flatten)]
    direct: Option<crate::catalog_cli::ImportArgs>,
}

/// The institution and operator that own a batch.
#[derive(Args)]
struct Owner {
    /// Existing institution data directory.
    #[arg(long)]
    data_dir: PathBuf,
    /// Operator identity supplied by the local administrator.
    #[arg(long)]
    operator: u64,
}

#[derive(Subcommand)]
enum Step {
    /// Capture the original file into a new private batch.
    Stage {
        #[command(flatten)]
        owner: Owner,
        file: PathBuf,
        /// Choose the tabular reader when the sample is inconclusive.
        #[arg(long)]
        table: bool,
        /// Choose the partial Koha SQL reader when the sample is inconclusive.
        #[arg(long, conflicts_with = "table")]
        koha_sql: bool,
    },
    /// Save a complete recipe for an existing revision of a batch.
    Edit {
        #[command(flatten)]
        owner: Owner,
        id: String,
        revision: u64,
        recipe: PathBuf,
    },
    /// Preview the current revision of a batch and mark it ready when it passes.
    Preview {
        #[command(flatten)]
        owner: Owner,
        id: String,
    },
    /// Explicitly confirm a previously previewed revision of a batch.
    Confirm {
        #[command(flatten)]
        owner: Owner,
        id: String,
        revision: u64,
    },
    /// Remove a private batch and its source.
    Cancel {
        #[command(flatten)]
        owner: Owner,
        id: String,
        /// Confirm that the catalog was checked after a possibly partial import.
        #[arg(long)]
        acknowledge_interrupted: bool,
    },
}

/// Runs a direct import, or the requested batch step.
pub(crate) fn run(args: ImportCommand) -> Result<(), String> {
    match (args.step, args.direct) {
        (Some(step), _) => run_step(step),
        (None, Some(direct)) => crate::catalog_cli::import(&direct),
        (None, None) => Err("give a MARC file to import or a batch step".into()),
    }
}

fn run_step(step: Step) -> Result<(), String> {
    let (owner, command) = match step {
        Step::Stage {
            owner,
            file,
            table,
            koha_sql,
        } => (
            owner,
            BatchCommand::Stage(
                Box::new(open_path(&file)?),
                table
                    .then_some(ImportFormat::Delimited)
                    .or(koha_sql.then_some(ImportFormat::KohaSqlDump)),
            ),
        ),
        Step::Edit {
            owner,
            id,
            revision,
            recipe,
        } => {
            let recipe = read_recipe(&recipe)?;
            (
                owner,
                BatchCommand::Edit(id, revision, recipe, FormatChoice::Keep),
            )
        }
        Step::Preview { owner, id } => (owner, BatchCommand::Preview(id, None, None)),
        Step::Confirm {
            owner,
            id,
            revision,
        } => (owner, BatchCommand::Confirm(id, revision)),
        Step::Cancel {
            owner,
            id,
            acknowledge_interrupted,
        } => (
            owner,
            BatchCommand::Cancel(id, None, acknowledge_interrupted),
        ),
    };
    match crate::import_service::execute(&owner.data_dir, owner.operator, None, command)
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
                return print_preview(&preview, &view.recipe);
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
    recipe: &elzevir_app::import::edits::Recipe,
) -> Result<(), String> {
    let BatchPreview {
        rendered, report, ..
    } = preview;
    let matching = if let Some(profile) = &recipe.biblivre {
        format!(
            "Biblivre 001/003 in {} (949$a = inventory number, 949$y = {:?}, home #{}, holding #{}, policy #{})",
            recipe.origin.as_deref().unwrap_or_default(),
            profile.date,
            profile.home_branch.0,
            profile.holding_branch.0,
            profile.policy.0
        )
    } else {
        format!("{:?}", recipe.matching)
    };
    println!(
        "{} original records, {} excluded, {} edited, {} items to submit, matching {matching}",
        rendered.read, rendered.excluded, rendered.changed, report.items
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
