// SPDX-License-Identifier: GPL-3.0-or-later
//! The code tables and their names (docs/specs/catalogo/tabelas-de-codigos.md, items 14 to
//! 19).

use askama::Template;

use crate::I18n;

/// The page of the code tables.
#[derive(Template)]
#[template(path = "staff_codes.html")]
pub struct StaffCodes<'a> {
    /// Interface texts.
    pub t: &'a I18n,
    /// Anti-forgery token of the session, for every form.
    pub csrf: &'a str,
    /// The four tables.
    pub tables: Vec<CodeTablePart>,
    /// Whether the last change was stored.
    pub saved: bool,
    /// Message key of why the last change was refused, with what was typed.
    pub refused: Option<&'static str>,
}

/// One table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeTablePart {
    /// Its name in the form and in anchors.
    pub name: &'static str,
    /// Message key of its heading.
    pub heading_key: &'static str,
    /// Its codes, in code order.
    pub rows: Vec<CodeRow>,
}

/// One code and its name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeRow {
    /// The code.
    pub code: String,
    /// The name.
    pub name: String,
}
