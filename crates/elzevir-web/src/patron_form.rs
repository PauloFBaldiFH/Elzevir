// SPDX-License-Identifier: GPL-3.0-or-later
//! Enrolling and editing a patron (docs/specs/circulacao/leitores.md, items 15 to 18).

use std::sync::Arc;

use axum::Form;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use elzevir_app::patrons::{
    Patron, PatronCommand, PatronDesk, PatronDeskError, PatronField, PatronFields, PatronOutput,
    PatronRefusal, PatronUpdate, ReadingHistory,
};
use elzevir_app::staff::Permission;
use elzevir_domain::catalog::BranchId;
use elzevir_domain::patron::{Cpf, PatronCategoryId, PatronId};
use elzevir_web_templates::{PasswordSection, PatronFormValues, StaffPatronForm};
use jiff::civil::Date;
use serde::Deserialize;

use crate::patrons::{failed, open, today};
use crate::security::tokens_match;
use crate::session::ActiveSession;
use crate::{AppState, render};

pub(crate) fn manages(session: &ActiveSession) -> bool {
    session.identity.can(Permission::Patrons)
}

/// The posted form, every field as text.
#[derive(Deserialize, Default)]
pub(crate) struct PatronForm {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    row_version: i64,
    #[serde(default)]
    cardnumber: String,
    #[serde(default)]
    surname: String,
    #[serde(default)]
    first_name: String,
    #[serde(default)]
    cpf: String,
    #[serde(default)]
    date_of_birth: String,
    #[serde(default)]
    email: String,
    #[serde(default)]
    phone: String,
    #[serde(default)]
    address: String,
    #[serde(default)]
    branch: i64,
    #[serde(default)]
    category: i64,
    #[serde(default)]
    expires: String,
    #[serde(default)]
    staff_note: String,
    #[serde(default)]
    history_kept: Option<String>,
}

fn text(value: &str) -> Option<String> {
    (!value.trim().is_empty()).then(|| value.to_owned())
}

fn date(value: &str, field: PatronField) -> Result<Option<Date>, PatronRefusal> {
    match value.trim() {
        "" => Ok(None),
        typed => typed
            .parse()
            .map(Some)
            .map_err(|_| PatronRefusal::BadField(field)),
    }
}

impl PatronForm {
    /// The fields the form asks for, enrolled on `enrolled`.
    fn fields(&self, enrolled: Date) -> Result<PatronFields, PatronRefusal> {
        let cpf = match self.cpf.trim() {
            "" => None,
            typed => {
                Some(Cpf::parse(typed).map_err(|_| PatronRefusal::BadField(PatronField::Cpf))?)
            }
        };
        Ok(PatronFields {
            cardnumber: self.cardnumber.clone(),
            surname: self.surname.clone(),
            first_name: text(&self.first_name),
            cpf,
            date_of_birth: date(&self.date_of_birth, PatronField::DateOfBirth)?,
            email: text(&self.email),
            phone: text(&self.phone),
            address: text(&self.address),
            branch: BranchId(self.branch),
            category: PatronCategoryId(self.category),
            enrolled,
            expires: date(&self.expires, PatronField::Expires)?,
            staff_note: text(&self.staff_note),
            reading_history: if self.history_kept.is_some() {
                ReadingHistory::Kept
            } else {
                ReadingHistory::NotKept
            },
        })
    }

    /// The form shown again as typed.
    fn values(&self) -> PatronFormValues {
        PatronFormValues {
            cardnumber: self.cardnumber.clone(),
            surname: self.surname.clone(),
            first_name: self.first_name.clone(),
            cpf: self.cpf.clone(),
            date_of_birth: self.date_of_birth.clone(),
            email: self.email.clone(),
            phone: self.phone.clone(),
            address: self.address.clone(),
            branch: self.branch,
            category: self.category,
            expires: self.expires.clone(),
            staff_note: self.staff_note.clone(),
            history_kept: self.history_kept.is_some(),
        }
    }
}

/// The stored patron in the form; the CPF whole, only here (spec, item 8).
pub(crate) fn stored_values(patron: &Patron) -> PatronFormValues {
    let fields = &patron.fields;
    let day = |day: Option<Date>| day.map(|day| day.to_string()).unwrap_or_default();
    PatronFormValues {
        cardnumber: fields.cardnumber.clone(),
        surname: fields.surname.clone(),
        first_name: fields.first_name.clone().unwrap_or_default(),
        cpf: fields
            .cpf
            .as_ref()
            .map(|cpf| cpf.digits().to_owned())
            .unwrap_or_default(),
        date_of_birth: day(fields.date_of_birth),
        email: fields.email.clone().unwrap_or_default(),
        phone: fields.phone.clone().unwrap_or_default(),
        address: fields.address.clone().unwrap_or_default(),
        branch: fields.branch.0,
        category: fields.category.0,
        expires: day(fields.expires),
        staff_note: fields.staff_note.clone().unwrap_or_default(),
        history_kept: fields.reading_history == ReadingHistory::Kept,
    }
}

/// What a form page shows besides the values.
pub(crate) struct FormPage {
    pub(crate) action: String,
    pub(crate) title: &'static str,
    pub(crate) row_version: i64,
    pub(crate) values: PatronFormValues,
    pub(crate) saved: bool,
    pub(crate) refused: Option<&'static str>,
    pub(crate) status: StatusCode,
    pub(crate) password: Option<PasswordSection>,
}

pub(crate) async fn form_page(
    state: &AppState,
    session: &ActiveSession,
    desk: Arc<dyn PatronDesk>,
    page: FormPage,
) -> Response {
    let catalog = Arc::clone(&state.inner.opac.catalog);
    let lists = tokio::task::spawn_blocking(move || {
        let codes = catalog
            .codes()
            .map_err(|error| PatronDeskError::Unavailable(error.to_string()))?;
        Ok::<_, PatronDeskError>((codes.branches, desk.categories()?))
    })
    .await;
    let (branches, categories) = match lists {
        Ok(Ok(lists)) => lists,
        Ok(Err(error)) => return failed(&error),
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    let refused = match (page.refused, categories.is_empty()) {
        (None, true) => Some("patron-error-no-category"),
        (refused, _) => refused,
    };
    let mut response = render(&StaffPatronForm {
        t: &state.inner.i18n,
        csrf: &session.csrf,
        action: page.action,
        title: page.title,
        row_version: page.row_version,
        values: page.values,
        branches: branches
            .into_iter()
            .map(|branch| (branch.id.0, branch.name))
            .collect(),
        categories: categories
            .into_iter()
            .map(|category| (category.id.0, category.name))
            .collect(),
        saved: page.saved,
        refused,
        password: page.password,
    });
    *response.status_mut() = page.status;
    response
}

const fn status_of(refusal: PatronRefusal) -> StatusCode {
    match refusal {
        PatronRefusal::CardnumberTaken | PatronRefusal::CpfTaken | PatronRefusal::Stale => {
            StatusCode::CONFLICT
        }
        _ => StatusCode::UNPROCESSABLE_ENTITY,
    }
}

const NEW: &str = "/equipe/leitores/novo";

/// `GET /equipe/leitores/novo`.
pub(crate) async fn new_patron(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let (session, desk) = match open(&state, &headers, manages) {
        Ok(opened) => opened,
        Err(response) => return *response,
    };
    let page = FormPage {
        action: NEW.to_owned(),
        title: "patron-new",
        row_version: 0,
        values: PatronFormValues::default(),
        saved: false,
        refused: None,
        status: StatusCode::OK,
        password: None,
    };
    form_page(&state, &session, desk, page).await
}

/// `POST /equipe/leitores/novo`: enrolled today in the library's time zone.
pub(crate) async fn add_patron(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<PatronForm>,
) -> Response {
    let (session, desk) = match open(&state, &headers, manages) {
        Ok(opened) => opened,
        Err(response) => return *response,
    };
    if !tokens_match(&session.csrf, &form.csrf) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let refused = |refusal: PatronRefusal| FormPage {
        action: NEW.to_owned(),
        title: "patron-new",
        row_version: 0,
        values: form.values(),
        saved: false,
        refused: Some(refusal.message_key()),
        status: status_of(refusal),
        password: None,
    };
    let fields = match form.fields(today(&state)) {
        Ok(fields) => fields,
        Err(refusal) => return form_page(&state, &session, desk, refused(refusal)).await,
    };
    let actor = session.identity.id;
    let writer = Arc::clone(&desk);
    let outcome = tokio::task::spawn_blocking(move || {
        writer.apply(actor, PatronCommand::Add(Box::new(fields)))
    })
    .await;
    match outcome {
        Ok(Ok(PatronOutput::Added(id))) => {
            Redirect::to(&format!("/equipe/leitores/{}/editar?saved=1", id.0)).into_response()
        }
        Ok(Ok(_)) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        Ok(Err(PatronDeskError::Refused(refusal))) => {
            form_page(&state, &session, desk, refused(refusal)).await
        }
        Ok(Err(error)) => failed(&error),
        Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

/// Query string of the edit form.
#[derive(Deserialize)]
pub(crate) struct EditParams {
    saved: Option<String>,
    /// `1` after a password was set, `0` after it was taken away.
    senha: Option<String>,
}

/// Logs the opening, then reads the patron (the form shows the whole CPF).
pub(crate) async fn viewed(
    desk: &Arc<dyn PatronDesk>,
    session: &ActiveSession,
    id: PatronId,
) -> Result<Patron, Response> {
    let desk = Arc::clone(desk);
    let actor = session.identity.id;
    let read = tokio::task::spawn_blocking(move || {
        desk.apply(actor, PatronCommand::View(id))?;
        desk.patron(id)
    })
    .await;
    match read {
        Ok(Ok(Some(patron))) => Ok(patron),
        Ok(Ok(None)) => Err(StatusCode::NOT_FOUND.into_response()),
        Ok(Err(error)) => Err(failed(&error)),
        Err(_) => Err(StatusCode::SERVICE_UNAVAILABLE.into_response()),
    }
}

/// `GET /equipe/leitores/{id}/editar`.
pub(crate) async fn edit_patron(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Query(params): Query<EditParams>,
) -> Response {
    let (session, desk) = match open(&state, &headers, manages) {
        Ok(opened) => opened,
        Err(response) => return *response,
    };
    let patron = match viewed(&desk, &session, PatronId(id)).await {
        Ok(patron) => patron,
        Err(response) => return response,
    };
    let page = FormPage {
        action: format!("/equipe/leitores/{id}/editar"),
        title: "patron-edit",
        row_version: patron.row_version,
        values: stored_values(&patron),
        saved: params.saved.is_some(),
        refused: None,
        status: StatusCode::OK,
        password: Some(crate::patron_password::section(
            id,
            patron.has_password,
            crate::patron_password::notice(params.senha.as_deref()),
            None,
        )),
    };
    form_page(&state, &session, desk, page).await
}

/// `POST /equipe/leitores/{id}/editar`: the day of enrolment stays as stored.
pub(crate) async fn update_patron(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Form(form): Form<PatronForm>,
) -> Response {
    let (session, desk) = match open(&state, &headers, manages) {
        Ok(opened) => opened,
        Err(response) => return *response,
    };
    if !tokens_match(&session.csrf, &form.csrf) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let reader = Arc::clone(&desk);
    let stored = match tokio::task::spawn_blocking(move || reader.patron(PatronId(id))).await {
        Ok(Ok(Some(patron))) => patron,
        Ok(Ok(None)) => return StatusCode::NOT_FOUND.into_response(),
        Ok(Err(error)) => return failed(&error),
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    let action = format!("/equipe/leitores/{id}/editar");
    let refused = |refusal: PatronRefusal| FormPage {
        action: action.clone(),
        title: "patron-edit",
        row_version: form.row_version,
        values: form.values(),
        saved: false,
        refused: Some(refusal.message_key()),
        status: status_of(refusal),
        password: Some(crate::patron_password::section(
            id,
            stored.has_password,
            None,
            None,
        )),
    };
    let fields = match form.fields(stored.fields.enrolled) {
        Ok(fields) => fields,
        Err(refusal) => return form_page(&state, &session, desk, refused(refusal)).await,
    };
    let update = PatronUpdate {
        patron: PatronId(id),
        row_version: form.row_version,
        fields,
    };
    let actor = session.identity.id;
    let writer = Arc::clone(&desk);
    let outcome = tokio::task::spawn_blocking(move || {
        writer.apply(actor, PatronCommand::Update(Box::new(update)))
    })
    .await;
    match outcome {
        Ok(Ok(_)) => Redirect::to(&format!("{action}?saved=1")).into_response(),
        Ok(Err(PatronDeskError::Refused(refusal))) => {
            form_page(&state, &session, desk, refused(refusal)).await
        }
        Ok(Err(error)) => failed(&error),
        Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}
