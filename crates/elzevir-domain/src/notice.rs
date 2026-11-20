// SPDX-License-Identifier: GPL-3.0-or-later
//! Overdue notices: the levels of a rule, the level a loan triggers and how the notice
//! goes out (docs/specs/circulacao/avisos.md).

use jiff::civil::Date;
use serde::{Deserialize, Serialize};

use crate::calendar::Calendar;

/// One of the three overdue notices a rule may have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum NoticeLevel {
    /// The first notice.
    First,
    /// The second notice.
    Second,
    /// The third notice.
    Third,
}

impl NoticeLevel {
    /// The level's number, 1 to 3, as stored.
    #[must_use]
    pub const fn number(self) -> u8 {
        match self {
            Self::First => 1,
            Self::Second => 2,
            Self::Third => 3,
        }
    }

    /// The level with this number, if any.
    #[must_use]
    pub const fn from_number(number: i64) -> Option<Self> {
        match number {
            1 => Some(Self::First),
            2 => Some(Self::Second),
            3 => Some(Self::Third),
            _ => None,
        }
    }
}

/// How a notice of a level goes out.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transports {
    /// By e-mail.
    pub email: bool,
    /// On paper, printed by the staff.
    pub print: bool,
}

/// One level of an overdue notice rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverdueLevel {
    /// Which notice.
    pub level: NoticeLevel,
    /// Days overdue on which it goes out.
    pub delay: u16,
    /// How it goes out.
    pub transports: Transports,
    /// Whether the patron is restricted when it goes out, until the staff lift it
    /// (`debarred1` to `debarred3`).
    #[serde(default)]
    pub restrict: bool,
}

/// Why the levels of a rule were refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum LevelsError {
    /// The same level twice.
    #[error("each level may appear once")]
    Repeated,
    /// A level with no days.
    #[error("a level goes out after at least one day")]
    NoDays,
    /// A later level not after an earlier one.
    #[error("each level goes out after the one before")]
    OutOfOrder,
}

/// The levels in order, when each appears once and each goes out after the one before.
///
/// Koha: `tools/overduerules.pl` (`ERRORDELAY`, `ERRORORDER`).
///
/// # Errors
///
/// A repeated level, a level of zero days, or days that do not rise with the levels.
pub fn checked_levels(mut levels: Vec<OverdueLevel>) -> Result<Vec<OverdueLevel>, LevelsError> {
    levels.sort_by_key(|level| level.level);
    if levels.iter().any(|level| level.delay == 0) {
        return Err(LevelsError::NoDays);
    }
    for pair in levels.windows(2) {
        if let [earlier, later] = pair {
            if earlier.level == later.level {
                return Err(LevelsError::Repeated);
            }
            if later.delay <= earlier.delay {
                return Err(LevelsError::OutOfOrder);
            }
        }
    }
    Ok(levels)
}

/// How overdue notices count days.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoticeSettings {
    /// Days the branch is closed do not count, and no notice goes out on them
    /// (`OverdueNoticeCalendar`, off by default).
    pub calendar: bool,
}

/// Whether the branch sends overdue notices on `today`.
///
/// Koha: `overdue_notices.pl` skips a branch closed on the day it runs when
/// `OverdueNoticeCalendar` is on.
#[must_use]
pub fn sends_on(today: Date, calendar: &Calendar, settings: NoticeSettings) -> bool {
    !(settings.calendar && calendar.is_closed(today))
}

/// Days a loan due on `due` is overdue on `today`, or `None` before its due date.
///
/// Koha: `overdue_notices.pl` (`delta_days`, or `Koha::Calendar::days_between`, which takes
/// away the closed days counting both ends, with `OverdueNoticeCalendar`).
#[must_use]
pub fn days_overdue(
    due: Date,
    today: Date,
    calendar: &Calendar,
    settings: NoticeSettings,
) -> Option<i64> {
    if due > today {
        return None;
    }
    let days = i64::from((today - due).get_days());
    if settings.calendar {
        Some(days - calendar.closed_days(due, today))
    } else {
        Some(days)
    }
}

/// The level that goes out for a loan `days` overdue: the one whose days are exactly these.
///
/// Koha: `overdue_notices.pl -t` (`$mindays != $days_between`).
#[must_use]
pub fn triggered(levels: &[OverdueLevel], days: i64) -> Option<&OverdueLevel> {
    levels.iter().find(|level| i64::from(level.delay) == days)
}

/// A way a notice goes out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Transport {
    /// By e-mail.
    Email,
    /// On paper.
    Print,
}

impl Transport {
    /// The name stored with the notice.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Email => "email",
            Self::Print => "print",
        }
    }
}

/// The notices that go out to a patron, with or without an e-mail address: an e-mail to
/// a patron without one goes on paper, and a patron gets one paper notice at most.
///
/// Koha: `overdue_notices.pl` (`$effective_mtt`, `$print_sent`).
#[must_use]
pub fn transports(wanted: Transports, has_email: bool) -> Vec<Transport> {
    let mut out = Vec::new();
    if wanted.email && has_email {
        out.push(Transport::Email);
    }
    if wanted.print || (wanted.email && !has_email) {
        out.push(Transport::Print);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::DatedEntry;
    use jiff::civil::{Weekday, date};

    fn level(level: NoticeLevel, delay: u16) -> OverdueLevel {
        OverdueLevel {
            level,
            delay,
            transports: Transports {
                email: true,
                print: false,
            },
            restrict: false,
        }
    }

    #[test]
    fn levels_come_back_in_order_and_must_rise() {
        let levels = checked_levels(vec![
            level(NoticeLevel::Third, 20),
            level(NoticeLevel::First, 3),
        ])
        .unwrap();
        assert_eq!(levels[0].level, NoticeLevel::First);
        assert_eq!(
            checked_levels(vec![
                level(NoticeLevel::First, 5),
                level(NoticeLevel::Second, 5)
            ]),
            Err(LevelsError::OutOfOrder)
        );
        assert_eq!(
            checked_levels(vec![
                level(NoticeLevel::First, 5),
                level(NoticeLevel::First, 7)
            ]),
            Err(LevelsError::Repeated)
        );
        assert_eq!(
            checked_levels(vec![level(NoticeLevel::Second, 0)]),
            Err(LevelsError::NoDays)
        );
        assert_eq!(checked_levels(Vec::new()), Ok(Vec::new()));
    }

    #[test]
    fn days_are_calendar_days_unless_the_calendar_counts() {
        let calendar = Calendar::always_open().closed_every(Weekday::Sunday);
        let off = NoticeSettings::default();
        let on = NoticeSettings { calendar: true };
        // Due on Friday 2026-11-13; Sunday the 15th is closed.
        let due = date(2026, 11, 13);
        assert_eq!(days_overdue(due, date(2026, 11, 12), &calendar, off), None);
        assert_eq!(days_overdue(due, due, &calendar, off), Some(0));
        assert_eq!(
            days_overdue(due, date(2026, 11, 16), &calendar, off),
            Some(3)
        );
        assert_eq!(
            days_overdue(due, date(2026, 11, 16), &calendar, on),
            Some(2)
        );
    }

    #[test]
    fn a_closed_due_day_counts_as_closed_too() {
        let day = date(2026, 11, 20);
        let calendar = Calendar::always_open().on(day, DatedEntry::Closed);
        let on = NoticeSettings { calendar: true };
        assert_eq!(
            days_overdue(day, date(2026, 11, 23), &calendar, on),
            Some(2)
        );
        assert!(!sends_on(day, &calendar, on));
        assert!(sends_on(day, &calendar, NoticeSettings::default()));
    }

    #[test]
    fn only_the_exact_day_triggers() {
        let levels = [level(NoticeLevel::First, 3), level(NoticeLevel::Second, 10)];
        assert_eq!(
            triggered(&levels, 3).map(|l| l.level),
            Some(NoticeLevel::First)
        );
        assert_eq!(triggered(&levels, 4), None);
        assert_eq!(
            triggered(&levels, 10).map(|l| l.level),
            Some(NoticeLevel::Second)
        );
    }

    #[test]
    fn email_without_address_goes_on_paper_once() {
        let both = Transports {
            email: true,
            print: true,
        };
        assert_eq!(transports(both, true), [Transport::Email, Transport::Print]);
        assert_eq!(transports(both, false), [Transport::Print]);
        let email = Transports {
            email: true,
            print: false,
        };
        assert_eq!(transports(email, false), [Transport::Print]);
        assert!(transports(Transports::default(), true).is_empty());
    }
}
