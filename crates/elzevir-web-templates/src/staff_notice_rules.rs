// SPDX-License-Identifier: GPL-3.0-or-later
//! The notices screen (docs/specs/circulacao/avisos.md, items 28 to 30).

use askama::Template;

use crate::{Choice, I18n};

/// The overdue notice rules, the one open, and the hold notice preferences.
#[derive(Template)]
#[template(path = "staff_notice_rules.html")]
pub struct StaffNoticeRules<'a> {
    /// Interface texts.
    pub t: &'a I18n,
    /// Anti-forgery token of the session.
    pub csrf: &'a str,
    /// Every overdue notice rule, "any branch" first.
    pub rules: Vec<NoticeRuleLine>,
    /// Branches to choose from, `0` for "any" first.
    pub branches: Vec<Choice>,
    /// Patron categories to choose from.
    pub categories: Vec<Choice>,
    /// The three levels of the open rule.
    pub levels: Vec<NoticeLevelRow>,
    /// One row per patron category for the hold notice.
    pub holds: Vec<HoldNoticeRow>,
    /// Whether the last change was stored.
    pub saved: bool,
    /// Why the last change was refused, as shown.
    pub refused: Option<String>,
}

/// One overdue notice rule of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoticeRuleLine {
    /// Where it opens.
    pub url: String,
    /// Branch name, or "any".
    pub branch: String,
    /// Patron category name.
    pub category: String,
    /// Each level in words.
    pub levels: Vec<String>,
    /// Whether it is the rule open below.
    pub current: bool,
}

/// One level of the open rule, as stored or typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoticeLevelRow {
    /// 1 to 3.
    pub number: u8,
    /// Days overdue, empty for no level.
    pub delay: String,
    /// Sent by e-mail.
    pub email: bool,
    /// Sent on paper.
    pub print: bool,
    /// Restricts the patron.
    pub restrict: bool,
}

/// How one patron category hears of a waiting hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoldNoticeRow {
    /// Category id, the name of its fields.
    pub category: i64,
    /// Category name.
    pub name: String,
    /// By e-mail.
    pub email: bool,
    /// On paper.
    pub print: bool,
}

/// The printed notices pending, ready to print (docs/specs/circulacao/avisos.md, items 31
/// to 33).
#[derive(Template)]
#[template(path = "staff_print_notices.html")]
pub struct StaffPrintNotices<'a> {
    /// Interface texts.
    pub t: &'a I18n,
    /// Anti-forgery token of the session.
    pub csrf: &'a str,
    /// The notices, oldest first.
    pub notices: Vec<PrintedNotice>,
    /// How many were just marked as sent, as shown.
    pub marked: Option<String>,
}

/// One notice to print.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintedNotice {
    /// Notice id, sent back when marking.
    pub id: i64,
    /// The patron's name.
    pub name: String,
    /// The patron's postal address.
    pub address: Option<String>,
    /// The subject.
    pub subject: String,
    /// The text, with line breaks.
    pub text: String,
}
