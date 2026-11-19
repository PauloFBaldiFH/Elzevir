// SPDX-License-Identifier: GPL-3.0-or-later
//! Each checkout the Koha oracle answered is answered the same by `circ.checkout`
//! (docs/specs/circulacao/emprestimo.md; `scenarios/circulacao/emprestimo.koha.json`).
//!
//! The recording names Koha's codes; here they name the test's branches and policies at
//! the MARC edge, as an import would. No Koha code reaches the domain.

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "tests: a missing code or field fails the test"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use elzevir_app::catalog::{
    CatalogCommand, CatalogOutput, CatalogReader, ItemData, ItemStatus, NewInstance, NewItem,
    NewRecord, RecordItems,
};
use elzevir_app::circulation_rules::{
    CirculationCommand, CirculationOutput, CirculationRefusal, RuleChange,
};
use elzevir_app::codes::{CodeTable, NamedCode};
use elzevir_app::commands::{WriteCommand, WriteOutput, WriteRefusal};
use elzevir_app::loans::{CheckedOut, CheckoutRefusal, CheckoutRequest};
use elzevir_app::patrons::{
    CategoryChange, PatronCommand, PatronFields, PatronOutput, ReadingHistory,
};
use elzevir_domain::catalog::{BranchId, InstanceId, PolicyId};
use elzevir_domain::checkout::{Confirm, Impossible, TooMany, TooManyReason};
use elzevir_domain::circulation_rules::RuleScopeKey;
use elzevir_domain::patron::{Enrolment, PatronCategoryId, PatronId};
use elzevir_domain::suspension::SuspensionEnd;
use elzevir_store::{CommandFailed, ElzevirHandler, ElzevirStore, StoreConfig};
use jiff::civil::date;
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

#[derive(Deserialize)]
struct RecordedRules {
    context: Context,
    rules: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct Context {
    #[serde(rename = "library_id")]
    library: String,
    #[serde(rename = "patron_category_id")]
    category: String,
    #[serde(rename = "item_type_id")]
    item_type: String,
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

fn submit(
    store: &ElzevirStore,
    command: WriteCommand,
) -> Result<WriteOutput, CommandFailed<WriteRefusal>> {
    store.submit(command).unwrap().wait()
}

/// A store holding what the oracle held for one check.
struct Library {
    store: ElzevirStore,
    branches: BTreeMap<String, BranchId>,
    policies: BTreeMap<String, PolicyId>,
    category: PatronCategoryId,
    patron: PatronId,
    instance: InstanceId,
}

impl Library {
    fn new(dir: &tempfile::TempDir, recording: &Recording, check: &Check) -> Self {
        let store = ElzevirStore::open(
            &dir.path().join("elzevir.db"),
            &StoreConfig::default(),
            ElzevirHandler,
        )
        .unwrap();
        let items = check.loans.iter().chain([&check.item]);
        let mut codes = BTreeSet::new();
        codes.insert((true, recording.patron.library.clone()));
        for item in items {
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
        for (is_branch, code) in codes {
            let table = if is_branch {
                CodeTable::Branch
            } else {
                CodeTable::ItemType
            };
            submit(
                &store,
                WriteCommand::Catalog(CatalogCommand::NameCodes(vec![NamedCode {
                    table,
                    name: code.clone(),
                    code,
                }])),
            )
            .unwrap();
        }
        let lists = store.codes().unwrap();
        let branches = lists
            .branches
            .iter()
            .map(|branch| (branch.name.clone(), branch.id))
            .collect::<BTreeMap<_, _>>();
        let policies = lists
            .policies
            .iter()
            .map(|policy| (policy.name.clone(), policy.id))
            .collect();
        let Ok(WriteOutput::Patrons(PatronOutput::CategoryChanged(Some(category)))) = submit(
            &store,
            WriteCommand::Patrons(PatronCommand::ChangeCategory(CategoryChange::Create {
                name: recording.patron.category.clone(),
                enrolment: Enrolment::Months(12),
            })),
        ) else {
            panic!("no category");
        };
        let patron = add_patron(&store, branches[&recording.patron.library], category);
        let instance = add_record(&store);
        Self {
            store,
            branches,
            policies,
            category,
            patron,
            instance,
        }
    }

    fn scope<'a>(&self, context: &'a Context) -> RuleScopeKey {
        let any = |code: &'a str| (code != "*").then_some(code);
        RuleScopeKey {
            branch: any(&context.library).map(|code| self.branches[code]),
            category: any(&context.category).map(|_| self.category),
            policy: any(&context.item_type).map(|code| self.policies[code]),
        }
    }

    fn add_item(&self, item: &RecordedItem) {
        submit(
            &self.store,
            WriteCommand::Catalog(CatalogCommand::AddItem(Box::new(NewItem {
                instance: self.instance,
                item: ItemData {
                    barcode: Some(item.barcode.clone()),
                    inventory_number: None,
                    home_branch: item.home_library.clone(),
                    holding_branch: item.holding_library.clone(),
                    item_type: item.item_type.clone(),
                    call_number: None,
                    location: None,
                    collection: None,
                    price: None,
                    replacement_price: None,
                    copy_number: None,
                    status: ItemStatus {
                        not_for_loan: item.not_for_loan,
                        damaged: 0,
                        lost: item.lost,
                        withdrawn: item.withdrawn,
                        restricted: item.restricted,
                    },
                    acquired_on: None,
                },
            }))),
        )
        .unwrap();
    }

    fn checkout(
        &self,
        barcode: &str,
        desk: &str,
        confirmed: bool,
    ) -> Result<CheckedOut, CheckoutRefusal> {
        match submit(
            &self.store,
            WriteCommand::Circulation(CirculationCommand::Checkout(CheckoutRequest {
                patron: self.patron,
                barcode: barcode.to_owned(),
                desk: self.branches[desk],
                on_site: false,
                confirmed,
                at: "2026-11-19T10:00:00-03:00[America/Sao_Paulo]"
                    .parse()
                    .unwrap(),
            })),
        ) {
            Ok(WriteOutput::Circulation(CirculationOutput::CheckedOut(done))) => Ok(done),
            Err(CommandFailed::Rejected(WriteRefusal::Circulation(
                CirculationRefusal::Checkout(refusal),
            ))) => Err(refusal),
            other => panic!("{other:?}"),
        }
    }
}

fn add_patron(store: &ElzevirStore, branch: BranchId, category: PatronCategoryId) -> PatronId {
    let Ok(WriteOutput::Patrons(PatronOutput::Added(patron))) = submit(
        store,
        WriteCommand::Patrons(PatronCommand::Add(Box::new(PatronFields {
            cardnumber: "000001".to_owned(),
            surname: "Leitora".to_owned(),
            first_name: None,
            cpf: None,
            date_of_birth: None,
            email: None,
            phone: None,
            address: None,
            branch,
            category,
            enrolled: date(2026, 1, 1),
            expires: None,
            staff_note: None,
            reading_history: ReadingHistory::NotKept,
        }))),
    ) else {
        panic!("no patron");
    };
    patron
}

fn add_record(store: &ElzevirStore) -> InstanceId {
    let Ok(WriteOutput::Catalog(CatalogOutput::Record { instance, .. })) = submit(
        store,
        WriteCommand::Catalog(CatalogCommand::AddRecord(Box::new(NewRecord {
            work_title: "Obra".to_owned(),
            work_description: elzevir_domain::statement::Description::default(),
            hub: None,
            relations: Vec::new(),
            classification: None,
            classification_scheme: None,
            summary: None,
            audience: None,
            languages: Vec::new(),
            contributions: Vec::new(),
            subjects: Vec::new(),
            items: RecordItems::default(),
            source: None,
            instance: NewInstance {
                title: "Obra".to_owned(),
                ..NewInstance::default()
            },
        }))),
    ) else {
        panic!("no record");
    };
    instance
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
                    Confirm::PatronCannotBorrow => "PATRON_CANT",
                    Confirm::TooMany(too_many) => {
                        keys.too_many = Some(limit(too_many));
                        "TOO_MANY"
                    }
                    Confirm::NotForLoan(_) => "NOT_FOR_LOAN_FORCING",
                    Confirm::Lost(_) => "ITEM_LOST",
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
    let library = Library::new(&dir, recording, check);
    let mut added = BTreeSet::new();
    for item in check.loans.iter().chain([&check.item]) {
        if added.insert(item.barcode.clone()) {
            library.add_item(item);
        }
    }
    for rules in &check.rules {
        let values = rules
            .rules
            .iter()
            .map(|(name, value)| (name.clone(), Some(value.clone())))
            .collect();
        submit(
            &library.store,
            WriteCommand::Circulation(CirculationCommand::SetRules(RuleChange {
                scope: library.scope(&rules.context),
                values,
            })),
        )
        .map_err(|error| format!("rules {:?}: {error:?}", rules.rules))?;
    }
    let desk = &recording.patron.library;
    for loan in &check.loans {
        library
            .checkout(&loan.barcode, desk, true)
            .map_err(|refusal| format!("loan {}: {refusal:?}", loan.barcode))?;
    }
    let outcome = library.checkout(&check.item.barcode, desk, false);
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
