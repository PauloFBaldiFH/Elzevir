// SPDX-License-Identifier: GPL-3.0-or-later
//! The staff header: scoped search, the "New" menu and the theme switch (interface §22).

use askama::Template;

use crate::I18n;

/// The staff header, loaded once per page into every staff screen.
#[derive(Template)]
#[template(path = "staff_bar.html")]
pub struct StaffBar<'a> {
    /// Interface texts.
    pub t: &'a I18n,
    /// Signed-in login.
    pub login: &'a str,
    /// Anti-forgery token for the sign-out form.
    pub csrf: &'a str,
    /// The search scopes this operator may use, in registry order.
    pub scopes: Vec<BarScope>,
    /// What the "New" menu may create, in registry order; the menu is left out when empty.
    pub new_entries: Vec<BarLink>,
    /// The main modules, for the drawer.
    pub modules: Vec<BarLink>,
    /// The screens that exist today, listed in the drawer under the modules.
    pub tools: Vec<BarLink>,
}

impl StaffBar<'_> {
    /// The scope the page starts with.
    #[must_use]
    pub fn selected(&self) -> Option<&BarScope> {
        self.scopes.iter().find(|scope| scope.selected)
    }

    /// The hint key of the selected scope, for the search box placeholder.
    #[must_use]
    pub fn hint_key(&self) -> &'static str {
        self.selected().map_or("bar-query", |scope| scope.hint_key)
    }

    /// Up to two letters of the login for the avatar, upper case.
    #[must_use]
    pub fn initials(&self) -> String {
        self.login
            .chars()
            .filter(|c| c.is_alphanumeric())
            .take(2)
            .flat_map(char::to_uppercase)
            .collect()
    }
}

/// One search scope of the header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BarScope {
    /// The value sent as `escopo`.
    pub key: &'static str,
    /// Message key of the name.
    pub label_key: &'static str,
    /// Message key of what to type.
    pub hint_key: &'static str,
    /// Icon symbol in the sprite.
    pub icon: &'static str,
    /// Domain tone (`circulation`, `catalog`, `patrons` or `admin`).
    pub tone: &'static str,
    /// The letter used with Alt, lower case.
    pub shortcut: char,
    /// Whether it is the scope the page starts with.
    pub selected: bool,
    /// Whether its screen exists; a scope that is not ready shows as a disabled placeholder.
    pub ready: bool,
    /// Whether it searches or acts on an Item.
    pub kind: ScopeKind,
}

/// One entry of the "New" menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BarLink {
    /// Message key of the name.
    pub label_key: &'static str,
    /// Icon symbol in the sprite.
    pub icon: &'static str,
    /// Domain tone.
    pub tone: &'static str,
    /// Where it goes; `None` for a placeholder of a screen that does not exist yet.
    pub href: Option<&'static str>,
}

/// What a scope does, which decides its tone and the divider before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeKind {
    /// It searches (patrons, catalog, authorities, repositories).
    Search,
    /// It acts on an Item (checkin, renewal), drawn in the circulation tone.
    Operation,
    /// The first operation after the searches, with a divider before it.
    FirstOperation,
}

impl ScopeKind {
    /// Whether it acts on an Item.
    #[must_use]
    pub fn is_operation(self) -> bool {
        self != Self::Search
    }

    /// Whether a divider goes before it.
    #[must_use]
    pub fn divider_before(self) -> bool {
        self == Self::FirstOperation
    }
}
