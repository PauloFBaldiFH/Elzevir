// SPDX-License-Identifier: GPL-3.0-or-later
//! Koha's circulation rules (`circulation_rules`) for the switch
//! (docs/specs/migracao/troca-do-koha.md, item 7a). The Elzevir's rule kinds are Koha's
//! own names, so a rule comes as it is; its scope is found by Koha's codes at the writer.

use std::collections::BTreeMap;

use elzevir_data::RuleKind;
use elzevir_domain::catalog::{BranchId, PolicyId};
use elzevir_domain::circulation_rules::{RuleScopeKey, RuleValueError, checked_value};
use elzevir_domain::patron::PatronCategoryId;
use serde::{Deserialize, Serialize};

use super::dump::{Row, Table};
use super::{KohaPlan, number};

/// A rule of `circulation_rules`, with its scope as Koha's codes (`None` is "any").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KohaRule {
    /// `id`.
    pub id: u64,
    /// `branchcode`.
    pub branch: Option<String>,
    /// `categorycode`.
    pub category: Option<String>,
    /// `itemtype`, which a circulation policy stands for in the Elzevir.
    pub item_type: Option<String>,
    /// `rule_name`, one of the Elzevir's rule kinds.
    pub name: String,
    /// `rule_value`, already checked as `circ.set_rules` checks it.
    pub value: String,
}

/// Why a rule does not come.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RuleLeftover {
    /// No `id`, or no `rule_name`.
    Incomplete,
    /// A `rule_name` that Koha 26.05 does not have.
    UnknownKind,
    /// A scope with a dimension the kind is not set by.
    OutOfScope,
    /// A value that is too long or, for money, not a plain decimal.
    BadValue,
    /// An empty value for a kind that cannot be blank: Koha deletes such a rule.
    Empty,
    /// A `branchcode` the Elzevir does not have.
    UnknownBranch,
    /// A `categorycode` the Elzevir does not have.
    UnknownCategory,
    /// An `itemtype` the Elzevir does not have as a circulation policy.
    UnknownItemType,
}

/// One write of rules.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KohaRulesBatch {
    /// The rules to set.
    pub rules: Vec<KohaRule>,
}

/// What a write of rules did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KohaRulesMigrated {
    /// Rules set.
    pub rules: usize,
    /// Rules left out at the writer, by `id`.
    pub left: Vec<(u64, RuleLeftover)>,
}

/// A raw cell: the rule value keeps its spaces and its emptiness, which mean something.
fn raw(row: &Row, column: &str) -> Option<String> {
    let value = row.iter().find(|(name, _)| *name == column)?.1.as_deref()?;
    std::str::from_utf8(value).ok().map(str::to_owned)
}

/// A code, `NULL` or empty being "any".
fn code(row: &Row, column: &str) -> Option<String> {
    raw(row, column)
        .map(|code| code.trim().to_owned())
        .filter(|code| !code.is_empty())
}

/// The scope with stand-in ids: the check only asks which dimensions are named.
fn shape(rule: &KohaRule) -> RuleScopeKey {
    RuleScopeKey {
        branch: rule.branch.as_ref().map(|_| BranchId(0)),
        category: rule.category.as_ref().map(|_| PatronCategoryId(0)),
        policy: rule.item_type.as_ref().map(|_| PolicyId(0)),
    }
}

/// The rule as `circ.set_rules` would take it, or why not.
///
/// # Errors
/// The [`RuleLeftover`] that keeps the rule out.
///
/// Koha: `Koha::CirculationRules::set_rule`
pub fn checked(rule: &KohaRule) -> Result<(), RuleLeftover> {
    let kind = RuleKind::from_koha_name(&rule.name).ok_or(RuleLeftover::UnknownKind)?;
    match checked_value(kind, &shape(rule), Some(&rule.value)) {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(RuleLeftover::Empty),
        Err(RuleValueError::OutOfScope(_)) => Err(RuleLeftover::OutOfScope),
        Err(_) => Err(RuleLeftover::BadValue),
    }
}

/// Reads `circulation_rules` into the plan, by `id`.
pub(super) fn read(tables: &BTreeMap<Table, Vec<Row>>, plan: &mut KohaPlan) {
    for row in tables.get(&Table::Rules).map_or(&[][..], Vec::as_slice) {
        let id = number(raw(row, "id").as_deref().map(str::trim)).unwrap_or(0);
        let Some(name) = code(row, "rule_name").filter(|_| id > 0) else {
            plan.rules_left.push((id, RuleLeftover::Incomplete));
            continue;
        };
        let rule = KohaRule {
            id,
            branch: code(row, "branchcode"),
            category: code(row, "categorycode"),
            item_type: code(row, "itemtype"),
            name,
            value: raw(row, "rule_value").unwrap_or_default(),
        };
        match checked(&rule) {
            Ok(()) => plan.rules.push(rule),
            Err(why) => plan.rules_left.push((id, why)),
        }
    }
    plan.rules.sort_by_key(|rule| rule.id);
    plan.rules_left.sort();
}

impl KohaPlan {
    /// The one write of the rules: a library has at most a few thousand.
    #[must_use]
    pub fn rules_batch(&self) -> KohaRulesBatch {
        KohaRulesBatch {
            rules: self.rules.clone(),
        }
    }

    /// The item types the rules name, for the switch to create the policies they need.
    #[must_use]
    pub fn rule_item_types(&self) -> std::collections::BTreeSet<String> {
        self.rules
            .iter()
            .filter_map(|rule| rule.item_type.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The patrons' synthetic dump with Koha's rules table.
    fn plan_with_rules(rows: &str) -> KohaPlan {
        let dump = format!(
            "{}\nCREATE TABLE `circulation_rules` (`id` int(11) NOT NULL, `branchcode` varchar(10), \
             `categorycode` varchar(10), `itemtype` varchar(10), `rule_name` varchar(32) NOT NULL, \
             `rule_value` varchar(32) NOT NULL);\nINSERT INTO `circulation_rules` VALUES {rows};\n",
            include_str!("../../tests/data/koha-patrons.sql")
        );
        let zone = jiff::tz::TimeZone::get("America/Sao_Paulo").unwrap();
        super::super::plan(dump.as_bytes(), jiff::civil::date(2026, 11, 27), &zone).unwrap()
    }

    #[test]
    fn rules_come_as_koha_keeps_them_or_with_the_reason_they_do_not() {
        let plan = plan_with_rules(
            "(1,NULL,NULL,NULL,'issuelength','21'),(2,'CTR','ADU','LIV','issuelength','14'),\
             (3,'CTR',NULL,NULL,'fine',''),(4,NULL,NULL,NULL,'no_such_rule','1'),\
             (5,NULL,NULL,'LIV','max_holds','3'),(6,NULL,NULL,NULL,'fine','1,50'),\
             (7,NULL,NULL,NULL,'issuelength','123456789012345678901234567890123'),\
             (8,NULL,NULL,NULL,'',''),(9,'',NULL,NULL,'fine','0.25'),\
             (10,NULL,NULL,NULL,'hold_fulfillment_policy','')",
        );
        let ids: Vec<u64> = plan.rules.iter().map(|rule| rule.id).collect();
        assert_eq!(ids, vec![1, 2, 3, 9]);
        let second = &plan.rules[1];
        assert_eq!(
            (
                second.branch.as_deref(),
                second.category.as_deref(),
                second.item_type.as_deref()
            ),
            (Some("CTR"), Some("ADU"), Some("LIV"))
        );
        // An empty `branchcode` is "any", and an empty value is kept where it may be blank.
        assert_eq!(plan.rules[3].branch, None);
        assert_eq!(plan.rules[2].value, "");
        assert_eq!(
            plan.rules_left,
            vec![
                (4, RuleLeftover::UnknownKind),
                (5, RuleLeftover::OutOfScope),
                (6, RuleLeftover::BadValue),
                (7, RuleLeftover::BadValue),
                (8, RuleLeftover::Incomplete),
                (10, RuleLeftover::Empty),
            ]
        );
        assert_eq!(
            plan.rule_item_types(),
            std::collections::BTreeSet::from(["LIV".to_owned()])
        );
    }
}
