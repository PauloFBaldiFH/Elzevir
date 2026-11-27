// SPDX-License-Identifier: GPL-3.0-or-later
//! The circulation rules of the dump against the Elzevir
//! (docs/specs/migracao/verificacao-do-koha.md, item 18).

use std::collections::BTreeSet;

use crate::koha_migration::{KohaPlan, KohaRule};

use super::{Cause, Footprint, RuleKey, Table, TableCheck, Unexplained, sorted};

fn rule_key(rule: &KohaRule) -> RuleKey {
    (
        rule.branch.clone(),
        rule.category.clone(),
        rule.item_type.clone(),
        rule.name.clone(),
    )
}

/// How a rule the dump lacks is named: its kind and its scope, `*` for "any".
fn named(key: &RuleKey) -> String {
    let any = |code: &Option<String>| code.clone().unwrap_or_else(|| "*".to_owned());
    format!(
        "{} ({}, {}, {})",
        key.3,
        any(&key.0),
        any(&key.1),
        any(&key.2)
    )
}

pub(super) fn rules(plan: &KohaPlan, found: &Footprint) -> TableCheck {
    let mut check = TableCheck::new(Table::Rules);
    for (id, why) in &plan.rules_left {
        check.in_dump += 1;
        check.explain(Cause::Rule(*why), &id.to_string());
    }
    let mut seen = BTreeSet::new();
    for rule in &plan.rules {
        check.in_dump += 1;
        let number = rule.id.to_string();
        let key = rule_key(rule);
        if let Some(value) = found.rules.get(&key) {
            check.arrived += 1;
            if *value != rule.value {
                check.differ(&number, Unexplained::Field("rule_value"));
            }
        } else {
            let missing = |code: &Option<String>, known: &dyn Fn(&String) -> bool| {
                code.as_ref().filter(|code| !known(code)).cloned()
            };
            if let Some(code) = missing(&rule.branch, &|code| found.branches.contains_key(code)) {
                check.explain(Cause::UnknownBranch(code), &number);
            } else if missing(&rule.category, &|code| found.categories.contains(code)).is_some() {
                check.explain(Cause::UnknownCode("categorycode"), &number);
            } else if missing(&rule.item_type, &|code| found.item_types.contains(code)).is_some() {
                check.explain(Cause::UnknownCode("itemtype"), &number);
            } else {
                check.differ(&number, Unexplained::Missing);
            }
        }
        seen.insert(key);
    }
    for key in found.rules.keys().filter(|key| !seen.contains(*key)) {
        check.differ(&named(key), Unexplained::ExtraBy("rule"));
    }
    sorted(check)
}
