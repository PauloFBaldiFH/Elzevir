// SPDX-License-Identifier: GPL-3.0-or-later
//! The reports screen (docs/specs/operacao/relatorios.md, items 10 to 13).

use askama::Template;

use crate::I18n;

/// The reports page.
#[derive(Template)]
#[template(path = "staff_reports.html")]
pub struct StaffReports<'a> {
    /// Interface texts.
    pub t: &'a I18n,
    /// The report asked for: `circulacao`, `acervo` or `leitores`; empty for none.
    pub report: &'a str,
    /// The first day of the period, as typed.
    pub from: &'a str,
    /// The last day of the period, as typed.
    pub to: &'a str,
    /// The day the active patrons are counted from, as typed.
    pub since: &'a str,
    /// Message key of why the report was refused.
    pub refused: Option<&'static str>,
    /// The report read, when one was.
    pub table: Option<ReportTable>,
    /// The same report as CSV.
    pub csv_url: Option<String>,
}

/// A report as a table: its headers and rows, each cell already written out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReportTable {
    /// The column headers.
    pub headers: Vec<String>,
    /// The rows, the last one the totals.
    pub rows: Vec<Vec<String>>,
}
