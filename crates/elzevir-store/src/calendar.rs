// SPDX-License-Identifier: GPL-3.0-or-later
//! `circ.change_calendar` and each branch's calendar (docs/specs/circulacao/calendario.md).

use elzevir_app::calendar::{
    CalendarAction, CalendarBook, CalendarChange, Holiday, HolidayText, HolidayWhen,
    checked_annual, checked_text, checked_weekday, dates,
};
use elzevir_app::circulation_rules::{
    CirculationCommand, CirculationDeskError, CirculationOutput, CirculationRefusal,
};
use elzevir_app::commands::{WriteCommand, WriteOutput, WriteRefusal};
use elzevir_app::staff::StaffId;
use elzevir_domain::calendar::{AnnualHoliday, Calendar, DatedEntry};
use elzevir_domain::catalog::BranchId;
use jiff::civil::{Date, Weekday};
use rusqlite::{Connection, params};

use crate::catalog::exists;
use crate::{CommandFailed, ElzevirStore, StoreError, SubmitError};

type Applied = Result<WriteOutput, CommandFailed<WriteRefusal>>;

const fn refused(refusal: CirculationRefusal) -> CommandFailed<WriteRefusal> {
    CommandFailed::Rejected(WriteRefusal::Circulation(refusal))
}

fn written(result: rusqlite::Result<usize>) -> Result<usize, CommandFailed<WriteRefusal>> {
    result.map_err(|error| StoreError::from(error).into())
}

/// Applies `circ.change_calendar` inside the writer's transaction. Setting an entry that
/// exists replaces it with the new title.
pub(crate) fn change(connection: &Connection, change: &CalendarChange) -> Applied {
    let branch = change.branch.0;
    if !exists(connection, "SELECT 1 FROM branches WHERE id = ?1", branch)? {
        return Err(refused(CirculationRefusal::UnknownBranch));
    }
    let entries = match &change.action {
        CalendarAction::CloseWeekday { weekday, text } => {
            let weekday = checked_weekday(*weekday).map_err(refused)?;
            let text = checked_text(text).map_err(refused)?;
            written(connection.execute(
                "INSERT OR REPLACE INTO calendar_weekly (branch_id, weekday, title, description)
                 VALUES (?1, ?2, ?3, ?4)",
                params![branch, weekday, text.title, text.description],
            ))?
        }
        CalendarAction::CloseAnnually { holiday, text } => {
            let holiday = checked_annual(*holiday).map_err(refused)?;
            let text = checked_text(text).map_err(refused)?;
            written(connection.execute(
                "INSERT OR REPLACE INTO calendar_annual (branch_id, month, day, title, description)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    branch,
                    holiday.month,
                    holiday.day,
                    text.title,
                    text.description
                ],
            ))?
        }
        CalendarAction::SetDates {
            first,
            last,
            entry,
            text,
        } => {
            let days = dates(*first, *last).map_err(refused)?;
            let text = checked_text(text).map_err(refused)?;
            let open = matches!(entry, DatedEntry::Open);
            let mut statement = connection
                .prepare_cached(
                    "INSERT OR REPLACE INTO calendar_dates (branch_id, day, open, title, description)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                )
                .map_err(StoreError::from)?;
            let mut count = 0;
            for day in days {
                count += written(statement.execute(params![
                    branch,
                    day.to_string(),
                    open,
                    text.title,
                    text.description
                ]))?;
            }
            count
        }
        CalendarAction::ReopenWeekday(weekday) => {
            let weekday = checked_weekday(*weekday).map_err(refused)?;
            written(connection.execute(
                "DELETE FROM calendar_weekly WHERE branch_id = ?1 AND weekday = ?2",
                params![branch, weekday],
            ))?
        }
        CalendarAction::RemoveAnnual(holiday) => written(connection.execute(
            "DELETE FROM calendar_annual WHERE branch_id = ?1 AND month = ?2 AND day = ?3",
            params![branch, holiday.month, holiday.day],
        ))?,
        CalendarAction::RemoveDates { first, last } => {
            // Checked like a new range, so that a mistyped range removes nothing.
            dates(*first, *last).map_err(refused)?;
            written(connection.execute(
                "DELETE FROM calendar_dates WHERE branch_id = ?1 AND day BETWEEN ?2 AND ?3",
                params![branch, first.to_string(), last.to_string()],
            ))?
        }
        CalendarAction::CopyTo { to, after } => copy(connection, branch, to.0, *after)?,
    };
    Ok(WriteOutput::Circulation(
        CirculationOutput::CalendarChanged { entries },
    ))
}

/// Copies what `to` does not have yet: `OR IGNORE` keeps the receiving branch's entries.
fn copy(
    connection: &Connection,
    from: i64,
    to: i64,
    after: Date,
) -> Result<usize, CommandFailed<WriteRefusal>> {
    if !exists(connection, "SELECT 1 FROM branches WHERE id = ?1", to)? {
        return Err(refused(CirculationRefusal::UnknownBranch));
    }
    let weekly = written(connection.execute(
        "INSERT OR IGNORE INTO calendar_weekly (branch_id, weekday, title, description)
         SELECT ?2, weekday, title, description FROM calendar_weekly WHERE branch_id = ?1",
        params![from, to],
    ))?;
    let annual = written(connection.execute(
        "INSERT OR IGNORE INTO calendar_annual (branch_id, month, day, title, description)
         SELECT ?2, month, day, title, description FROM calendar_annual WHERE branch_id = ?1",
        params![from, to],
    ))?;
    let dated = written(connection.execute(
        "INSERT OR IGNORE INTO calendar_dates (branch_id, day, open, title, description)
         SELECT ?2, day, open, title, description FROM calendar_dates
         WHERE branch_id = ?1 AND day > ?3",
        params![from, to, after.to_string()],
    ))?;
    Ok(weekly + annual + dated)
}

/// A stored date; a malformed one is a storage failure, never a guess.
fn day(text: &str) -> rusqlite::Result<Date> {
    text.parse().map_err(|error: jiff::Error| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, error.into())
    })
}

fn unavailable(error: impl std::fmt::Display) -> CirculationDeskError {
    CirculationDeskError::Unavailable(error.to_string())
}

/// Every entry of `branch`, in the order [`CalendarBook::holidays`] promises.
fn read_holidays(connection: &Connection, branch: i64) -> rusqlite::Result<Vec<Holiday>> {
    let text = |row: &rusqlite::Row<'_>, at: usize| -> rusqlite::Result<HolidayText> {
        Ok(HolidayText {
            title: row.get(at)?,
            description: row.get(at + 1)?,
        })
    };
    let mut holidays = Vec::new();
    let mut weekly = connection.prepare_cached(
        "SELECT weekday, title, description FROM calendar_weekly
         WHERE branch_id = ?1 ORDER BY weekday",
    )?;
    for holiday in weekly.query_map([branch], |row| {
        Ok(Holiday {
            when: HolidayWhen::Weekly(row.get(0)?),
            text: text(row, 1)?,
        })
    })? {
        holidays.push(holiday?);
    }
    let mut annual = connection.prepare_cached(
        "SELECT month, day, title, description FROM calendar_annual
         WHERE branch_id = ?1 ORDER BY month, day",
    )?;
    for holiday in annual.query_map([branch], |row| {
        Ok(Holiday {
            when: HolidayWhen::Annual(AnnualHoliday {
                month: row.get(0)?,
                day: row.get(1)?,
            }),
            text: text(row, 2)?,
        })
    })? {
        holidays.push(holiday?);
    }
    let mut dated = connection.prepare_cached(
        "SELECT day, open, title, description FROM calendar_dates
         WHERE branch_id = ?1 ORDER BY day",
    )?;
    for holiday in dated.query_map([branch], |row| {
        let entry = if row.get::<_, bool>(1)? {
            DatedEntry::Open
        } else {
            DatedEntry::Closed
        };
        Ok(Holiday {
            when: HolidayWhen::Dated(day(&row.get::<_, String>(0)?)?, entry),
            text: text(row, 2)?,
        })
    })? {
        holidays.push(holiday?);
    }
    Ok(holidays)
}

impl CalendarBook for ElzevirStore {
    fn calendar(&self, branch: BranchId) -> Result<Calendar, CirculationDeskError> {
        let holidays = self.holidays(branch)?;
        let mut calendar = Calendar::always_open();
        for holiday in holidays {
            calendar = match holiday.when {
                HolidayWhen::Weekly(offset) => {
                    let weekday = i8::try_from(offset)
                        .ok()
                        .and_then(|offset| Weekday::from_sunday_zero_offset(offset).ok())
                        .ok_or_else(|| unavailable(format!("stored weekday {offset}")))?;
                    calendar.closed_every(weekday)
                }
                HolidayWhen::Annual(holiday) => calendar.closed_annually(holiday),
                HolidayWhen::Dated(date, entry) => calendar.on(date, entry),
            };
        }
        Ok(calendar)
    }

    fn holidays(&self, branch: BranchId) -> Result<Vec<Holiday>, CirculationDeskError> {
        self.readers()
            .read(|c| read_holidays(c, branch.0))
            .map_err(unavailable)
    }

    fn change(
        &self,
        actor: StaffId,
        change: CalendarChange,
    ) -> Result<CirculationOutput, CirculationDeskError> {
        let pending = self
            .submit_as(
                actor.0,
                WriteCommand::Circulation(CirculationCommand::ChangeCalendar(change)),
            )
            .map_err(|error| match error {
                SubmitError::Busy => CirculationDeskError::Busy,
                SubmitError::Closed => unavailable(error),
            })?;
        match pending.wait() {
            Ok(WriteOutput::Circulation(output)) => Ok(output),
            Ok(_) => Err(unavailable("the writer answered with another result")),
            Err(CommandFailed::Rejected(WriteRefusal::Circulation(refusal))) => {
                Err(CirculationDeskError::Refused(refusal))
            }
            Err(failure) => Err(unavailable(failure)),
        }
    }
}
