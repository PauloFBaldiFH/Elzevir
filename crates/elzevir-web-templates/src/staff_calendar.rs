// SPDX-License-Identifier: GPL-3.0-or-later
//! The calendar screen of a branch (docs/specs/circulacao/calendario.md, items 9 to 14).

use askama::Template;

use crate::{Choice, I18n};

/// One month of a branch's calendar, its entries and the forms that change them.
#[derive(Template)]
#[template(path = "staff_calendar.html")]
pub struct StaffCalendar<'a> {
    /// Interface texts.
    pub t: &'a I18n,
    /// Anti-forgery token of the session.
    pub csrf: &'a str,
    /// Branches to choose from; the one shown is chosen.
    pub branches: Vec<Choice>,
    /// Id of the branch shown.
    pub branch: i64,
    /// The month shown, `YYYY-MM`.
    pub month: String,
    /// The month as a heading, such as "November 2026".
    pub month_title: String,
    /// Addresses of the month before and after.
    pub previous_url: String,
    /// Address of the month after.
    pub next_url: String,
    /// Names of the weekdays, Sunday first.
    pub weekdays: Vec<String>,
    /// The weeks of the month, Sunday first; days of other months are empty.
    pub weeks: Vec<Vec<DayCell>>,
    /// Closed weekdays.
    pub weekly: Vec<EntryLine>,
    /// Annual holidays.
    pub annual: Vec<EntryLine>,
    /// Dated entries from the first day of the month on.
    pub dated: Vec<EntryLine>,
    /// Months to choose from, `1` January first.
    pub months: Vec<String>,
    /// Whether the last change was stored.
    pub saved: bool,
    /// Why the last change was refused, as shown.
    pub refused: Option<String>,
}

/// One day of the month grid.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DayCell {
    /// Day of the month; `0` for a cell outside the month.
    pub day: i8,
    /// `YYYY-MM-DD`, empty outside the month.
    pub date: String,
    /// Whether the branch is closed.
    pub closed: bool,
    /// Whether the day is open as an exception.
    pub exception: bool,
    /// Whether it is today.
    pub today: bool,
    /// Title of the entry that decides the day, if any.
    pub title: String,
}

/// One stored entry as listed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryLine {
    /// When, as shown: a weekday, a day and month, or a date.
    pub when: String,
    /// The values the remove form sends: `name=value` pairs.
    pub key: Vec<(&'static str, String)>,
    /// Its title.
    pub title: String,
    /// Its description, empty when none.
    pub description: String,
    /// Whether it is an open exception.
    pub exception: bool,
}
