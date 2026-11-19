// SPDX-License-Identifier: GPL-3.0-or-later
//! The circulation rules screen (docs/specs/circulacao/regras.md, items 8 to 13).

use askama::Template;

use crate::I18n;

/// The overview of the scopes with rules and the rules of one scope.
#[derive(Template)]
#[template(path = "staff_circulation_rules.html")]
pub struct StaffCirculationRules<'a> {
    /// Interface texts.
    pub t: &'a I18n,
    /// Anti-forgery token of the session.
    pub csrf: &'a str,
    /// Message keys of the overview's rule columns.
    pub overview_columns: Vec<String>,
    /// Every scope with rules.
    pub scopes: Vec<ScopeLine>,
    /// Branches to choose from, `0` for "any" first.
    pub branches: Vec<Choice>,
    /// Patron categories to choose from, `0` for "any" first.
    pub categories: Vec<Choice>,
    /// Circulation policies to choose from, `0` for "any" first.
    pub policies: Vec<Choice>,
    /// The rules of the chosen scope, in groups.
    pub groups: Vec<RuleGroupPart>,
    /// Whether the last change was stored.
    pub saved: bool,
    /// Why the last change was refused, as shown.
    pub refused: Option<String>,
}

/// One entry of a scope list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    /// Id, `0` for "any".
    pub id: i64,
    /// Name as shown.
    pub name: String,
    /// Whether it is the one chosen.
    pub chosen: bool,
}

/// One scope of the overview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeLine {
    /// Where its rules open.
    pub url: String,
    /// Branch name, or "any".
    pub branch: String,
    /// Patron category name, or "any".
    pub category: String,
    /// Circulation policy name, or "any".
    pub policy: String,
    /// Values of the overview's columns, empty where unset.
    pub values: Vec<String>,
    /// How many rules, as shown.
    pub count: String,
    /// Whether it is the scope open below.
    pub current: bool,
}

/// A group of rules, such as loans or holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleGroupPart {
    /// Message key of its heading.
    pub label_key: &'static str,
    /// Its rules.
    pub rows: Vec<RuleRow>,
}

/// One rule kind at the chosen scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleRow {
    /// Rule name (Koha's `rule_name`), the name of its form fields.
    pub name: &'static str,
    /// Label as shown.
    pub label: String,
    /// The value set at this scope, or as typed.
    pub value: String,
    /// Whether an empty value is kept on purpose.
    pub blank: bool,
    /// Whether an empty value may be kept.
    pub can_be_blank: bool,
    /// Why this scope cannot set it, as shown; `None` when it can.
    pub not_scoped: Option<String>,
    /// The value that applies here, as shown.
    pub effective: String,
    /// Where that value comes from, as shown.
    pub origin: String,
}
