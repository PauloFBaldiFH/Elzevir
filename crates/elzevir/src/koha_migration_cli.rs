// SPDX-License-Identifier: GPL-3.0-or-later
//! `elzevir migrate-koha`: branches, patron categories, patrons and their restrictions
//! from a whole Koha database dump (docs/specs/migracao/leitores-do-koha.md).

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

use clap::Args;
use elzevir_app::commands::{WriteCommand, WriteOutput};
use elzevir_app::koha_migration::{KohaMigrated, KohaPlan, Leftover, plan};
use elzevir_app::patrons::{PatronCommand, PatronOutput};

use crate::open_store;

/// `elzevir migrate-koha`.
#[derive(Args)]
pub(crate) struct MigrateKohaArgs {
    /// Institution data directory.
    #[arg(long)]
    data_dir: PathBuf,
    /// The dump `koha-dump` wrote (`.sql` or `.sql.gz`).
    dump: PathBuf,
}

/// Reads the dump, writes in batches and prints the report (items 1 to 18).
pub(crate) fn run(args: &MigrateKohaArgs) -> Result<(), String> {
    // Locked first: with a server running on the directory, nothing is read.
    let data = open_store(&args.data_dir)?;
    let zone = crate::config::load(&args.data_dir)?.time_zone;
    let today = jiff::Timestamp::now().to_zoned(zone.clone()).date();
    let input = open(&args.dump)?;
    let plan = plan(input, today, &zone).map_err(|e| format!("{}: {e}", args.dump.display()))?;
    let mut done = KohaMigrated::default();
    for batch in plan.batches() {
        let answer = data
            .store
            .submit(WriteCommand::Patrons(PatronCommand::MigrateKoha(Box::new(
                batch,
            ))))
            .map_err(|e| e.to_string())?
            .wait()
            .map_err(|e| e.to_string())?;
        let WriteOutput::Patrons(PatronOutput::KohaMigrated(write)) = answer else {
            return Err("unexpected answer from the store".to_owned());
        };
        done.add(write);
    }
    print!("{}", report(&plan, &done));
    Ok(())
}

/// The dump, decompressed when its first two bytes are gzip's (item 1).
fn open(path: &PathBuf) -> Result<Box<dyn BufRead>, String> {
    let file = File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let mut input = BufReader::new(file);
    let head = input
        .fill_buf()
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    Ok(if head.starts_with(&[0x1f, 0x8b]) {
        Box::new(BufReader::new(flate2::bufread::MultiGzDecoder::new(input)))
    } else {
        Box::new(input)
    })
}

const fn leftover(why: Leftover) -> &'static str {
    match why {
        Leftover::NoNumber => "no borrowernumber",
        Leftover::Cardnumber => "no card number, or a longer one than 32 characters",
        Leftover::Name => "no name, or a longer one than 256 characters",
        Leftover::UnknownBranch => "branchcode not in branches",
        Leftover::UnknownCategory => "categorycode not in categories",
    }
}

/// The report, without personal data: counts and Koha's patron numbers (item 17).
fn report(plan: &KohaPlan, done: &KohaMigrated) -> String {
    let mut lines = vec![
        format!(
            "branches: {} created, {} updated",
            done.branches_created, done.branches_updated
        ),
        format!(
            "patron categories: {} created, {} found by name, {} with the default period of 12 months",
            done.categories_created, done.categories_linked, plan.default_periods
        ),
        format!(
            "patrons: {} migrated, {} already migrated, {} left out, {} with staff permissions in Koha",
            done.patrons_added,
            done.already_migrated,
            plan.left.len() + done.cardnumber_taken.len(),
            plan.staff
        ),
    ];
    for (number, why) in &plan.left {
        lines.push(format!("  borrowernumber {number}: {}", leftover(*why)));
    }
    for number in &done.cardnumber_taken {
        lines.push(format!(
            "  borrowernumber {number}: card number taken by another patron"
        ));
    }
    lines.push(format!(
        "restrictions: {} migrated, {} already ended",
        done.restrictions, plan.ended_restrictions
    ));
    lines.push(format!(
        "OPAC passwords: {} migrated, {} in a format older than bcrypt left out",
        done.passwords, plan.old_passwords
    ));
    if !plan.cleared.is_empty() {
        lines.push("fields emptied for breaking a rule:".to_owned());
        for (field, count) in &plan.cleared {
            lines.push(format!("  {}: {count}", field.0));
        }
    }
    if !plan.left_out.is_empty() {
        lines.push("borrowers columns not kept (patrons with a value):".to_owned());
        for (column, count) in &plan.left_out {
            lines.push(format!("  {column}: {count}"));
        }
    }
    lines.push("not migrated: CPF (Koha has no column)".to_owned());
    let mut text = lines.join("\n");
    text.push('\n');
    text
}
