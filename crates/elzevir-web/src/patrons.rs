// SPDX-License-Identifier: GPL-3.0-or-later
//! Patrons behind the wall: search, record and categories
//! (docs/specs/circulacao/leitores.md, items 10 to 14; interface §18).

use std::sync::Arc;

use axum::Form;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use elzevir_app::patrons::{
    CategoryChange, Patron, PatronCommand, PatronDesk, PatronDeskError, PatronQuery, PatronRefusal,
    ReadingHistory,
};
use elzevir_app::staff::Permission;
use elzevir_domain::patron::{Cpf, Enrolment, PatronCategoryId, PatronId, is_expired};
use elzevir_web_templates::{
    CategoryLine, I18n, PatronLine, StaffPatron, StaffPatronCategories, StaffPatrons,
};
use jiff::civil::Date;
use serde::Deserialize;

use crate::security::tokens_match;
use crate::session::ActiveSession;
use crate::staff::current_session;
use crate::{AppState, render};

const CATEGORIES: &str = "/equipe/leitores/categorias";

/// The session and the desk, or the response that stops the request.
pub(crate) fn open(
    state: &AppState,
    headers: &HeaderMap,
    allowed: impl Fn(&ActiveSession) -> bool,
) -> Result<(ActiveSession, Arc<dyn PatronDesk>), Box<Response>> {
    let Some(session) = current_session(state, headers) else {
        return Err(Box::new(Redirect::to("/equipe/login").into_response()));
    };
    if !allowed(&session) {
        return Err(Box::new(StatusCode::FORBIDDEN.into_response()));
    }
    let Some(desk) = state.patrons.clone() else {
        return Err(Box::new(StatusCode::NOT_FOUND.into_response()));
    };
    Ok((session, desk))
}

fn sees_patrons(session: &ActiveSession) -> bool {
    session.identity.can(Permission::Circulation) || session.identity.can(Permission::Patrons)
}

pub(crate) fn failed(error: &PatronDeskError) -> Response {
    match error {
        PatronDeskError::Refused(PatronRefusal::UnknownPatron) => {
            StatusCode::NOT_FOUND.into_response()
        }
        PatronDeskError::Refused(refusal) => {
            (StatusCode::UNPROCESSABLE_ENTITY, refusal.to_string()).into_response()
        }
        PatronDeskError::Busy | PatronDeskError::Unavailable(_) => {
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

/// A date as the language writes it.
pub(crate) fn shown(t: &I18n, day: Date) -> String {
    if t.lang().starts_with("pt") {
        day.strftime("%d/%m/%Y").to_string()
    } else {
        day.to_string()
    }
}

/// Today in the library's time zone.
pub(crate) fn today(state: &AppState) -> Date {
    jiff::Timestamp::now()
        .to_zoned(state.inner.config.time_zone.clone())
        .date()
}

pub(crate) fn full_name(surname: &str, first_name: Option<&str>) -> String {
    match first_name {
        Some(first) => format!("{surname}, {first}"),
        None => surname.to_owned(),
    }
}

/// Query string of the search.
#[derive(Deserialize)]
pub(crate) struct SearchParams {
    #[serde(default)]
    q: String,
}

/// `GET /equipe/leitores`: the search. A card number found alone opens its record.
pub(crate) async fn search(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<SearchParams>,
) -> Response {
    let (session, desk) = match open(&state, &headers, sees_patrons) {
        Ok(opened) => opened,
        Err(response) => return *response,
    };
    let query = PatronQuery::parse(&params.q);
    let rows = match query.clone() {
        None => Vec::new(),
        Some(query) => match tokio::task::spawn_blocking(move || desk.search(&query)).await {
            Ok(Ok(rows)) => rows,
            Ok(Err(error)) => return failed(&error),
            Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
        },
    };
    if let (Some(query), [only]) = (&query, rows.as_slice())
        && only.cardnumber == query.cardnumber
    {
        return Redirect::to(&format!("/equipe/leitores/{}", only.id.0)).into_response();
    }
    let t = &state.inner.i18n;
    let today = today(&state);
    render(&StaffPatrons {
        t,
        query: &params.q,
        searched: query.is_some(),
        rows: rows
            .into_iter()
            .map(|row| PatronLine {
                id: row.id.0,
                name: full_name(&row.surname, row.first_name.as_deref()),
                cardnumber: row.cardnumber,
                category: row.category_name,
                expires: shown(t, row.expires),
                expired: is_expired(row.expires, today),
            })
            .collect(),
        can_manage: session.identity.can(Permission::Patrons),
    })
}

/// `GET /equipe/leitores/{id}`: the record, only with the circulation permission, and every
/// opening in the audit log (interface §18.1).
pub(crate) async fn record(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    let (session, desk) = match open(&state, &headers, |session| {
        session.identity.can(Permission::Circulation)
    }) {
        Ok(opened) => opened,
        Err(response) => return *response,
    };
    let actor = session.identity.id;
    let read = tokio::task::spawn_blocking(move || {
        // The opening is logged before the data leaves the server.
        desk.apply(actor, PatronCommand::View(PatronId(id)))?;
        desk.patron(PatronId(id))
    })
    .await;
    let patron = match read {
        Ok(Ok(Some(patron))) => patron,
        Ok(Ok(None)) => return StatusCode::NOT_FOUND.into_response(),
        Ok(Err(error)) => return failed(&error),
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    let t = &state.inner.i18n;
    let expired = patron
        .fields
        .expires
        .is_some_and(|day| is_expired(day, today(&state)));
    render(&StaffPatron {
        t,
        name: full_name(&patron.fields.surname, patron.fields.first_name.as_deref()),
        rows: record_rows(t, &patron),
        expired,
        edit_url: session
            .identity
            .can(Permission::Patrons)
            .then(|| format!("/equipe/leitores/{id}/editar")),
    })
}

/// The rows of the record; the CPF only masked (spec, item 8).
fn record_rows(t: &I18n, patron: &Patron) -> Vec<(&'static str, String)> {
    let fields = &patron.fields;
    let mut rows = vec![
        ("patron-cardnumber", fields.cardnumber.clone()),
        ("patron-category", patron.category_name.clone()),
        ("patron-branch", patron.branch_name.clone()),
        ("patron-enrolled", shown(t, fields.enrolled)),
    ];
    if let Some(expires) = fields.expires {
        rows.push(("patron-expires", shown(t, expires)));
    }
    let optional = [
        ("patron-cpf", fields.cpf.as_ref().map(Cpf::masked)),
        (
            "patron-date-of-birth",
            fields.date_of_birth.map(|day| shown(t, day)),
        ),
        ("patron-email", fields.email.clone()),
        ("patron-phone", fields.phone.clone()),
        ("patron-address", fields.address.clone()),
        ("patron-staff-note", fields.staff_note.clone()),
    ];
    rows.extend(
        optional
            .into_iter()
            .filter_map(|(label, value)| value.map(|value| (label, value))),
    );
    rows.push((
        "patron-reading-history",
        t.get(match fields.reading_history {
            ReadingHistory::Kept => "patron-history-kept",
            ReadingHistory::NotKept => "patron-history-not-kept",
        }),
    ));
    rows
}

/// Query string of the categories page.
#[derive(Deserialize)]
pub(crate) struct CategoriesParams {
    saved: Option<String>,
}

/// `GET /equipe/leitores/categorias`.
pub(crate) async fn categories(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<CategoriesParams>,
) -> Response {
    match open(&state, &headers, |session| {
        session.identity.can(Permission::Patrons)
    }) {
        Ok((session, desk)) => {
            categories_page(&state, &session, desk, params.saved.is_some(), None).await
        }
        Err(response) => *response,
    }
}

/// One posted category form.
#[derive(Deserialize)]
pub(crate) struct CategoryForm {
    csrf: String,
    action: String,
    #[serde(default)]
    id: Option<i64>,
    #[serde(default)]
    name: String,
    #[serde(default)]
    months: String,
    #[serde(default)]
    until: String,
}

/// The change a form asks for; `Err` with a refusal for a period that is neither months
/// nor a date, `Ok(None)` for a form that makes no sense.
fn category_change(form: &CategoryForm) -> Result<Option<CategoryChange>, PatronRefusal> {
    let enrolment = || match (form.months.trim(), form.until.trim()) {
        (months, "") => months
            .parse()
            .map(Enrolment::Months)
            .map_err(|_| PatronRefusal::BadCategory),
        ("", until) => until
            .parse()
            .map(Enrolment::Until)
            .map_err(|_| PatronRefusal::BadCategory),
        _ => Err(PatronRefusal::BadCategory),
    };
    Ok(match (form.action.as_str(), form.id) {
        ("create", _) => Some(CategoryChange::Create {
            name: form.name.clone(),
            enrolment: enrolment()?,
        }),
        ("update", Some(id)) => Some(CategoryChange::Update {
            category: PatronCategoryId(id),
            name: form.name.clone(),
            enrolment: enrolment()?,
        }),
        ("delete", Some(id)) => Some(CategoryChange::Delete(PatronCategoryId(id))),
        _ => None,
    })
}

/// `POST /equipe/leitores/categorias`.
pub(crate) async fn change_category(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<CategoryForm>,
) -> Response {
    let (session, desk) = match open(&state, &headers, |session| {
        session.identity.can(Permission::Patrons)
    }) {
        Ok(opened) => opened,
        Err(response) => return *response,
    };
    if !tokens_match(&session.csrf, &form.csrf) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let change = match category_change(&form) {
        Ok(Some(change)) => change,
        Ok(None) => return (StatusCode::BAD_REQUEST, "unknown change").into_response(),
        Err(refusal) => {
            return refused_page(&state, &session, desk, refusal).await;
        }
    };
    let actor = session.identity.id;
    let writer = Arc::clone(&desk);
    let outcome = tokio::task::spawn_blocking(move || {
        writer.apply(actor, PatronCommand::ChangeCategory(change))
    })
    .await;
    match outcome {
        Ok(Ok(_)) => Redirect::to(&format!("{CATEGORIES}?saved=1")).into_response(),
        Ok(Err(PatronDeskError::Refused(refusal))) => {
            refused_page(&state, &session, desk, refusal).await
        }
        Ok(Err(error)) => failed(&error),
        Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

async fn refused_page(
    state: &AppState,
    session: &ActiveSession,
    desk: Arc<dyn PatronDesk>,
    refusal: PatronRefusal,
) -> Response {
    let mut response =
        categories_page(state, session, desk, false, Some(refusal.message_key())).await;
    if response.status() == StatusCode::OK {
        *response.status_mut() = match refusal {
            PatronRefusal::CategoryNameTaken | PatronRefusal::CategoryInUse => StatusCode::CONFLICT,
            PatronRefusal::UnknownCategory => StatusCode::NOT_FOUND,
            _ => StatusCode::UNPROCESSABLE_ENTITY,
        };
    }
    response
}

async fn categories_page(
    state: &AppState,
    session: &ActiveSession,
    desk: Arc<dyn PatronDesk>,
    saved: bool,
    refused: Option<&'static str>,
) -> Response {
    let categories = match tokio::task::spawn_blocking(move || desk.categories()).await {
        Ok(Ok(categories)) => categories,
        Ok(Err(error)) => return failed(&error),
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    let t = &state.inner.i18n;
    render(&StaffPatronCategories {
        t,
        csrf: &session.csrf,
        categories: categories
            .into_iter()
            .map(|category| {
                let (months, until) = match category.enrolment {
                    Enrolment::Months(months) => (months.to_string(), String::new()),
                    Enrolment::Until(day) => (String::new(), day.to_string()),
                };
                CategoryLine {
                    id: category.id.0,
                    name: category.name,
                    months,
                    until,
                    patrons: t
                        .get_numbers("patron-category-patrons", &[("count", category.patrons)]),
                }
            })
            .collect(),
        saved,
        refused,
    })
}
