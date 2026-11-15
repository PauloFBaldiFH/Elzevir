// SPDX-License-Identifier: GPL-3.0-or-later
//! The registries of branches, shelving places, collections and circulation policies
//! (docs/specs/catalogo/bibframe-completo.md, item 23d).

use askama::Template;

use crate::I18n;

/// The page of the registries.
#[derive(Template)]
#[template(path = "staff_registries.html")]
pub struct StaffRegistries<'a> {
    /// Interface texts.
    pub t: &'a I18n,
    /// Anti-forgery token of the session, for every form.
    pub csrf: &'a str,
    /// Branches, in name order.
    pub branches: Vec<BranchPart>,
    /// Circulation policies, in name order.
    pub policies: Vec<RegistryRow>,
    /// Whether the last change was stored.
    pub saved: bool,
    /// Message key of why the last change was refused.
    pub refused: Option<&'static str>,
}

/// One branch with its contact data and its lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchPart {
    /// The branch.
    pub row: RegistryRow,
    /// Postal address.
    pub address: String,
    /// Phone.
    pub phone: String,
    /// E-mail.
    pub email: String,
    /// Web page.
    pub url: String,
    /// Its shelving places, in name order.
    pub sublocations: Vec<RegistryRow>,
    /// Its collections, in name order.
    pub collections: Vec<RegistryRow>,
}

/// One row: its id and name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryRow {
    /// The id.
    pub id: i64,
    /// The name.
    pub name: String,
}
