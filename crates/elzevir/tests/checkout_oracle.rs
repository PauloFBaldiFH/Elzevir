// SPDX-License-Identifier: GPL-3.0-or-later
//! Each checkout the Koha oracle answered is answered the same by `circ.checkout`
//! (docs/specs/circulacao/emprestimo.md; `scenarios/circulacao/emprestimo.koha.json`).

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "tests: a missing code or field fails the test"
)]

mod oracle;

use std::collections::BTreeSet;
use std::path::Path;

use elzevir_app::catalog::ItemStatus;
use elzevir_app::circulation_rules::{CirculationCommand, CirculationOutput, CirculationRefusal};
use elzevir_app::commands::{WriteCommand, WriteOutput, WriteRefusal};
use elzevir_app::loans::{CheckedOut, CheckoutRefusal, CheckoutRequest};
use elzevir_domain::checkout::{Confirm, HeldFor, Impossible, TooMany, TooManyReason};
use elzevir_domain::circulation_rules::RuleScopeKey;
use elzevir_domain::suspension::SuspensionEnd;
use elzevir_store::CommandFailed;
use oracle::{Context, Library, RecordedRules, submit};
use serde::Deserialize;
use serde_json::{Map, Value};

#[derive(Deserialize)]
struct Recording {
    patron: RecordedPatron,
    checks: Vec<Check>,
}

#[derive(Deserialize)]
struct RecordedPatron {
    category: String,
    library: String,
}

#[derive(Deserialize)]
struct Check {
    name: String,
    rules: Vec<RecordedRules>,
    loans: Vec<RecordedItem>,
    item: RecordedItem,
    answer: Answer,
}

#[derive(Deserialize, Clone)]
struct RecordedItem {
    barcode: String,
    item_type: String,
    home_library: String,
    holding_library: String,
    withdrawn: i64,
    not_for_loan: i64,
    lost: i64,
    restricted: i64,
}

#[derive(Deserialize)]
struct Answer {
    blockers: Map<String, Value>,
    confirms: Map<String, Value>,
    warnings: Map<String, Value>,
}

fn library(dir: &tempfile::TempDir, recording: &Recording, check: &Check) -> Library {
    let mut codes = BTreeSet::new();
    codes.insert((true, recording.patron.library.clone()));
    for item in check.loans.iter().chain([&check.item]) {
        codes.insert((true, item.home_library.clone()));
        codes.insert((true, item.holding_library.clone()));
        codes.insert((false, item.item_type.clone()));
    }
    for rules in &check.rules {
        for (is_branch, code) in [
            (true, &rules.context.library),
            (false, &rules.context.item_type),
        ] {
            if code != "*" {
                codes.insert((is_branch, code.clone()));
            }
        }
    }
    Library::new(
        dir,
        &codes,
        &recording.patron.library,
        &recording.patron.category,
    )
}

fn add_item(library: &Library, item: &RecordedItem) {
    library.add_item(
        &item.barcode,
        (&item.home_library, &item.holding_library),
        &item.item_type,
        ItemStatus {
            not_for_loan: item.not_for_loan,
            damaged: 0,
            lost: item.lost,
            withdrawn: item.withdrawn,
            restricted: item.restricted,
        },
    );
}

fn checkout(
    library: &Library,
    barcode: &str,
    desk: &str,
    confirmed: bool,
) -> Result<CheckedOut, CheckoutRefusal> {
    match submit(
        &library.store,
        WriteCommand::Circulation(CirculationCommand::Checkout(CheckoutRequest {
            patron: library.patron,
            barcode: barcode.to_owned(),
            desk: library.branches[desk],
            on_site: false,
            confirmed,
            at: "2026-11-19T10:00:00-03:00[America/Sao_Paulo]"
                .parse()
                .unwrap(),
        })),
    ) {
        Ok(WriteOutput::Circulation(CirculationOutput::CheckedOut(done))) => Ok(done),
        Err(CommandFailed::Rejected(WriteRefusal::Circulation(CirculationRefusal::Checkout(
            refusal,
        )))) => Err(refusal),
        other => panic!("{other:?}"),
    }
}

/// What Koha would have answered for Elzevir's outcome: the keys of the blockers, of the
/// confirmations and of the warnings, and the loan limit's count and maximum.
#[derive(Debug, Default, PartialEq, Eq)]
struct Keys {
    blockers: BTreeSet<String>,
    confirms: BTreeSet<String>,
    warnings: BTreeSet<String>,
    too_many: Option<(String, u64, String)>,
}

fn reason(reason: TooManyReason) -> &'static str {
    match reason {
        TooManyReason::Checkouts => "TOO_MANY_CHECKOUTS",
        TooManyReason::OnSiteCheckouts => "TOO_MANY_ONSITE_CHECKOUTS",
        TooManyReason::NoRuleDefined => "NO_RULE_DEFINED",
    }
}

fn limit(too_many: &TooMany) -> (String, u64, String) {
    (
        reason(too_many.reason).to_owned(),
        u64::from(too_many.count),
        too_many.max_allowed.clone(),
    )
}

fn elzevir_keys(outcome: &Result<CheckedOut, CheckoutRefusal>) -> Keys {
    let mut keys = Keys::default();
    match outcome {
        Ok(done) => {
            keys.warnings = done
                .alerts
                .iter()
                .map(|alert| format!("{alert:?}"))
                .collect();
        }
        Err(CheckoutRefusal::Impossible(reasons)) => {
            for reason in reasons {
                let key = match reason {
                    Impossible::Suspended(end) => {
                        keys.blockers.insert("DEBARRED".to_owned());
                        match end {
                            SuspensionEnd::Day(_) => "USERBLOCKEDWITHENDDATE",
                            SuspensionEnd::WhenLifted => "USERBLOCKEDNOENDDATE",
                        }
                    }
                    Impossible::PatronExpired => "EXPIRED",
                    Impossible::Debt(_) => "DEBT",
                    Impossible::TooMany(too_many) => {
                        keys.too_many = Some(limit(too_many));
                        "TOO_MANY"
                    }
                    Impossible::NotForLoan(_) => "NOT_FOR_LOAN",
                    Impossible::Withdrawn => "WTHDRAWN",
                    Impossible::Restricted => "RESTRICTED",
                };
                keys.blockers.insert(key.to_owned());
            }
        }
        Err(CheckoutRefusal::NeedsConfirmation(reasons)) => {
            for reason in reasons {
                let key = match reason {
                    Confirm::Renew => "RENEW_ISSUE",
                    Confirm::IssuedToAnother => "ISSUED_TO_ANOTHER",
                    Confirm::Debt(_) => "DEBT",
                    Confirm::PatronCannotBorrow => "PATRON_CANT",
                    Confirm::TooMany(too_many) => {
                        keys.too_many = Some(limit(too_many));
                        "TOO_MANY"
                    }
                    Confirm::NotForLoan(_) => "NOT_FOR_LOAN_FORCING",
                    Confirm::Lost(_) => "ITEM_LOST",
                    Confirm::Held(HeldFor::Waiting) => "RESERVE_WAITING",
                    Confirm::Held(HeldFor::InTransit) => "TRANSFERRED",
                    Confirm::Held(HeldFor::Reserved) => "RESERVED",
                };
                keys.confirms.insert(key.to_owned());
            }
        }
        Err(other) => panic!("{other:?}"),
    }
    keys
}

/// The same from the recorded answer, leaving out what only describes the limit.
fn koha_keys(answer: &Answer) -> Keys {
    let described = |key: &str| {
        key.starts_with("circulation_rule_")
            || key == "current_loan_count"
            || key == "max_loans_allowed"
    };
    let names = |map: &Map<String, Value>| {
        map.keys()
            .filter(|key| !described(key))
            .cloned()
            .collect::<BTreeSet<_>>()
    };
    let too_many = answer
        .blockers
        .get("TOO_MANY")
        .map(|_| &answer.blockers)
        .or_else(|| answer.confirms.get("TOO_MANY").map(|_| &answer.confirms))
        .map(|map| {
            (
                map["TOO_MANY"].as_str().unwrap().to_owned(),
                map["current_loan_count"].as_u64().unwrap(),
                map["max_loans_allowed"]
                    .to_string()
                    .trim_matches('"')
                    .to_owned(),
            )
        });
    Keys {
        blockers: names(&answer.blockers),
        confirms: names(&answer.confirms),
        warnings: names(&answer.warnings),
        too_many,
    }
}

/// The scope of the rule Koha named, when it names one with its context.
fn koha_rule(library: &Library, answer: &Answer) -> Option<RuleScopeKey> {
    let rule = answer
        .confirms
        .get("circulation_rule_TOO_MANY")
        .or_else(|| answer.blockers.get("circulation_rule_TOO_MANY"))?;
    rule.get("rule_name")?;
    let code = |field: &str| rule[field].as_str().unwrap_or("*").to_owned();
    Some(library.scope(&Context {
        library: code("branchcode"),
        category: code("categorycode"),
        item_type: code("itemtype"),
    }))
}

fn replay(recording: &Recording, check: &Check) -> Result<(), String> {
    let dir = tempfile::tempdir().unwrap();
    let library = library(&dir, recording, check);
    let mut added = BTreeSet::new();
    for item in check.loans.iter().chain([&check.item]) {
        if added.insert(item.barcode.clone()) {
            add_item(&library, item);
        }
    }
    library.set_rules(&check.rules)?;
    let desk = &recording.patron.library;
    for loan in &check.loans {
        checkout(&library, &loan.barcode, desk, true)
            .map_err(|refusal| format!("loan {}: {refusal:?}", loan.barcode))?;
    }
    let outcome = checkout(&library, &check.item.barcode, desk, false);
    let (ours, koha) = (elzevir_keys(&outcome), koha_keys(&check.answer));
    if ours != koha {
        return Err(format!("Elzevir {ours:?}, Koha {koha:?}"));
    }
    if let Some(expected) = koha_rule(&library, &check.answer) {
        let found = match &outcome {
            Err(CheckoutRefusal::NeedsConfirmation(reasons)) => {
                reasons.iter().find_map(|reason| match reason {
                    Confirm::TooMany(too_many) => too_many.rule,
                    _ => None,
                })
            }
            _ => None,
        };
        if found != Some(expected) {
            return Err(format!("rule {found:?}, Koha {expected:?}"));
        }
    }
    Ok(())
}

#[test]
fn checkouts_are_answered_as_the_oracle_answered() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scenarios/circulacao/emprestimo.koha.json");
    let recording: Recording =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert!(!recording.checks.is_empty());
    let failures: Vec<String> = recording
        .checks
        .iter()
        .filter_map(|check| {
            replay(&recording, check)
                .err()
                .map(|error| format!("{}: {error}", check.name))
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
