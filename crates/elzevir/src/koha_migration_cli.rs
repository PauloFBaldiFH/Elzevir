// SPDX-License-Identifier: GPL-3.0-or-later
//! `elzevir migrate-koha`: branches, patron categories, patrons and their restrictions,
//! then current loans, holds, patron accounts and returned loans, from a whole Koha database dump
//! (docs/specs/migracao/leitores-do-koha.md, circulacao-do-koha.md).

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

use clap::Args;
use elzevir_app::commands::{WriteCommand, WriteOutput};
use elzevir_app::koha_migration::{
    CirculationLeftover, KohaAccountsMigrated, KohaCirculationMigrated, KohaHistoryMigrated,
    KohaMigrated, KohaPlan, Leftover, plan,
};
use elzevir_app::patrons::{PatronCommand, PatronOutput};

use elzevir_store::ElzevirStore;

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
    let done = migrate_patrons(&data.store, &plan)?;
    let (circulation, accounts, history) = migrate_circulation(&data.store, &plan)?;
    print!("{}", report(&plan, &done));
    print!("{}", circulation_report(&plan, &circulation));
    print!("{}", accounts_report(&plan, &accounts));
    print!("{}", history_report(&plan, &history));
    Ok(())
}

/// The branches, patron categories, patrons and their restrictions, in batches
/// (leitores-do-koha.md, items 4 to 16).
pub(crate) fn migrate_patrons(
    store: &ElzevirStore,
    plan: &KohaPlan,
) -> Result<KohaMigrated, String> {
    let mut done = KohaMigrated::default();
    for batch in plan.batches() {
        let answer = store
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
    Ok(done)
}

/// The loans, holds, accounts and returned loans, in batches, once the catalog is in
/// (circulacao-do-koha.md).
pub(crate) fn migrate_circulation(
    store: &ElzevirStore,
    plan: &KohaPlan,
) -> Result<
    (
        KohaCirculationMigrated,
        KohaAccountsMigrated,
        KohaHistoryMigrated,
    ),
    String,
> {
    let mut circulation = KohaCirculationMigrated::default();
    for batch in plan.circulation_batches() {
        let answer = store
            .submit(WriteCommand::Patrons(
                PatronCommand::MigrateKohaCirculation(Box::new(batch)),
            ))
            .map_err(|e| e.to_string())?
            .wait()
            .map_err(|e| e.to_string())?;
        let WriteOutput::Patrons(PatronOutput::KohaCirculationMigrated(write)) = answer else {
            return Err("unexpected answer from the store".to_owned());
        };
        circulation.add(write);
    }
    let mut accounts = KohaAccountsMigrated::default();
    for batch in plan.account_batches() {
        let answer = store
            .submit(WriteCommand::Patrons(PatronCommand::MigrateKohaAccounts(
                Box::new(batch),
            )))
            .map_err(|e| e.to_string())?
            .wait()
            .map_err(|e| e.to_string())?;
        let WriteOutput::Patrons(PatronOutput::KohaAccountsMigrated(write)) = answer else {
            return Err("unexpected answer from the store".to_owned());
        };
        accounts.add(write);
    }
    let mut history = KohaHistoryMigrated::default();
    for batch in plan.history_batches() {
        let answer = store
            .submit(WriteCommand::Patrons(PatronCommand::MigrateKohaHistory(
                Box::new(batch),
            )))
            .map_err(|e| e.to_string())?
            .wait()
            .map_err(|e| e.to_string())?;
        let WriteOutput::Patrons(PatronOutput::KohaHistoryMigrated(write)) = answer else {
            return Err("unexpected answer from the store".to_owned());
        };
        history.add(write);
    }
    Ok((circulation, accounts, history))
}

/// The dump, decompressed when its first two bytes are gzip's (item 1).
pub(crate) fn open(path: &PathBuf) -> Result<Box<dyn BufRead>, String> {
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

pub(crate) const fn leftover(why: Leftover) -> &'static str {
    match why {
        Leftover::NoNumber => "no borrowernumber",
        Leftover::Cardnumber => "no card number, or a longer one than 32 characters",
        Leftover::Name => "no name, or a longer one than 256 characters",
        Leftover::UnknownBranch => "branchcode not in branches",
        Leftover::UnknownCategory => "categorycode not in categories",
    }
}

/// The report, without personal data: counts and Koha's patron numbers (item 17).
pub(crate) fn report(plan: &KohaPlan, done: &KohaMigrated) -> String {
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

pub(crate) const fn circulation_leftover(why: CirculationLeftover) -> &'static str {
    match why {
        CirculationLeftover::Incomplete => "a number, patron, Item or date missing",
        CirculationLeftover::Patron => "patron not migrated",
        CirculationLeftover::Item => "no Item, or two, with its itemnumber or barcode",
        CirculationLeftover::Record => "no record, or two, with its biblionumber",
        CirculationLeftover::Branch => "branchcode not in branches",
        CirculationLeftover::ItemOnLoan => "Item already lent to another patron",
    }
}

/// The report of loans and holds, by Koha's numbers only (circulacao-do-koha.md, item 8).
pub(crate) fn circulation_report(plan: &KohaPlan, done: &KohaCirculationMigrated) -> String {
    let mut loans_left = plan.loans_left.clone();
    loans_left.extend(done.loans_left.iter().copied());
    let mut holds_left = plan.holds_left.clone();
    holds_left.extend(done.holds_left.iter().copied());
    let mut lines = vec![format!(
        "loans: {} migrated, {} left out",
        done.loans,
        loans_left.len()
    )];
    for (number, why) in &loans_left {
        lines.push(format!(
            "  issue_id {number}: {}",
            circulation_leftover(*why)
        ));
    }
    lines.push(format!(
        "holds: {} migrated, {} left out, {} suspended, {} queued with an end date (dropped)",
        done.holds,
        holds_left.len(),
        plan.suspended_holds,
        plan.holds_with_end
    ));
    for (number, why) in &holds_left {
        lines.push(format!(
            "  reserve_id {number}: {}",
            circulation_leftover(*why)
        ));
    }
    lines.push(format!(
        "loans and holds already migrated: {}",
        done.already_migrated
    ));
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// The report of the accounts, by Koha's numbers only (circulacao-do-koha.md, item 13).
pub(crate) fn accounts_report(plan: &KohaPlan, done: &KohaAccountsMigrated) -> String {
    let read = &plan.accounts_read;
    let mut lines = vec![format!(
        "patron accounts: {} migrated with {} lines and {} offsets, {} already migrated, {} left out",
        done.accounts,
        done.lines,
        done.offsets,
        done.already_migrated,
        done.accounts_left.len()
    )];
    for number in &done.accounts_left {
        lines.push(format!("  borrowernumber {number}: patron not migrated"));
    }
    lines.push(format!(
        "account lines left out: {}; offsets left out: {}; amounts rounded to cents: {}",
        read.lines_left.len(),
        read.offsets_left,
        read.rounded
    ));
    for number in &read.lines_left {
        lines.push(format!(
            "  accountlines_id {number}: a number, patron, type, amount or date missing"
        ));
    }
    if !read.other_types.is_empty() {
        lines.push("types of this Koha brought as MANUAL or CREDIT:".to_owned());
        for (code, count) in &read.other_types {
            lines.push(format!("  {code}: {count}"));
        }
    }
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// The report of the returned loans, by Koha's numbers only (circulacao-do-koha.md,
/// item 17).
pub(crate) fn history_report(plan: &KohaPlan, done: &KohaHistoryMigrated) -> String {
    let mut left = plan.returned_left.clone();
    left.extend(done.left.iter().copied());
    let mut lines = vec![format!(
        "returned loans: {} migrated ({} in a reading history, the others without patron), {} already migrated, {} left out",
        done.returned,
        done.with_patron,
        done.already_migrated,
        left.len()
    )];
    for (number, why) in &left {
        lines.push(format!(
            "  issue_id {number}: {}",
            circulation_leftover(*why)
        ));
    }
    let mut text = lines.join("\n");
    text.push('\n');
    text
}
