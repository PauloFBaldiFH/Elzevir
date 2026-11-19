// SPDX-License-Identifier: GPL-3.0-or-later
//! Circulation rules: where each value comes from, and what a value may be
//! (docs/specs/circulacao/regras.md).
//!
//! A rule is a value for one [`RuleKind`] at one scope: a branch, a patron category and a
//! circulation policy, each of them given or left as "any". Koha's third dimension is the
//! item type; in Elzevir it is the circulation policy the Item carries.

use elzevir_data::{RuleKind, RuleScope};

use crate::catalog::{BranchId, PolicyId};
use crate::patron::PatronCategoryId;

/// Longest rule value (Koha: `circulation_rules.rule_value`, `varchar(32)`).
pub const MAX_RULE_VALUE: usize = 32;

/// Where a rule applies; `None` is "any" (Koha's `*`).
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct RuleScopeKey {
    /// The branch.
    pub branch: Option<BranchId>,
    /// The patron category.
    pub category: Option<PatronCategoryId>,
    /// The circulation policy of the Item.
    pub policy: Option<PolicyId>,
}

impl RuleScopeKey {
    /// Whether a rule set at `self` applies to a loan at `at`: every dimension the rule
    /// names is the one asked for, and a dimension asked as "any" takes only rules for any.
    #[must_use]
    pub fn covers(&self, at: &Self) -> bool {
        fn fits<T: PartialEq + Copy>(rule: Option<T>, asked: Option<T>) -> bool {
            rule.is_none() || rule == asked
        }
        fits(self.branch, at.branch)
            && fits(self.category, at.category)
            && fits(self.policy, at.policy)
    }

    /// How specific the scope is, branch first, then category, then policy: the order in
    /// which a more specific rule wins.
    const fn specificity(&self) -> (bool, bool, bool) {
        (
            self.branch.is_some(),
            self.category.is_some(),
            self.policy.is_some(),
        )
    }

    /// Whether the dimension is given.
    #[must_use]
    pub const fn names(&self, dimension: RuleScope) -> bool {
        match dimension {
            RuleScope::Branch => self.branch.is_some(),
            RuleScope::PatronCategory => self.category.is_some(),
            RuleScope::ItemType => self.policy.is_some(),
        }
    }
}

/// One stored rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CirculationRule {
    /// What it sets.
    pub kind: RuleKind,
    /// Where it applies.
    pub scope: RuleScopeKey,
    /// The value as stored; an empty text is a value, not an absence.
    pub value: String,
}

/// The rule of `kind` that applies at `at`: among the rules whose scope covers `at`, the
/// one with a branch wins over any without, then one with a category, then one with a
/// policy. The rule returned carries its scope, which says where the value came from.
///
/// Koha: `Koha::CirculationRules::get_effective_rule` (ordered by branch, category and
/// item type, descending, so that `NULL`, "any", comes last).
#[must_use]
pub fn effective_rule<'a>(
    rules: &'a [CirculationRule],
    kind: RuleKind,
    at: &RuleScopeKey,
) -> Option<&'a CirculationRule> {
    rules
        .iter()
        .filter(|rule| rule.kind == kind && rule.scope.covers(at))
        .max_by_key(|rule| rule.scope.specificity())
}

/// Why a rule value cannot be stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RuleValueError {
    /// The kind is not set per this dimension, so the scope must leave it as "any".
    #[error("this rule is not set per {0:?}")]
    OutOfScope(RuleScope),
    /// An amount of money that is not digits with an optional decimal part.
    #[error("not an amount of money")]
    NotDecimal,
    /// Longer than [`MAX_RULE_VALUE`] characters.
    #[error("the value is too long")]
    TooLong,
}

/// The value to store for `kind` at `scope`: `None` removes the rule. An empty value
/// removes it too when the kind cannot be blank.
///
/// # Errors
///
/// [`RuleValueError`] when the scope names a dimension the kind is not set by, a
/// monetary value is not a plain decimal, or the value is too long.
///
/// Koha: `Koha::CirculationRules::set_rule`.
pub fn checked_value(
    kind: RuleKind,
    scope: &RuleScopeKey,
    value: Option<&str>,
) -> Result<Option<String>, RuleValueError> {
    for dimension in [
        RuleScope::Branch,
        RuleScope::PatronCategory,
        RuleScope::ItemType,
    ] {
        if scope.names(dimension) && !kind.is_scoped_by(dimension) {
            return Err(RuleValueError::OutOfScope(dimension));
        }
    }
    let info = kind.info();
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_empty() && !info.can_be_blank {
        return Ok(None);
    }
    if value.chars().count() > MAX_RULE_VALUE {
        return Err(RuleValueError::TooLong);
    }
    if info.is_monetary && !value.is_empty() && !is_plain_decimal(value) {
        return Err(RuleValueError::NotDecimal);
    }
    Ok(Some(value.to_owned()))
}

/// `^\d+(\.\d+)?$`.
fn is_plain_decimal(value: &str) -> bool {
    let (whole, fraction) = match value.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (value, None),
    };
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    digits(whole) && fraction.is_none_or(digits)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(branch: Option<i64>, category: Option<i64>, policy: Option<i64>) -> RuleScopeKey {
        RuleScopeKey {
            branch: branch.map(BranchId),
            category: category.map(PatronCategoryId),
            policy: policy.map(PolicyId),
        }
    }

    fn rule(scope: RuleScopeKey, value: &str) -> CirculationRule {
        CirculationRule {
            kind: RuleKind::Issuelength,
            scope,
            value: value.to_owned(),
        }
    }

    #[test]
    fn a_branch_wins_over_a_category_and_a_category_over_a_policy() {
        let rules = [
            rule(at(None, None, None), "21"),
            rule(at(None, Some(2), None), "7"),
            rule(at(None, None, Some(3)), "3"),
            rule(at(Some(1), None, Some(3)), "2"),
            rule(at(None, Some(2), Some(3)), "5"),
            rule(at(Some(1), None, None), "14"),
        ];
        let value = |key| {
            effective_rule(&rules, RuleKind::Issuelength, &key).map(|rule| rule.value.as_str())
        };
        assert_eq!(value(at(Some(1), Some(2), Some(3))), Some("2"));
        assert_eq!(value(at(Some(1), Some(2), Some(4))), Some("14"));
        assert_eq!(value(at(None, Some(2), Some(3))), Some("5"));
        assert_eq!(value(at(None, Some(2), None)), Some("7"));
        assert_eq!(value(at(None, None, None)), Some("21"));
        assert_eq!(value(at(Some(9), Some(9), Some(9))), Some("21"));
        assert_eq!(
            effective_rule(&rules, RuleKind::Fine, &at(None, None, None)),
            None
        );
    }

    #[test]
    fn values_follow_the_kind() {
        let any = RuleScopeKey::default();
        assert_eq!(
            checked_value(RuleKind::Fine, &any, Some("1.50")),
            Ok(Some("1.50".to_owned()))
        );
        assert_eq!(
            checked_value(RuleKind::Fine, &any, Some("1,50")),
            Err(RuleValueError::NotDecimal)
        );
        assert_eq!(
            checked_value(RuleKind::Fine, &any, Some(".5")),
            Err(RuleValueError::NotDecimal)
        );
        assert_eq!(
            checked_value(RuleKind::Fine, &any, Some("")),
            Ok(Some(String::new()))
        );
        // A kind that cannot be blank is removed by an empty value.
        assert_eq!(
            checked_value(RuleKind::Holdallowed, &any, Some("")),
            Ok(None)
        );
        assert_eq!(
            checked_value(RuleKind::Note, &any, Some(&"x".repeat(33))),
            Err(RuleValueError::TooLong)
        );
        // `max_holds` is set per branch and category, never per policy.
        assert_eq!(
            checked_value(RuleKind::MaxHolds, &at(None, None, Some(1)), Some("3")),
            Err(RuleValueError::OutOfScope(RuleScope::ItemType))
        );
    }
}
