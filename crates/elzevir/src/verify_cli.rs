// SPDX-License-Identifier: GPL-3.0-or-later
//! `elzevir verify-koha`: the dump compared with what the Elzevir holds from it, by Koha's
//! numbers (docs/specs/migracao/verificacao-do-koha.md).

use std::fmt::Write as _;
use std::path::PathBuf;

use clap::Args;
use elzevir_app::import::koha_sql::read_database_catalog;
use elzevir_app::koha_migration::plan;
use elzevir_app::koha_verify::{Cause, Table, TableCheck, Unexplained, verify};

use crate::koha_migration_cli::{circulation_leftover, leftover, open};
use crate::open_store;

/// `elzevir verify-koha`.
#[derive(Args)]
pub(crate) struct VerifyKohaArgs {
    /// Institution data directory that received the switch.
    #[arg(long)]
    data_dir: PathBuf,
    /// The dump the switch read (`.sql` or `.sql.gz`).
    dump: PathBuf,
}

/// Prints the comparison and fails when a difference has no cause (items 1 to 3).
pub(crate) fn run(args: &VerifyKohaArgs) -> Result<(), String> {
    let data = open_store(&args.data_dir)?;
    let zone = crate::config::load(&args.data_dir)?.time_zone;
    let today = jiff::Timestamp::now().to_zoned(zone.clone()).date();
    let dumped =
        |e: elzevir_app::import::koha_sql::DumpError| format!("{}: {e}", args.dump.display());
    let plan = plan(open(&args.dump)?, today, &zone).map_err(dumped)?;
    let catalog = read_database_catalog(open(&args.dump)?)
        .map_err(dumped)?
        .koha_records();
    let found = data.store.koha_footprint().map_err(|e| e.to_string())?;
    let checks = verify(&plan, &catalog, &found);
    print!("{}", report(&checks));
    let unexplained: usize = checks.iter().map(|check| check.unexplained.len()).sum();
    if unexplained == 0 {
        Ok(())
    } else {
        Err(format!("{unexplained} differences without a cause"))
    }
}

const fn names(table: Table) -> (&'static str, &'static str) {
    match table {
        Table::Branches => ("branches", "branchcode"),
        Table::Categories => ("patron categories", "categorycode"),
        Table::Patrons => ("patrons", "borrowernumber"),
        Table::Records => ("records", "biblionumber"),
        Table::Items => ("Items", "itemnumber"),
        Table::Loans => ("loans", "issue_id"),
        Table::Holds => ("holds", "reserve_id"),
        Table::Accounts => ("patron accounts", "borrowernumber"),
        Table::Returned => ("returned loans", "issue_id"),
        Table::Rules => ("circulation rules", "id"),
        Table::RepeatableHolidays => ("weekly and annual closings", "repeatable_holidays id"),
        Table::SpecialHolidays => ("dated closings and exceptions", "special_holidays id"),
        Table::AttributeTypes => ("patron attribute types", "code"),
        Table::StatusNames => ("item status names", "category and number"),
        Table::StatusDates => ("item status dates", "itemnumber"),
    }
}

fn cause(cause: &Cause) -> String {
    match cause {
        Cause::Patron(why) => leftover(*why).to_owned(),
        Cause::UnknownBranch(code) => format!("a branch the Elzevir lacks ({code})"),
        Cause::RecordLeftOut => "its record did not come".to_owned(),
        Cause::UnreadableRecord => "MARCXML the Elzevir cannot read".to_owned(),
        Cause::RepeatedBarcode => "barcode repeated in the dump".to_owned(),
        Cause::Anonymized => "anonymized after the migration (counted as arrived)".to_owned(),
        Cause::Circulation(why) => circulation_leftover(*why).to_owned(),
        Cause::PatronLeftOut => "its patron did not come".to_owned(),
        Cause::ItemLeftOut => "its Item did not come".to_owned(),
        Cause::ItemOnLoan => "its Item came lent by another loan of the dump".to_owned(),
        Cause::AlreadyHeld => "the patron already holds the record in the dump".to_owned(),
        Cause::Rule(why) => crate::switch_cli::rule_leftover(*why).to_owned(),
        Cause::UnknownCode(column) => format!("{column} not in the Elzevir"),
        Cause::Holiday(why) => crate::switch_cli::holiday_leftover(*why).to_owned(),
        Cause::Attribute(why) => format!(
            "a value left out: {}",
            crate::koha_attributes_cli::leftover(*why)
        ),
        Cause::StatusNameTaken => "another number of the kind has the name".to_owned(),
    }
}

fn difference(what: &Unexplained) -> String {
    match what {
        Unexplained::Missing => "not in the Elzevir".to_owned(),
        Unexplained::Repeated => "more than once".to_owned(),
        Unexplained::Extra | Unexplained::ExtraBy(_) => {
            "in the Elzevir, not among what the dump brings".to_owned()
        }
        Unexplained::Field(column) => format!("{column} differs"),
        Unexplained::NoNumber => "no 999 $c".to_owned(),
    }
}

/// Counts, causes and differences by Koha's numbers and column names only (items 11 and 12).
fn report(checks: &[TableCheck]) -> String {
    let mut text = String::new();
    for check in checks {
        let (table, key) = names(check.table);
        // Writing to a String cannot fail.
        let _written = writeln!(
            text,
            "{table}: {} in the dump, {} in the Elzevir, {} left out with a cause, {} differences without a cause",
            check.in_dump,
            check.arrived,
            check.left_out(),
            check.unexplained.len()
        );
        for (why, numbers) in &check.explained {
            let _written = writeln!(
                text,
                "  {}: {} ({key} {})",
                cause(why),
                numbers.len(),
                numbers.join(", ")
            );
        }
        for (number, what) in &check.unexplained {
            let key = match what {
                Unexplained::ExtraBy(other) => other,
                _ => key,
            };
            let _written = writeln!(
                text,
                "  WITHOUT A CAUSE {key} {number}: {}",
                difference(what)
            );
        }
    }
    text
}
