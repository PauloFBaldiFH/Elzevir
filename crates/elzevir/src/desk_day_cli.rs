// SPDX-License-Identifier: GPL-3.0-or-later
//! `elzevir desk-day`: replays the day Koha did on a copy of the library on the same copy
//! after the switch, and compares each step (docs/specs/migracao/desk-day.md).

use std::fmt::Write as _;
use std::path::PathBuf;

use clap::Args;
use elzevir_app::accounts::PaymentRequest;
use elzevir_app::circulation_rules::{CirculationCommand, CirculationOutput, CirculationRefusal};
use elzevir_app::commands::{WriteCommand, WriteOutput, WriteRefusal};
use elzevir_app::desk_day::{
    DayCheck, DeskItem, KohaDay, KohaStep, MAX_STEPS, Step, StepAnswer, StepLevel, checkin_answer,
    checkout_answer, compare, hold_placed_answer, renew_answer,
};
use elzevir_app::holds::{HoldLevel, HoldRequest};
use elzevir_app::koha_parity::{ReturnHold, hold_answer};
use elzevir_app::loans::{CheckinRequest, CheckoutRequest, RenewRequest};
use elzevir_domain::account::PaymentKind;
use elzevir_domain::catalog::Money;
use elzevir_domain::checkin::LoanOutcome;
use elzevir_domain::hold::HoldMatch;
use elzevir_domain::patron::PatronId;
use elzevir_store::{CommandFailed, ElzevirStore};
use jiff::Zoned;

use crate::open_store;

/// Largest day file read, far above the output of `MAX_STEPS` steps.
const MAX_DAY_BYTES: u64 = 16 * 1024 * 1024;

/// `elzevir desk-day`.
#[derive(Args)]
pub(crate) struct DeskDayArgs {
    /// Institution data directory that received the switch of the copy, before Koha's day.
    #[arg(long)]
    data_dir: PathBuf,
    /// Confirms that the folder is a copy: the day writes to it.
    #[arg(long)]
    on_a_copy: bool,
    /// The JSON written by `tools/koha-parity/desk_day.pl` on the same copy.
    day: PathBuf,
}

/// Replays the day, prints the comparison and fails on any step that differs (items 7
/// to 10).
pub(crate) fn run(args: &DeskDayArgs) -> Result<(), String> {
    if !args.on_a_copy {
        return Err(
            "desk-day writes to the data directory: run it on a copy, with --on-a-copy".to_owned(),
        );
    }
    let koha = read_day(&args.day)?;
    let zone = crate::config::load(&args.data_dir)?.time_zone;
    let data = open_store(&args.data_dir)?;
    let mut answers = Vec::with_capacity(koha.steps.len());
    for step in &koha.steps {
        let at = jiff::civil::DateTime::strptime("%Y-%m-%d %H:%M", &step.at)
            .and_then(|moment| moment.to_zoned(zone.clone()))
            .map_err(|e| format!("{}: `at` is not YYYY-MM-DD HH:MM: {e}", args.day.display()))?;
        answers.push(replay(&data.store, step, at)?);
    }
    let check = compare(&koha, &answers);
    print!("{}", report(&koha, &check));
    if check.agrees() {
        Ok(())
    } else {
        Err("the day differs from Koha's".to_owned())
    }
}

fn read_day(path: &PathBuf) -> Result<KohaDay, String> {
    let size = std::fs::metadata(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .len();
    if size > MAX_DAY_BYTES {
        return Err(format!(
            "{}: larger than {MAX_DAY_BYTES} bytes",
            path.display()
        ));
    }
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let day: KohaDay =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    if day.steps.len() > MAX_STEPS {
        return Err(format!("{}: more than {MAX_STEPS} steps", path.display()));
    }
    Ok(day)
}

/// The answer of one step, or why it could not be done; the outer error is a storage
/// failure, which stops the day.
type Replayed = Result<Result<StepAnswer, String>, String>;

/// What a desk command gave: its output, or the circulation refusal.
type Done = Result<CirculationOutput, CirculationRefusal>;

fn submit(store: &ElzevirStore, command: CirculationCommand) -> Result<Done, String> {
    let outcome = store
        .submit(WriteCommand::Circulation(command))
        .map_err(|e| e.to_string())?
        .wait();
    match outcome {
        Ok(WriteOutput::Circulation(output)) => Ok(Ok(output)),
        Err(CommandFailed::Rejected(WriteRefusal::Circulation(refusal))) => Ok(Err(refusal)),
        Ok(_) => Err("unexpected answer from the store".to_owned()),
        Err(failed) => Err(failed.to_string()),
    }
}

fn item(store: &ElzevirStore, itemnumber: u64) -> Result<Result<DeskItem, String>, String> {
    Ok(store
        .desk_item(itemnumber)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("the Item {itemnumber} did not come")))
}

fn patron(
    store: &ElzevirStore,
    borrowernumber: u64,
) -> Result<Result<(PatronId, elzevir_domain::catalog::BranchId), String>, String> {
    Ok(store
        .desk_patron(borrowernumber)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("the patron {borrowernumber} did not come")))
}

fn owed(store: &ElzevirStore, patron: Option<PatronId>) -> Result<Option<Money>, String> {
    patron
        .map(|patron| store.owed_by(patron).map_err(|e| e.to_string()))
        .transpose()
}

/// Does one step at `at` through the desk command (items 7 to 9).
fn replay(store: &ElzevirStore, step: &KohaStep, at: Zoned) -> Replayed {
    match step.step {
        Step::Checkout {
            borrowernumber,
            itemnumber,
            confirm,
        } => checkout(store, (borrowernumber, itemnumber, confirm), at),
        Step::Checkin { itemnumber } => checkin(store, itemnumber, at),
        Step::Renew { itemnumber } => renew(store, itemnumber, at),
        Step::Hold {
            borrowernumber,
            itemnumber,
            level,
        } => hold(store, (borrowernumber, itemnumber, level), at),
        Step::Pay {
            borrowernumber,
            amount_cents,
        } => pay(store, borrowernumber, Money(amount_cents), at),
    }
}

fn checkout(
    store: &ElzevirStore,
    (borrowernumber, itemnumber, confirm): (u64, u64, bool),
    at: Zoned,
) -> Replayed {
    let (item, (patron, _)) = match (item(store, itemnumber)?, patron(store, borrowernumber)?) {
        (Ok(item), Ok(patron)) => (item, patron),
        (Err(gap), _) | (_, Err(gap)) => return Ok(Err(gap)),
    };
    let done = submit(
        store,
        CirculationCommand::Checkout(CheckoutRequest {
            patron,
            barcode: item.barcode,
            desk: item.holding,
            on_site: false,
            confirmed: confirm,
            at,
        }),
    )?;
    let answer = match done {
        Ok(CirculationOutput::CheckedOut(out)) => checkout_answer(Ok(&out.due)),
        Err(CirculationRefusal::Checkout(refusal)) => checkout_answer(Err(&refusal)),
        other => return Err(format!("unexpected checkout answer: {other:?}")),
    };
    answered(store, answer, Some(patron))
}

fn checkin(store: &ElzevirStore, itemnumber: u64, at: Zoned) -> Replayed {
    let item = match item(store, itemnumber)? {
        Ok(item) => item,
        Err(gap) => return Ok(Err(gap)),
    };
    let done = submit(
        store,
        CirculationCommand::Checkin(CheckinRequest {
            barcode: item.barcode,
            desk: item.holding,
            at,
        }),
    )?;
    let answer = match done {
        Ok(CirculationOutput::CheckedIn(checked)) => {
            let returned = match checked.loan {
                LoanOutcome::NotOnLoan => "not_on_loan",
                LoanOutcome::Ended { .. } => "ok",
                LoanOutcome::KeptLost => "kept_lost",
            };
            let hold = match checked.hold {
                None => hold_answer(None),
                Some(hold) => {
                    let borrowernumber = store
                        .hold_borrowernumber(hold.found.hold())
                        .map_err(|e| e.to_string())?;
                    let state = match hold.found {
                        HoldMatch::Waiting(_) => ReturnHold::Waiting,
                        HoldMatch::InTransit(_) => ReturnHold::InTransit,
                        HoldMatch::Reserved(_) => ReturnHold::Reserved,
                    };
                    hold_answer(Some((state, borrowernumber.as_deref())))
                }
            };
            checkin_answer(returned, &hold)
        }
        Err(CirculationRefusal::Checkin(_)) => "refused".to_owned(),
        other => return Err(format!("unexpected checkin answer: {other:?}")),
    };
    answered(store, answer, item.borrower)
}

fn renew(store: &ElzevirStore, itemnumber: u64, at: Zoned) -> Replayed {
    let item = match item(store, itemnumber)? {
        Ok(item) => item,
        Err(gap) => return Ok(Err(gap)),
    };
    let done = submit(
        store,
        CirculationCommand::Renew(RenewRequest {
            barcode: item.barcode,
            override_limit: false,
            at,
            by_patron: None,
        }),
    )?;
    let answer = match done {
        Ok(CirculationOutput::Renewed(renewed)) => renew_answer(Ok(&renewed.due)),
        Err(CirculationRefusal::Renew(refusal)) => renew_answer(Err(&refusal)),
        other => return Err(format!("unexpected renewal answer: {other:?}")),
    };
    answered(store, answer, item.borrower)
}

fn hold(
    store: &ElzevirStore,
    (borrowernumber, itemnumber, level): (u64, u64, StepLevel),
    at: Zoned,
) -> Replayed {
    let (item, (patron, branch)) = match (item(store, itemnumber)?, patron(store, borrowernumber)?)
    {
        (Ok(item), Ok(patron)) => (item, patron),
        (Err(gap), _) | (_, Err(gap)) => return Ok(Err(gap)),
    };
    let done = submit(
        store,
        CirculationCommand::PlaceHold(HoldRequest {
            patron,
            barcode: item.barcode,
            level: match level {
                StepLevel::Record => HoldLevel::Record,
                StepLevel::Item => HoldLevel::Item,
            },
            pickup: branch,
            at,
            by_patron: false,
        }),
    )?;
    let answer = match done {
        Ok(CirculationOutput::HoldPlaced(placed)) => hold_placed_answer(Ok(placed.priority)),
        Err(CirculationRefusal::Hold(refusal)) => hold_placed_answer(Err(&refusal)),
        other => return Err(format!("unexpected hold answer: {other:?}")),
    };
    answered(store, answer, Some(patron))
}

fn pay(store: &ElzevirStore, borrowernumber: u64, amount: Money, at: Zoned) -> Replayed {
    let (patron, branch) = match patron(store, borrowernumber)? {
        Ok(patron) => patron,
        Err(gap) => return Ok(Err(gap)),
    };
    let done = submit(
        store,
        CirculationCommand::Pay(PaymentRequest {
            patron,
            kind: PaymentKind::Payment,
            amount,
            lines: Vec::new(),
            desk: branch,
            at,
        }),
    )?;
    let answer = match done {
        Ok(CirculationOutput::Paid(_)) => "ok".to_owned(),
        Err(CirculationRefusal::Pay(_)) => "refused".to_owned(),
        other => return Err(format!("unexpected payment answer: {other:?}")),
    };
    answered(store, answer, Some(patron))
}

/// The answer with what the patron owes after it.
fn answered(store: &ElzevirStore, answer: String, patron: Option<PatronId>) -> Replayed {
    Ok(Ok(StepAnswer {
        answer,
        balance: owed(store, patron)?,
    }))
}

/// Counts, then each step that differs, by its position and values only (item 10).
fn report(koha: &KohaDay, check: &DayCheck) -> String {
    let mut text = format!(
        "Koha {}: {} steps, {} the same, {} different\n",
        koha.koha_version.as_deref().unwrap_or("(version unknown)"),
        check.steps,
        check.same,
        check.divergences.len()
    );
    for divergence in &check.divergences {
        // Writing to a String cannot fail.
        let _written = writeln!(
            text,
            "  DIFFERENT step {} ({}): Koha {}, Elzevir {}",
            divergence.position, divergence.op, divergence.koha, divergence.elzevir
        );
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_report_names_steps_by_position() {
        let koha: KohaDay = serde_json::from_str(
            r#"{"koha_version": "26.05", "steps": [
                {"op": "renew", "itemnumber": 3, "at": "2026-11-27 10:00",
                 "answer": "too_soon", "balance_cents": 0}
            ]}"#,
        )
        .unwrap();
        let check = compare(
            &koha,
            &[Ok(StepAnswer {
                answer: "ok due 2026-12-11 23:59".to_owned(),
                balance: Some(Money(0)),
            })],
        );
        assert_eq!(
            report(&koha, &check),
            "Koha 26.05: 1 steps, 0 the same, 1 different\n  DIFFERENT step 1 (renew): \
             Koha too_soon (owes 0), Elzevir ok due 2026-12-11 23:59 (owes 0)\n"
        );
    }
}
