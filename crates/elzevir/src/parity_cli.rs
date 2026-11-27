// SPDX-License-Identifier: GPL-3.0-or-later
//! `elzevir parity-koha`: Koha's own answers on a copy of the library against the
//! Elzevir's on the same copy after the switch (docs/specs/migracao/koha-parity.md).

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::PathBuf;

use clap::Args;
use elzevir_app::koha_parity::{KohaAnswers, ParityGap, Question, QuestionCheck, compare};

use crate::open_store;

/// Largest recorder file read, far above the answers of a large library.
const MAX_ANSWERS_BYTES: u64 = 256 * 1024 * 1024;

/// `elzevir parity-koha`.
#[derive(Args)]
pub(crate) struct ParityKohaArgs {
    /// Institution data directory that received the switch.
    #[arg(long)]
    data_dir: PathBuf,
    /// The JSON written by `tools/koha-parity/record.pl` on the same copy.
    answers: PathBuf,
}

/// Prints the comparison and fails on any divergence or answer without a counterpart
/// (items 6 to 8).
pub(crate) fn run(args: &ParityKohaArgs) -> Result<(), String> {
    let koha = read_answers(&args.answers)?;
    let data = open_store(&args.data_dir)?;
    let zone = crate::config::load(&args.data_dir)?.time_zone;
    let moment = |name: &str, text: &str| {
        jiff::civil::DateTime::strptime("%Y-%m-%d %H:%M", text)
            .and_then(|moment| moment.to_zoned(zone.clone()))
            .map_err(|e| {
                format!(
                    "{}: `{name}` is not YYYY-MM-DD HH:MM: {e}",
                    args.answers.display()
                )
            })
    };
    let now = moment("now", &koha.now)?;
    let renewals_at = match &koha.renewals_at {
        Some(text) => moment("renewals_at", text)?,
        None => now.clone(),
    };
    let itemnumbers: BTreeSet<u64> = koha
        .due_dates
        .iter()
        .map(|answer| answer.itemnumber)
        .chain(koha.fines.iter().map(|answer| answer.itemnumber))
        .chain(koha.renewals.iter().map(|answer| answer.itemnumber))
        .chain(koha.holds_on_return.iter().map(|answer| answer.itemnumber))
        .collect();
    let elzevir = data
        .store
        .koha_parity_answers(&itemnumbers, &now, &renewals_at)
        .map_err(|e| e.to_string())?;
    let checks = compare(&koha, &elzevir);
    print!("{}", report(&koha, &checks));
    if checks.iter().all(QuestionCheck::agrees) {
        Ok(())
    } else {
        Err("answers differ from Koha's, or have nothing to be compared with".to_owned())
    }
}

fn read_answers(path: &PathBuf) -> Result<KohaAnswers, String> {
    let size = std::fs::metadata(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .len();
    if size > MAX_ANSWERS_BYTES {
        return Err(format!(
            "{}: larger than {MAX_ANSWERS_BYTES} bytes",
            path.display()
        ));
    }
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

const fn question(question: Question) -> &'static str {
    match question {
        Question::DueDate => "due dates",
        Question::Fine => "fines",
        Question::Renewal => "renewals",
        Question::HoldOnReturn => "holds of returned Items",
    }
}

const fn gap(gap: ParityGap) -> &'static str {
    match gap {
        ParityGap::ItemMissing => "the Item did not come",
        ParityGap::NotOnLoan => "the Item is not on loan in the Elzevir",
        ParityGap::NoDueDate => "the Elzevir's calendar has no open day to land on",
    }
}

/// Counts, then each answer without a counterpart and each divergence, by `issue_id` and
/// values only (item 8).
fn report(koha: &KohaAnswers, checks: &[QuestionCheck]) -> String {
    let mut text = format!(
        "Koha {}, answers at {}\n",
        koha.koha_version.as_deref().unwrap_or("(version unknown)"),
        koha.now
    );
    for check in checks {
        // Writing to a String cannot fail.
        let _written = writeln!(
            text,
            "{}: {} answers from Koha, {} the same, {} without a counterpart, {} different",
            question(check.question),
            check.answers,
            check.same,
            check.gaps.values().map(Vec::len).sum::<usize>(),
            check.divergences.len()
        );
        for (why, issues) in &check.gaps {
            let numbers: Vec<String> = issues.iter().map(u64::to_string).collect();
            let _written = writeln!(
                text,
                "  {}: {} (issue_id {})",
                gap(*why),
                issues.len(),
                numbers.join(", ")
            );
        }
        for (issue_id, theirs, ours) in &check.divergences {
            let _written = writeln!(
                text,
                "  DIFFERENT issue_id {issue_id}: Koha {theirs}, Elzevir {ours}"
            );
        }
    }
    text
}
