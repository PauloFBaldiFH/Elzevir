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
}

impl StaffBar<'_> {
    /// The hint key of the selected scope, for the search box placeholder.
    #[must_use]
    pub fn hint_key(&self) -> &'static str {
        self.scopes
            .iter()
            .find(|scope| scope.selected)
            .map_or("bar-query", |scope| scope.hint_key)
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
    /// Where it goes.
    pub href: &'static str,
}
