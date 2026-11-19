// SPDX-License-Identifier: GPL-3.0-or-later
//! The calendar screen: one month of a branch, its entries, and the forms that change them
//! (docs/specs/circulacao/calendario.md, items 9 to 14).

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::Form;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use elzevir_app::calendar::{
    CalendarAction, CalendarBook, CalendarChange, Holiday, HolidayText, HolidayWhen,
};
use elzevir_app::circulation_rules::CirculationDeskError;
use elzevir_app::staff::Permission;
use elzevir_domain::calendar::{AnnualHoliday, Calendar, DatedEntry};
use elzevir_domain::catalog::BranchId;
use elzevir_web_templates::{Choice, DayCell, EntryLine, I18n, StaffCalendar};
use jiff::ToSpan;
use jiff::civil::Date;
use serde::Deserialize;

use crate::patrons::{shown, today};
use crate::security::tokens_match;
use crate::session::ActiveSession;
use crate::staff::current_session;
use crate::{AppState, render};

const PATH: &str = "/equipe/admin/calendario";

/// The session and the calendar book, or the response that stops the request.
fn open(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(ActiveSession, Arc<dyn CalendarBook>), Box<Response>> {
    let Some(session) = current_session(state, headers) else {
        return Err(Box::new(Redirect::to("/equipe/login").into_response()));
    };
    if !session.identity.can(Permission::Administration) {
        return Err(Box::new(StatusCode::FORBIDDEN.into_response()));
    }
    let Some(book) = state.calendar.clone() else {
        return Err(Box::new(StatusCode::NOT_FOUND.into_response()));
    };
    Ok((session, book))
}

/// The first day of a `YYYY-MM` month.
fn month_start(text: &str) -> Option<Date> {
    let (year, month) = text.split_once('-')?;
    Date::new(year.parse().ok()?, month.parse().ok()?, 1).ok()
}

fn month_text(first: Date) -> String {
    first.strftime("%Y-%m").to_string()
}

fn url_of(branch: i64, first: Date) -> String {
    format!(
        "{PATH}?branch={branch}&month={}#calendar-month",
        month_text(first)
    )
}

/// Query string of the page.
#[derive(Deserialize)]
pub(crate) struct CalendarParams {
    branch: Option<i64>,
    month: Option<String>,
    saved: Option<String>,
}

/// `GET /equipe/admin/calendario`.
pub(crate) async fn calendar(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<CalendarParams>,
) -> Response {
    match open(&state, &headers) {
        Ok((session, book)) => {
            let first = params
                .month
                .as_deref()
                .and_then(month_start)
                .unwrap_or_else(|| today(&state).first_of_month());
            let shown = Shown {
                branch: params.branch,
                first,
                saved: params.saved.is_some(),
                refused: None,
            };
            page(&state, &session, book, shown, StatusCode::OK).await
        }
        Err(response) => *response,
    }
}

/// One posted form; each action reads the fields it needs.
#[derive(Deserialize)]
pub(crate) struct CalendarForm {
    csrf: String,
    action: String,
    branch: i64,
    #[serde(default)]
    month: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    weekday: Option<u8>,
    #[serde(default)]
    day: Option<i8>,
    #[serde(default)]
    annual_month: Option<i8>,
    #[serde(default)]
    first: String,
    #[serde(default)]
    last: String,
    #[serde(default)]
    open: Option<String>,
    #[serde(default)]
    to: Option<i64>,
}

/// The action a form asks for, or `None` when the form does not make sense.
fn action_of(form: &CalendarForm, today: Date) -> Option<CalendarAction> {
    let text = || HolidayText {
        title: form.title.clone(),
        description: Some(form.description.clone()),
    };
    let first = || form.first.parse::<Date>().ok();
    // An empty "to" is the one day of "from".
    let last = || {
        if form.last.trim().is_empty() {
            first()
        } else {
            form.last.parse::<Date>().ok()
        }
    };
    let annual = || {
        Some(AnnualHoliday {
            month: form.annual_month?,
            day: form.day?,
        })
    };
    Some(match form.action.as_str() {
        "weekly" => CalendarAction::CloseWeekday {
            weekday: form.weekday?,
            text: text(),
        },
        "annual" => CalendarAction::CloseAnnually {
            holiday: annual()?,
            text: text(),
        },
        "dates" => CalendarAction::SetDates {
            first: first()?,
            last: last()?,
            entry: if form.open.is_some() {
                DatedEntry::Open
            } else {
                DatedEntry::Closed
            },
            text: text(),
        },
        "reopen_weekday" => CalendarAction::ReopenWeekday(form.weekday?),
        "remove_annual" => CalendarAction::RemoveAnnual(annual()?),
        "remove_dates" => CalendarAction::RemoveDates {
            first: first()?,
            last: last()?,
        },
        "copy" => CalendarAction::CopyTo {
            to: BranchId(form.to?),
            after: today,
        },
        _ => return None,
    })
}

/// `POST /equipe/admin/calendario`.
pub(crate) async fn change_calendar(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<CalendarForm>,
) -> Response {
    let (session, book) = match open(&state, &headers) {
        Ok(opened) => opened,
        Err(response) => return *response,
    };
    if !tokens_match(&session.csrf, &form.csrf) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let today = today(&state);
    let Some(action) = action_of(&form, today) else {
        return (StatusCode::BAD_REQUEST, "unknown change").into_response();
    };
    let first = month_start(&form.month).unwrap_or_else(|| today.first_of_month());
    let change = CalendarChange {
        branch: BranchId(form.branch),
        action,
    };
    let actor = session.identity.id;
    let writer = Arc::clone(&book);
    let outcome = tokio::task::spawn_blocking(move || writer.change(actor, change)).await;
    match outcome {
        Ok(Ok(_)) => {
            Redirect::to(&url_of(form.branch, first).replace('#', "&saved=1#")).into_response()
        }
        Ok(Err(CirculationDeskError::Refused(refusal))) => {
            let shown = Shown {
                branch: Some(form.branch),
                first,
                saved: false,
                refused: Some(state.inner.i18n.get(refusal.message_key())),
            };
            page(
                &state,
                &session,
                book,
                shown,
                StatusCode::UNPROCESSABLE_ENTITY,
            )
            .await
        }
        Ok(Err(CirculationDeskError::Busy | CirculationDeskError::Unavailable(_))) | Err(_) => {
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

/// What the page shows besides the stored calendar.
struct Shown {
    branch: Option<i64>,
    first: Date,
    saved: bool,
    refused: Option<String>,
}

async fn page(
    state: &AppState,
    session: &ActiveSession,
    book: Arc<dyn CalendarBook>,
    shown: Shown,
    status: StatusCode,
) -> Response {
    let catalog = Arc::clone(&state.inner.opac.catalog);
    let Ok(Ok(codes)) = tokio::task::spawn_blocking(move || catalog.codes()).await else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let branch = shown
        .branch
        .filter(|id| codes.branches.iter().any(|row| row.id.0 == *id))
        .or_else(|| codes.branches.first().map(|row| row.id.0));
    let read = match branch {
        Some(id) => {
            let book = Arc::clone(&book);
            match tokio::task::spawn_blocking(move || {
                Ok::<_, CirculationDeskError>((
                    book.calendar(BranchId(id))?,
                    book.holidays(BranchId(id))?,
                ))
            })
            .await
            {
                Ok(Ok(read)) => read,
                _ => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
            }
        }
        None => (Calendar::always_open(), Vec::new()),
    };
    let t = &state.inner.i18n;
    let first = shown.first;
    let (calendar, holidays) = read;
    let mut response = render(&StaffCalendar {
        t,
        csrf: &session.csrf,
        branches: codes
            .branches
            .iter()
            .map(|row| Choice {
                id: row.id.0,
                name: row.name.clone(),
                chosen: Some(row.id.0) == branch,
            })
            .collect(),
        branch: branch.unwrap_or(0),
        month: month_text(first),
        month_title: t.get_texts(
            "calendar-month-title",
            &[
                ("month", &month_name(t, first.month())),
                ("year", &first.year().to_string()),
            ],
        ),
        previous_url: url_of(
            branch.unwrap_or(0),
            first.checked_sub(1.month()).unwrap_or(first),
        ),
        next_url: url_of(
            branch.unwrap_or(0),
            first.checked_add(1.month()).unwrap_or(first),
        ),
        weekdays: (0..7).map(|day| weekday_name(t, day)).collect(),
        weeks: weeks(&calendar, &holidays, first, today(state)),
        weekly: lines(t, &holidays, first, Kind::Weekly),
        annual: lines(t, &holidays, first, Kind::Annual),
        dated: lines(t, &holidays, first, Kind::Dated),
        months: (1..=12).map(|month| month_name(t, month)).collect(),
        saved: shown.saved,
        refused: shown.refused,
    });
    *response.status_mut() = status;
    response
}

fn weekday_name(t: &I18n, sunday_zero: u8) -> String {
    t.get(&format!("calendar-weekday-{sunday_zero}"))
}

fn month_name(t: &I18n, month: i8) -> String {
    t.get(&format!("calendar-month-{month}"))
}

/// The month's grid, Sunday first, with the entry that decides each day.
fn weeks(calendar: &Calendar, holidays: &[Holiday], first: Date, today: Date) -> Vec<Vec<DayCell>> {
    let mut weekly = BTreeMap::new();
    let mut annual = BTreeMap::new();
    let mut dated = BTreeMap::new();
    for holiday in holidays {
        match holiday.when {
            HolidayWhen::Weekly(day) => {
                weekly.insert(day, holiday.text.title.as_str());
            }
            HolidayWhen::Annual(day) => {
                annual.insert((day.month, day.day), holiday.text.title.as_str());
            }
            HolidayWhen::Dated(day, entry) => {
                dated.insert(day, (entry, holiday.text.title.as_str()));
            }
        }
    }
    let lead = usize::from(first.weekday().to_sunday_zero_offset().unsigned_abs());
    let mut cells = vec![DayCell::default(); lead];
    let mut day = first;
    while day.month() == first.month() {
        let weekday = day.weekday().to_sunday_zero_offset().unsigned_abs();
        let (exception, title) = match dated.get(&day) {
            Some((entry, title)) => (*entry == DatedEntry::Open, *title),
            None => (
                false,
                weekly
                    .get(&weekday)
                    .or_else(|| annual.get(&(day.month(), day.day())))
                    .copied()
                    .unwrap_or_default(),
            ),
        };
        cells.push(DayCell {
            day: day.day(),
            date: day.to_string(),
            closed: calendar.is_closed(day),
            exception,
            today: day == today,
            title: title.to_owned(),
        });
        let Ok(next) = day.checked_add(1.day()) else {
            break;
        };
        day = next;
    }
    while cells.len() % 7 != 0 {
        cells.push(DayCell::default());
    }
    cells.chunks(7).map(<[DayCell]>::to_vec).collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Weekly,
    Annual,
    Dated,
}

/// The entries of one kind; dated ones only from `first` on.
fn lines(t: &I18n, holidays: &[Holiday], first: Date, kind: Kind) -> Vec<EntryLine> {
    holidays
        .iter()
        .filter_map(|holiday| {
            let (when, key, exception) = match (kind, holiday.when) {
                (Kind::Weekly, HolidayWhen::Weekly(day)) => (
                    weekday_name(t, day),
                    vec![("weekday", day.to_string())],
                    false,
                ),
                (Kind::Annual, HolidayWhen::Annual(day)) => (
                    format!("{} {}", day.day, month_name(t, day.month)),
                    vec![
                        ("day", day.day.to_string()),
                        ("annual_month", day.month.to_string()),
                    ],
                    false,
                ),
                (Kind::Dated, HolidayWhen::Dated(day, entry)) if day >= first => (
                    shown(t, day),
                    vec![("first", day.to_string()), ("last", day.to_string())],
                    entry == DatedEntry::Open,
                ),
                _ => return None,
            };
            Some(EntryLine {
                when,
                key,
                title: holiday.text.title.clone(),
                description: holiday.text.description.clone().unwrap_or_default(),
                exception,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::{Weekday, date};

    #[test]
    fn the_grid_starts_on_sunday_and_names_what_closes_each_day() {
        let holidays = vec![
            Holiday {
                when: HolidayWhen::Weekly(0),
                text: HolidayText {
                    title: "Domingo".to_owned(),
                    description: None,
                },
            },
            Holiday {
                when: HolidayWhen::Dated(date(2026, 11, 20), DatedEntry::Closed),
                text: HolidayText {
                    title: "Consciência Negra".to_owned(),
                    description: None,
                },
            },
        ];
        let calendar = Calendar::always_open()
            .closed_every(Weekday::Sunday)
            .on(date(2026, 11, 20), DatedEntry::Closed);
        let grid = weeks(&calendar, &holidays, date(2026, 11, 1), date(2026, 11, 19));
        // 1 November 2026 is a Sunday; 30 days fill five weeks.
        assert_eq!(grid.len(), 5);
        let first = &grid[0][0];
        assert_eq!(
            (first.day, first.closed, first.title.as_str()),
            (1, true, "Domingo")
        );
        let friday = &grid[2][5];
        assert_eq!(
            (friday.day, friday.closed, friday.title.as_str()),
            (20, true, "Consciência Negra")
        );
        assert!(grid[2][4].today);
        assert_eq!(grid[4][1].day, 30);
        assert_eq!(grid[4][2].day, 0);
    }

    #[test]
    fn months_and_weekdays_have_names_in_both_languages() {
        for t in [I18n::english().unwrap(), I18n::portuguese().unwrap()] {
            for day in 0..7 {
                assert!(t.has(&format!("calendar-weekday-{day}")));
            }
            for month in 1..=12 {
                assert!(t.has(&format!("calendar-month-{month}")));
            }
        }
        assert_eq!(month_start("2026-02"), Some(date(2026, 2, 1)));
        assert_eq!(month_start("2026-13"), None);
    }
}
