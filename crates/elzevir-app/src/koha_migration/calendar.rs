// SPDX-License-Identifier: GPL-3.0-or-later
//! Koha's calendar (`repeatable_holidays` and `special_holidays`) for the switch
//! (docs/specs/migracao/troca-do-koha.md, item 7b). Each row becomes an entry of the same
//! branch's calendar, as `circ.change_calendar` sets it; nothing becomes a default.

use std::collections::{BTreeMap, BTreeSet};

use elzevir_domain::calendar::{AnnualHoliday, DatedEntry};
use jiff::civil::Date;
use serde::{Deserialize, Serialize};

use super::dump::{Row, Table};
use super::{KohaPlan, number, text};
use crate::calendar::{CalendarAction, HolidayText, HolidayWhen, MAX_DESCRIPTION, MAX_TITLE};

/// Most calendar entries in one write command.
pub const HOLIDAYS_PER_COMMAND: usize = 5_000;

/// The title of an entry that Koha keeps without one and without a description.
pub const UNTITLED: &str = "(Koha)";

/// The Koha table a calendar row comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum HolidayTable {
    /// `repeatable_holidays`: weekly and annual closings.
    Repeatable,
    /// `special_holidays`: dated closings and open exceptions.
    Special,
}

/// A row of Koha's calendar, ready for the writer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KohaHoliday {
    /// The table it comes from.
    pub table: HolidayTable,
    /// `id`.
    pub id: u64,
    /// `branchcode`.
    pub branch: String,
    /// The entry, as `circ.change_calendar` takes it.
    pub action: CalendarAction,
    /// Koha has no title for it: the title comes from the description, or is [`UNTITLED`].
    pub untitled: bool,
    /// The description was longer than [`MAX_DESCRIPTION`] and comes cut.
    pub cut: bool,
}

impl KohaHoliday {
    /// When the entry applies, which names it in its branch's calendar; the plan builds
    /// only closings and dated entries, so `None` never comes from it.
    #[must_use]
    pub const fn when(&self) -> Option<HolidayWhen> {
        match &self.action {
            CalendarAction::CloseWeekday { weekday, .. } => Some(HolidayWhen::Weekly(*weekday)),
            CalendarAction::CloseAnnually { holiday, .. } => Some(HolidayWhen::Annual(*holiday)),
            CalendarAction::SetDates { first, entry, .. } => {
                Some(HolidayWhen::Dated(*first, *entry))
            }
            _ => None,
        }
    }

    /// Its title and description.
    #[must_use]
    pub fn text(&self) -> Option<&HolidayText> {
        match &self.action {
            CalendarAction::CloseWeekday { text, .. }
            | CalendarAction::CloseAnnually { text, .. }
            | CalendarAction::SetDates { text, .. } => Some(text),
            _ => None,
        }
    }
}

/// How an entry is named in its branch's calendar, open or closed alike: `weekday 0`,
/// `annual 12-25` or the date.
#[must_use]
pub fn holiday_name(when: &HolidayWhen) -> String {
    match when {
        HolidayWhen::Weekly(weekday) => format!("weekday {weekday}"),
        HolidayWhen::Annual(holiday) => format!("annual {:02}-{:02}", holiday.month, holiday.day),
        HolidayWhen::Dated(date, _) => date.to_string(),
    }
}

/// Why a calendar row does not come.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum HolidayLeftover {
    /// No `id` or `branchcode`, or neither a weekday nor a day and month.
    Incomplete,
    /// A weekday past 6.
    BadWeekday,
    /// A day that does not exist, such as 31 April.
    BadDate,
    /// An earlier row of the branch names the same day; Koha reads them as one.
    SameDay,
    /// A `branchcode` the Elzevir does not have.
    UnknownBranch,
}

/// One write of calendar entries.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KohaCalendarBatch {
    /// The entries to set.
    pub holidays: Vec<KohaHoliday>,
}

/// What a write of calendar entries did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KohaCalendarMigrated {
    /// Entries set.
    pub holidays: usize,
    /// Rows left out at the writer.
    pub left: Vec<(HolidayTable, u64, HolidayLeftover)>,
}

/// A raw cell, kept as it is.
fn raw(row: &Row, column: &str) -> Option<String> {
    let value = row.iter().find(|(name, _)| *name == column)?.1.as_deref()?;
    std::str::from_utf8(value).ok().map(str::to_owned)
}

/// A small whole number, `None` when absent or not one.
fn small(row: &Row, column: &str) -> Option<i64> {
    text(row, column)?.parse().ok()
}

/// The text the Elzevir keeps: Koha allows an empty title, the Elzevir does not, so the
/// description lends its start, or [`UNTITLED`] stands in; a long description is cut.
fn holiday_text(row: &Row) -> (HolidayText, bool, bool) {
    let description = raw(row, "description")
        .map(|description| description.trim().to_owned())
        .filter(|description| !description.is_empty());
    let cut = description
        .as_ref()
        .is_some_and(|description| description.chars().count() > MAX_DESCRIPTION);
    let description =
        description.map(|description| description.chars().take(MAX_DESCRIPTION).collect());
    let title = raw(row, "title")
        .map(|title| title.trim().to_owned())
        .filter(|title| !title.is_empty());
    let untitled = title.is_none();
    let title = if let Some(title) = title {
        title.chars().take(MAX_TITLE).collect()
    } else {
        let lent = description
            .as_deref()
            .and_then(|description: &str| description.lines().next())
            .map_or_else(String::new, |line| {
                line.trim().chars().take(MAX_TITLE).collect::<String>()
            });
        let lent = lent.trim();
        if lent.is_empty() {
            UNTITLED.to_owned()
        } else {
            lent.to_owned()
        }
    };
    (HolidayText { title, description }, untitled, cut)
}

/// A weekly or annual closing.
///
/// Koha: `Koha::Calendar::_init`: a row with a weekday is weekly, whatever its day and
/// month; one without is annual.
fn repeatable(row: &Row) -> Result<(u64, String, CalendarAction, bool, bool), HolidayLeftover> {
    let id = number(text(row, "id").as_deref()).ok_or(HolidayLeftover::Incomplete)?;
    let branch = text(row, "branchcode").ok_or(HolidayLeftover::Incomplete)?;
    let (text, untitled, cut) = holiday_text(row);
    let action = if let Some(weekday) = small(row, "weekday") {
        let weekday = u8::try_from(weekday)
            .ok()
            .filter(|weekday| *weekday <= 6)
            .ok_or(HolidayLeftover::BadWeekday)?;
        CalendarAction::CloseWeekday { weekday, text }
    } else {
        let (Some(month), Some(day)) = (small(row, "month"), small(row, "day")) else {
            return Err(HolidayLeftover::Incomplete);
        };
        let holiday = i8::try_from(month)
            .ok()
            .zip(i8::try_from(day).ok())
            .and_then(|(month, day)| AnnualHoliday::new(month, day))
            .ok_or(HolidayLeftover::BadDate)?;
        CalendarAction::CloseAnnually { holiday, text }
    };
    Ok((id, branch, action, untitled, cut))
}

/// Whether Perl reads the cell as true: anything but NULL, empty or `0`.
fn perl_true(row: &Row, column: &str) -> bool {
    raw(row, column).is_some_and(|value| !matches!(value.trim(), "" | "0"))
}

/// A dated closing or open exception.
fn special(row: &Row) -> Result<(u64, String, Date, bool, Row), HolidayLeftover> {
    let id = number(text(row, "id").as_deref()).ok_or(HolidayLeftover::Incomplete)?;
    let branch = text(row, "branchcode").ok_or(HolidayLeftover::Incomplete)?;
    let (Some(year), Some(month), Some(day)) =
        (small(row, "year"), small(row, "month"), small(row, "day"))
    else {
        return Err(HolidayLeftover::Incomplete);
    };
    let date = i16::try_from(year)
        .ok()
        .zip(i8::try_from(month).ok())
        .zip(i8::try_from(day).ok())
        .and_then(|((year, month), day)| Date::new(year, month, day).ok())
        .ok_or(HolidayLeftover::BadDate)?;
    Ok((id, branch, date, perl_true(row, "isexception"), row.clone()))
}

/// Reads both calendar tables into the plan, by `id`.
pub(super) fn read(tables: &BTreeMap<Table, Vec<Row>>, plan: &mut KohaPlan) {
    let rows = |table| tables.get(&table).map_or(&[][..], Vec::as_slice);
    let mut repeatables = Vec::new();
    for row in rows(Table::RepeatableHolidays) {
        match repeatable(row) {
            Ok(entry) => repeatables.push(entry),
            Err(why) => plan.holidays_left.push((
                HolidayTable::Repeatable,
                number(text(row, "id").as_deref()).unwrap_or(0),
                why,
            )),
        }
    }
    repeatables.sort_by_key(|entry| entry.0);
    let mut seen = BTreeSet::new();
    for (id, branch, action, untitled, cut) in repeatables {
        let holiday = KohaHoliday {
            table: HolidayTable::Repeatable,
            id,
            branch,
            action,
            untitled,
            cut,
        };
        if seen.insert((
            holiday.branch.clone(),
            holiday.when().map(|when| holiday_name(&when)),
        )) {
            plan.holidays.push(holiday);
        } else {
            plan.holidays_left
                .push((HolidayTable::Repeatable, id, HolidayLeftover::SameDay));
        }
    }
    read_special(rows(Table::SpecialHolidays), plan);
    plan.holidays_left.sort();
}

/// A dated row: its `id`, whether it opens the day, and its cells.
type DatedRow = (u64, bool, Row);

/// The dated rows, one entry per branch and date.
///
/// Koha: `Koha::Calendar::_holidays`: the rows of a date are read as one, open when any of
/// them is an exception (`MAX(isexception)`).
fn read_special(rows: &[Row], plan: &mut KohaPlan) {
    let mut by_day: BTreeMap<(String, Date), Vec<DatedRow>> = BTreeMap::new();
    for row in rows {
        match special(row) {
            Ok((id, branch, date, open, row)) => {
                by_day
                    .entry((branch, date))
                    .or_default()
                    .push((id, open, row));
            }
            Err(why) => plan.holidays_left.push((
                HolidayTable::Special,
                number(text(row, "id").as_deref()).unwrap_or(0),
                why,
            )),
        }
    }
    let mut kept = Vec::new();
    for ((branch, date), mut same) in by_day {
        same.sort_by_key(|(id, ..)| *id);
        let open = same.iter().any(|(_, open, _)| *open);
        // The first row in the state Koha reads names the entry; the others join it.
        let Some(first) = same.iter().position(|(_, row_open, _)| *row_open == open) else {
            continue;
        };
        for (index, (id, ..)) in same.iter().enumerate() {
            if index != first {
                plan.holidays_left
                    .push((HolidayTable::Special, *id, HolidayLeftover::SameDay));
            }
        }
        let Some((id, _, row)) = same.get(first) else {
            continue;
        };
        let (text, untitled, cut) = holiday_text(row);
        kept.push(KohaHoliday {
            table: HolidayTable::Special,
            id: *id,
            branch,
            action: CalendarAction::SetDates {
                first: date,
                last: date,
                entry: if open {
                    DatedEntry::Open
                } else {
                    DatedEntry::Closed
                },
                text,
            },
            untitled,
            cut,
        });
    }
    kept.sort_by_key(|holiday| holiday.id);
    plan.holidays.extend(kept);
}

impl KohaPlan {
    /// The writes of the calendar, at most [`HOLIDAYS_PER_COMMAND`] entries each.
    #[must_use]
    pub fn calendar_batches(&self) -> Vec<KohaCalendarBatch> {
        self.holidays
            .chunks(HOLIDAYS_PER_COMMAND)
            .map(|holidays| KohaCalendarBatch {
                holidays: holidays.to_vec(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The patrons' synthetic dump with Koha's calendar tables.
    fn plan_with_calendar(repeatable: &str, special: &str) -> KohaPlan {
        let dump = format!(
            "{}\nCREATE TABLE `repeatable_holidays` (`id` int(11) NOT NULL, \
             `branchcode` varchar(10) NOT NULL, `weekday` smallint(6), `day` smallint(6), \
             `month` smallint(6), `title` varchar(50) NOT NULL, `description` mediumtext NOT NULL);\n\
             INSERT INTO `repeatable_holidays` VALUES {repeatable};\n\
             CREATE TABLE `special_holidays` (`id` int(11) NOT NULL, \
             `branchcode` varchar(10) NOT NULL, `day` smallint(6) NOT NULL, \
             `month` smallint(6) NOT NULL, `year` smallint(6) NOT NULL, \
             `isexception` smallint(1) NOT NULL, `title` varchar(50) NOT NULL, \
             `description` mediumtext NOT NULL);\nINSERT INTO `special_holidays` VALUES {special};\n",
            include_str!("../../tests/data/koha-patrons.sql")
        );
        let zone = jiff::tz::TimeZone::get("America/Sao_Paulo").unwrap();
        super::super::plan(dump.as_bytes(), jiff::civil::date(2026, 11, 27), &zone).unwrap()
    }

    fn names(plan: &KohaPlan) -> Vec<(u64, String, String)> {
        plan.holidays
            .iter()
            .map(|holiday| {
                (
                    holiday.id,
                    holiday.branch.clone(),
                    holiday_name(&holiday.when().unwrap()),
                )
            })
            .collect()
    }

    #[test]
    fn koha_calendar_rows_become_entries_of_their_branch() {
        let long = "x".repeat(MAX_DESCRIPTION + 5);
        let plan = plan_with_calendar(
            &format!(
                "(1,'CTR',0,NULL,NULL,'Domingo',''),(2,'CTR',NULL,25,12,'Natal',''),\
                 (3,'CTR',7,NULL,NULL,'Dia sete',''),(4,'CTR',NULL,31,4,'Abril',''),\
                 (5,'CTR',0,1,1,'',''),(6,'CTR',NULL,NULL,NULL,'Nada',''),\
                 (7,'BIB',NULL,29,2,'','Bissexto\\nsegunda linha'),(8,'',1,NULL,NULL,'X',''),\
                 (9,'BIB',6,NULL,NULL,'Sábado','{long}')"
            ),
            "(1,'CTR',20,2,2026,0,'Carnaval',''),(2,'CTR',20,2,2026,1,'Aberto',''),\
             (3,'CTR',20,2,2026,1,'De novo',''),(4,'CTR',31,2,2026,0,'Nunca',''),\
             (5,'BIB',3,4,2026,0,'',''),(6,'CTR',NULL,4,2026,0,'Sem dia','')",
        );
        assert_eq!(
            names(&plan),
            vec![
                (1, "CTR".to_owned(), "weekday 0".to_owned()),
                (2, "CTR".to_owned(), "annual 12-25".to_owned()),
                (7, "BIB".to_owned(), "annual 02-29".to_owned()),
                (9, "BIB".to_owned(), "weekday 6".to_owned()),
                (2, "CTR".to_owned(), "2026-02-20".to_owned()),
                (5, "BIB".to_owned(), "2026-04-03".to_owned()),
            ]
        );
        // The exception wins, named by its first row, and the closed row joins it.
        assert_eq!(
            plan.holidays[4].when(),
            Some(HolidayWhen::Dated(
                jiff::civil::date(2026, 2, 20),
                DatedEntry::Open
            ))
        );
        assert_eq!(plan.holidays[4].text().unwrap().title, "Aberto");
        // An empty title borrows the description's first line, or stands in.
        assert_eq!(plan.holidays[2].text().unwrap().title, "Bissexto");
        assert!(plan.holidays[2].untitled);
        assert_eq!(plan.holidays[5].text().unwrap().title, UNTITLED);
        assert!(plan.holidays[5].untitled && !plan.holidays[0].untitled);
        let cut = &plan.holidays[3];
        assert!(cut.cut);
        assert_eq!(
            cut.text().unwrap().description.as_deref().map(str::len),
            Some(MAX_DESCRIPTION)
        );
        assert_eq!(
            plan.holidays_left,
            vec![
                (HolidayTable::Repeatable, 3, HolidayLeftover::BadWeekday),
                (HolidayTable::Repeatable, 4, HolidayLeftover::BadDate),
                (HolidayTable::Repeatable, 5, HolidayLeftover::SameDay),
                (HolidayTable::Repeatable, 6, HolidayLeftover::Incomplete),
                (HolidayTable::Repeatable, 8, HolidayLeftover::Incomplete),
                (HolidayTable::Special, 1, HolidayLeftover::SameDay),
                (HolidayTable::Special, 3, HolidayLeftover::SameDay),
                (HolidayTable::Special, 4, HolidayLeftover::BadDate),
                (HolidayTable::Special, 6, HolidayLeftover::Incomplete),
            ]
        );
        assert_eq!(plan.calendar_batches().len(), 1);
    }
}
