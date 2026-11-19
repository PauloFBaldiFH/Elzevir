// SPDX-License-Identifier: GPL-3.0-or-later
//! Circulation rules set and read through the real store (docs/specs/circulacao/regras.md).
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "integration test: helper functions fail the test on error"
)]

use elzevir_app::catalog::{CatalogCommand, CatalogOutput};
use elzevir_app::circulation_rules::{
    CirculationCommand, CirculationOutput, CirculationRefusal, RuleBook, RuleChange, RuleKind,
};
use elzevir_app::commands::{WriteCommand, WriteOutput, WriteRefusal};
use elzevir_app::registries::{RegistryChange, RegistryEntry};
use elzevir_domain::catalog::{BranchId, PolicyId};
use elzevir_domain::circulation_rules::{RuleScopeKey, effective_rule};
use elzevir_domain::patron::PatronCategoryId;
use elzevir_store::{CommandFailed, ElzevirHandler, ElzevirStore, StoreConfig};

fn open(dir: &tempfile::TempDir) -> ElzevirStore {
    ElzevirStore::open(
        &dir.path().join("elzevir.db"),
        &StoreConfig::default(),
        ElzevirHandler,
    )
    .unwrap()
}

fn catalog(store: &ElzevirStore, command: CatalogCommand) -> CatalogOutput {
    let WriteOutput::Catalog(output) = store
        .submit(WriteCommand::Catalog(command))
        .unwrap()
        .wait()
        .unwrap()
    else {
        panic!("not a catalog answer");
    };
    output
}

fn set(
    store: &ElzevirStore,
    scope: RuleScopeKey,
    values: &[(&str, Option<&str>)],
) -> Result<CirculationOutput, CommandFailed<WriteRefusal>> {
    store
        .submit(WriteCommand::Circulation(CirculationCommand::SetRules(
            RuleChange {
                scope,
                values: values
                    .iter()
                    .map(|(name, value)| ((*name).to_owned(), value.map(str::to_owned)))
                    .collect(),
            },
        )))
        .unwrap()
        .wait()
        .map(|output| {
            let WriteOutput::Circulation(output) = output else {
                panic!("not a circulation answer");
            };
            output
        })
}

fn library(dir: &tempfile::TempDir) -> (ElzevirStore, BranchId, PolicyId) {
    let store = open(dir);
    let CatalogOutput::Branch(branch) = catalog(
        &store,
        CatalogCommand::CreateBranch {
            name: "Central".to_owned(),
        },
    ) else {
        panic!("no branch");
    };
    let CatalogOutput::Policy(policy) = catalog(
        &store,
        CatalogCommand::CreatePolicy {
            name: "Empréstimo curto".to_owned(),
        },
    ) else {
        panic!("no policy");
    };
    (store, branch, policy)
}

#[test]
fn rules_are_set_replaced_and_removed_and_the_most_specific_applies() {
    let dir = tempfile::tempdir().unwrap();
    let (store, branch, policy) = library(&dir);
    let any = RuleScopeKey::default();
    let short = RuleScopeKey {
        policy: Some(policy),
        ..any
    };
    assert_eq!(
        set(
            &store,
            any,
            &[
                ("issuelength", Some("14")),
                ("fine", Some("1.00")),
                ("finedays", Some(""))
            ]
        )
        .unwrap(),
        CirculationOutput::RulesSet {
            stored: 3,
            removed: 0
        }
    );
    set(&store, short, &[("issuelength", Some("3"))]).unwrap();
    set(&store, any, &[("issuelength", Some("21"))]).unwrap();
    // A kind that cannot be blank is removed by an empty value.
    set(&store, any, &[("holdallowed", Some("from_any_library"))]).unwrap();
    assert_eq!(
        set(&store, any, &[("holdallowed", Some("")), ("fine", None)]).unwrap(),
        CirculationOutput::RulesSet {
            stored: 0,
            removed: 2
        }
    );
    let rules = store.rules().unwrap();
    assert_eq!(rules.len(), 3);
    let value =
        |at: &RuleScopeKey, kind| effective_rule(&rules, kind, at).map(|rule| rule.value.clone());
    let loan = RuleScopeKey {
        branch: Some(branch),
        category: None,
        policy: Some(policy),
    };
    assert_eq!(value(&loan, RuleKind::Issuelength).as_deref(), Some("3"));
    assert_eq!(value(&any, RuleKind::Issuelength).as_deref(), Some("21"));
    assert_eq!(value(&loan, RuleKind::Finedays).as_deref(), Some(""));
    assert_eq!(value(&loan, RuleKind::Fine), None);
    // The policy has a rule now, so it stays.
    let deleted = store
        .submit(WriteCommand::Catalog(CatalogCommand::ChangeRegistry(
            RegistryChange::Delete(RegistryEntry::Policy(policy)),
        )))
        .unwrap()
        .wait();
    assert!(
        matches!(deleted, Err(CommandFailed::Rejected(_))),
        "{deleted:?}"
    );
}

#[test]
fn unknown_rules_scopes_and_values_are_refused_whole() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _, policy) = library(&dir);
    let refused = |scope, values: &[(&str, Option<&str>)]| match set(&store, scope, values) {
        Err(CommandFailed::Rejected(WriteRefusal::Circulation(refusal))) => refusal,
        other => panic!("not refused: {other:?}"),
    };
    let any = RuleScopeKey::default();
    assert_eq!(
        refused(
            any,
            &[("issuelength", Some("14")), ("loan_days", Some("1"))]
        ),
        CirculationRefusal::UnknownRule("loan_days".to_owned())
    );
    assert!(matches!(
        refused(any, &[("fine", Some("R$ 1"))]),
        CirculationRefusal::BadValue { .. }
    ));
    let nowhere = RuleScopeKey {
        category: Some(PatronCategoryId(9)),
        ..any
    };
    assert_eq!(
        refused(nowhere, &[("issuelength", Some("14"))]),
        CirculationRefusal::UnknownCategory
    );
    // `patron_maxissueqty` is per branch and category, never per policy.
    let per_policy = RuleScopeKey {
        policy: Some(policy),
        ..any
    };
    assert!(matches!(
        refused(per_policy, &[("patron_maxissueqty", Some("5"))]),
        CirculationRefusal::BadValue { .. }
    ));
    assert!(store.rules().unwrap().is_empty());
}
