// SPDX-License-Identifier: GPL-3.0-or-later
//! Each branch's opening calendar: closed weekdays, annual holidays and dated entries
//! (docs/specs/circulacao/calendario.md).

use elzevir_domain::calendar::{AnnualHoliday, Calendar, DatedEntry};
use elzevir_domain::catalog::BranchId;
use jiff::ToSpan;
use jiff::civil::Date;
use serde::{Deserialize, Serialize};

use crate::circulation_rules::{CirculationDeskError, CirculationOutput, CirculationRefusal};
use crate::staff::StaffId;

/// Longest title (Koha: `special_holidays.title`, `varchar(50)`).
pub const MAX_TITLE: usize = 50;

/// Longest description.
pub const MAX_DESCRIPTION: usize = 2000;

/// Most days one dated change may cover.
pub const MAX_RANGE_DAYS: i32 = 366;

/// What a closing is called.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HolidayText {
    /// Short name, such as "Christmas".
    pub title: String,
    /// Longer note, if any.
    pub description: Option<String>,
}

/// One change to a branch's calendar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalendarChange {
    /// The branch whose calendar changes.
    pub branch: BranchId,
    /// What changes.
    pub action: CalendarAction,
}

/// What a calendar change does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CalendarAction {
    /// Closed every week on this weekday, `0` for Sunday (Koha's numbering).
    CloseWeekday {
        /// The weekday, `0` for Sunday.
        weekday: u8,
        /// Its name.
        text: HolidayText,
    },
    /// Closed every year on this day.
    CloseAnnually {
        /// Month and day.
        holiday: AnnualHoliday,
        /// Its name.
        text: HolidayText,
    },
    /// Every date from `first` to `last` closed, or open as an exception.
    SetDates {
        /// First date.
        first: Date,
        /// Last date, the same as `first` for one day.
        last: Date,
        /// Closed, or open despite a weekly or annual closing.
        entry: DatedEntry,
        /// Its name.
        text: HolidayText,
    },
    /// No longer closed every week on this weekday.
    ReopenWeekday(u8),
    /// No longer closed every year on this day.
    RemoveAnnual(AnnualHoliday),
    /// No dated entry from `first` to `last`.
    RemoveDates {
        /// First date.
        first: Date,
        /// Last date.
        last: Date,
    },
    /// Copies the calendar to another branch: every weekly and annual entry, and the dated
    /// entries after `after`, each one the other branch does not have yet.
    ///
    /// Koha: `C4::Calendar::copy_to_branch`, with `after` as today.
    CopyTo {
        /// The branch that receives the entries.
        to: BranchId,
        /// Dated entries on or before this date stay behind.
        after: Date,
    },
}

/// When a closing applies, as listed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HolidayWhen {
    /// Every week, `0` for Sunday.
    Weekly(u8),
    /// Every year.
    Annual(AnnualHoliday),
    /// One date, closed or open.
    Dated(Date, DatedEntry),
}

/// One stored entry of a calendar, with its name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holiday {
    /// When it applies.
    pub when: HolidayWhen,
    /// Its name.
    pub text: HolidayText,
}

/// The text as stored: the title trimmed, an empty description left out.
///
/// # Errors
///
/// [`CirculationRefusal::BadHolidayTitle`] for an empty or long title or a long description.
pub fn checked_text(text: &HolidayText) -> Result<HolidayText, CirculationRefusal> {
    let title = text.title.trim();
    let description = text
        .description
        .as_deref()
        .map(str::trim)
        .filter(|description| !description.is_empty());
    if title.is_empty()
        || title.chars().count() > MAX_TITLE
        || description.is_some_and(|description| description.chars().count() > MAX_DESCRIPTION)
    {
        return Err(CirculationRefusal::BadHolidayTitle);
    }
    Ok(HolidayText {
        title: title.to_owned(),
        description: description.map(str::to_owned),
    })
}

/// Every date from `first` to `last`.
///
/// # Errors
///
/// [`CirculationRefusal::BadDates`] when `last` is before `first` or the range is longer
/// than [`MAX_RANGE_DAYS`].
pub fn dates(first: Date, last: Date) -> Result<Vec<Date>, CirculationRefusal> {
    let mut all = Vec::new();
    let mut day = first;
    while day <= last {
        if all.len() >= usize::try_from(MAX_RANGE_DAYS).unwrap_or(usize::MAX) {
            return Err(CirculationRefusal::BadDates);
        }
        all.push(day);
        day = day
            .checked_add(1.day())
            .map_err(|_| CirculationRefusal::BadDates)?;
    }
    if all.is_empty() {
        return Err(CirculationRefusal::BadDates);
    }
    Ok(all)
}

/// The weekday, when it is one.
///
/// # Errors
///
/// [`CirculationRefusal::BadWeekday`] past Saturday.
pub const fn checked_weekday(weekday: u8) -> Result<u8, CirculationRefusal> {
    if weekday <= 6 {
        Ok(weekday)
    } else {
        Err(CirculationRefusal::BadWeekday)
    }
}

/// The annual holiday, when the day exists.
///
/// # Errors
///
/// [`CirculationRefusal::BadDates`] for a day such as 31 April.
pub fn checked_annual(holiday: AnnualHoliday) -> Result<AnnualHoliday, CirculationRefusal> {
    AnnualHoliday::new(holiday.month, holiday.day).ok_or(CirculationRefusal::BadDates)
}

/// Each branch's calendar, for the due date and the fines, and for the calendar screen.
pub trait CalendarBook: Send + Sync {
    /// The calendar of `branch`; every day is open in a branch without entries.
    ///
    /// # Errors
    ///
    /// When storage fails.
    fn calendar(&self, branch: BranchId) -> Result<Calendar, CirculationDeskError>;

    /// The entries of `branch` with their names: weekly, then annual, then dated, each in
    /// order.
    ///
    /// # Errors
    ///
    /// When storage fails.
    fn holidays(&self, branch: BranchId) -> Result<Vec<Holiday>, CirculationDeskError>;

    /// Runs `circ.change_calendar` as `actor`.
    ///
    /// # Errors
    ///
    /// [`CirculationDeskError::Refused`] with the refusal, or why storage failed.
    fn change(
        &self,
        actor: StaffId,
        change: CalendarChange,
    ) -> Result<CirculationOutput, CirculationDeskError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    #[test]
    fn titles_are_trimmed_and_bounded() {
        let text = |title: &str| HolidayText {
            title: title.to_owned(),
            description: Some("  ".to_owned()),
        };
        assert_eq!(
            checked_text(&text("  Natal ")),
            Ok(HolidayText {
                title: "Natal".to_owned(),
                description: None
            })
        );
        assert_eq!(
            checked_text(&text(" ")),
            Err(CirculationRefusal::BadHolidayTitle)
        );
        assert_eq!(
            checked_text(&text(&"x".repeat(51))),
            Err(CirculationRefusal::BadHolidayTitle)
        );
    }

    #[test]
    fn ranges_run_forward_and_are_bounded() {
        assert_eq!(
            dates(date(2026, 12, 24), date(2026, 12, 26)),
            Ok(vec![
                date(2026, 12, 24),
                date(2026, 12, 25),
                date(2026, 12, 26)
            ])
        );
        assert_eq!(
            dates(date(2026, 12, 26), date(2026, 12, 24)),
            Err(CirculationRefusal::BadDates)
        );
        assert_eq!(
            dates(date(2026, 1, 1), date(2026, 12, 31)).map(|d| d.len()),
            Ok(365)
        );
        assert_eq!(
            dates(date(2026, 1, 1), date(2027, 1, 2)),
            Err(CirculationRefusal::BadDates)
        );
        assert_eq!(checked_weekday(7), Err(CirculationRefusal::BadWeekday));
        assert_eq!(
            checked_annual(AnnualHoliday { month: 4, day: 31 }),
            Err(CirculationRefusal::BadDates)
        );
    }
}
