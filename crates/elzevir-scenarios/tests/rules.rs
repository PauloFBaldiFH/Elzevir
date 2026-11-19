// SPDX-License-Identifier: GPL-3.0-or-later
//! The rule Elzevir resolves is the one the Koha oracle resolved, for every kind and every
//! combination of branch, patron category and item type recorded
//! (docs/specs/circulacao/regras.md; `scenarios/circulacao/regras-efetivas.koha.json`).
//!
//! The recording names Koha's codes; here they become generic ids, as the MARC edge does:
//! no Koha code reaches the domain.

#![expect(clippy::unwrap_used, reason = "tests")]

use std::collections::BTreeMap;
use std::path::Path;

use elzevir_data::RuleKind;
use elzevir_domain::catalog::{BranchId, PolicyId};
use elzevir_domain::circulation_rules::{CirculationRule, RuleScopeKey, effective_rule};
use elzevir_domain::patron::PatronCategoryId;
use serde::Deserialize;

#[derive(Deserialize)]
struct Recording {
    raw: Vec<RawRules>,
    effective: Vec<Effective>,
}

#[derive(Deserialize)]
struct RawRules {
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

#[derive(Deserialize)]
struct Effective {
    library: String,
    category: String,
    item_type: String,
    rules: BTreeMap<String, String>,
}

/// Koha codes to ids, `*` to "any"; a new code gets the next id.
#[derive(Default)]
struct Ids(BTreeMap<String, i64>);

impl Ids {
    fn of(&mut self, code: &str) -> Option<i64> {
        if code == "*" {
            return None;
        }
        let next = i64::try_from(self.0.len()).unwrap() + 1;
        Some(*self.0.entry(code.to_owned()).or_insert(next))
    }
}

struct Codes {
    branches: Ids,
    categories: Ids,
    policies: Ids,
}

impl Codes {
    fn key(&mut self, branch: &str, category: &str, item_type: &str) -> RuleScopeKey {
        RuleScopeKey {
            branch: self.branches.of(branch).map(BranchId),
            category: self.categories.of(category).map(PatronCategoryId),
            policy: self.policies.of(item_type).map(PolicyId),
        }
    }
}

#[test]
fn every_effective_rule_matches_the_oracle() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scenarios/circulacao/regras-efetivas.koha.json");
    let recording: Recording =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut codes = Codes {
        branches: Ids::default(),
        categories: Ids::default(),
        policies: Ids::default(),
    };
    let mut rules = Vec::new();
    for set in &recording.raw {
        let scope = codes.key(
            &set.context.library,
            &set.context.category,
            &set.context.item_type,
        );
        for (name, value) in &set.rules {
            rules.push(CirculationRule {
                kind: RuleKind::from_koha_name(name).unwrap(),
                scope,
                value: value.clone(),
            });
        }
    }
    let mut compared = 0;
    let mut differences = Vec::new();
    for case in &recording.effective {
        let at = codes.key(&case.library, &case.category, &case.item_type);
        for kind in RuleKind::ALL {
            let ours = effective_rule(&rules, kind, &at).map(|rule| rule.value.as_str());
            let koha = case.rules.get(kind.koha_name()).map(String::as_str);
            compared += 1;
            if ours != koha {
                differences.push(format!(
                    "{} {} {} {}: Elzevir {ours:?}, Koha {koha:?}",
                    case.library,
                    case.category,
                    case.item_type,
                    kind.koha_name()
                ));
            }
        }
    }
    assert_eq!(compared, 24 * RuleKind::ALL.len());
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}
