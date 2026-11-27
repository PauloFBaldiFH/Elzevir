// SPDX-License-Identifier: GPL-3.0-or-later
//! Enrolling and editing a patron (docs/specs/circulacao/leitores.md, items 15 to 18).

use std::sync::Arc;

use axum::Form;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use elzevir_app::patron_fields::{
    ExtraProblem, ExtraValue, FieldKind, PatronExtra, PatronFieldLine,
};
use elzevir_app::patrons::{
    Patron, PatronCommand, PatronDesk, PatronDeskError, PatronField, PatronFields, PatronOutput,
    PatronRefusal, PatronUpdate, ReadingHistory,
};
use elzevir_app::staff::Permission;
use elzevir_domain::catalog::BranchId;
use elzevir_domain::patron::{Cpf, PatronCategoryId, PatronFieldOptionId, PatronId};
use elzevir_web_templates::{
    AnonymizeSection, ExtraInput, PasswordSection, PatronFormValues, StaffPatronForm,
};
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
#[derive(Default)]
pub(crate) struct PatronForm {
    csrf: String,
    row_version: i64,
    cardnumber: String,
    surname: String,
    first_name: String,
    cpf: String,
    date_of_birth: String,
    email: String,
    phone: String,
    address: String,
    branch: i64,
    category: i64,
    expires: String,
    staff_note: String,
    history_kept: Option<String>,
    /// The additional fields, as typed: field id and value (an option id for a list), from
    /// the inputs named `extra.FIELD.N` (docs/specs/circulacao/patron-attributes.md, item 8).
    extra: Vec<(i64, String)>,
}

impl PatronForm {
    /// Reads the posted pairs; an additional field may come more than once.
    pub(crate) fn from_pairs(pairs: Vec<(String, String)>) -> Self {
        let mut form = Self::default();
        for (name, value) in pairs {
            let number = || value.trim().parse().unwrap_or_default();
            match name.as_str() {
                "csrf" => form.csrf = value,
                "row_version" => form.row_version = number(),
                "cardnumber" => form.cardnumber = value,
                "surname" => form.surname = value,
                "first_name" => form.first_name = value,
                "cpf" => form.cpf = value,
                "date_of_birth" => form.date_of_birth = value,
                "email" => form.email = value,
                "phone" => form.phone = value,
                "address" => form.address = value,
                "branch" => form.branch = number(),
                "category" => form.category = number(),
                "expires" => form.expires = value,
                "staff_note" => form.staff_note = value,
                "history_kept" => form.history_kept = Some(value),
                other => {
                    if let Some(field) = other
                        .strip_prefix("extra.")
                        .and_then(|rest| rest.split('.').next())
                        .and_then(|field| field.parse().ok())
                    {
                        form.extra.push((field, value));
                    }
                }
            }
        }
        form
    }
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
    /// The fields the form asks for, enrolled on `enrolled`; `kinds` reads the additional
    /// fields by the kind of each.
    fn fields(
        &self,
        enrolled: Date,
        kinds: &[PatronFieldLine],
    ) -> Result<PatronFields, PatronRefusal> {
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
            extra: extra_values(&self.extra, kinds)?,
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
            extra: self.extra.clone(),
            refused_field: 0,
        }
    }
}

/// The typed additional values as the command takes them; a field that no longer exists
/// is left out, and a date that is not one is refused (spec, item 10).
fn extra_values(
    typed: &[(i64, String)],
    kinds: &[PatronFieldLine],
) -> Result<Vec<PatronExtra>, PatronRefusal> {
    let mut extra = Vec::new();
    for (field, text) in typed {
        let Some(line) = kinds.iter().find(|line| line.id.0 == *field) else {
            continue;
        };
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        let invalid = || PatronRefusal::BadExtra(line.id, ExtraProblem::Invalid);
        let value = match line.spec.kind {
            FieldKind::Text => ExtraValue::Text(text.to_owned()),
            FieldKind::Date => ExtraValue::Date(text.parse().map_err(|_| invalid())?),
            FieldKind::List(_) => {
                ExtraValue::Option(PatronFieldOptionId(text.parse().map_err(|_| invalid())?))
            }
        };
        extra.push(PatronExtra {
            field: line.id,
            value,
        });
    }
    Ok(extra)
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
        extra: fields
            .extra
            .iter()
            .map(|extra| {
                let value = match &extra.value {
                    ExtraValue::Text(text) => text.clone(),
                    ExtraValue::Date(day) => day.to_string(),
                    ExtraValue::Option(option) => option.0.to_string(),
                };
                (extra.field.0, value)
            })
            .collect(),
        refused_field: 0,
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
    pub(crate) anonymize: Option<AnonymizeSection>,
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
        Ok::<_, PatronDeskError>((codes.branches, desk.categories()?, desk.fields()?))
    })
    .await;
    let (branches, categories, fields) = match lists {
        Ok(Ok(lists)) => lists,
        Ok(Err(error)) => return failed(&error),
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    let refused = match (page.refused, categories.is_empty()) {
        (None, true) => Some("patron-error-no-category"),
        (refused, _) => refused,
    };
    let extra_fields = extra_inputs(fields, &categories, &page.values);
    let mut response = render(&StaffPatronForm {
        extra_fields,
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
        anonymize: page.anonymize,
    });
    *response.status_mut() = page.status;
    response
}

/// The additional fields as the form shows them, filled with `values`.
fn extra_inputs(
    fields: Vec<PatronFieldLine>,
    categories: &[elzevir_app::patrons::PatronCategory],
    values: &PatronFormValues,
) -> Vec<ExtraInput> {
    fields
        .into_iter()
        .map(|line| {
            let id = line.id.0;
            let held: Vec<&str> = values
                .extra
                .iter()
                .filter(|(field, _)| *field == id)
                .map(|(_, value)| value.as_str())
                .collect();
            let (kind, options) = match &line.spec.kind {
                FieldKind::Text => ("text", Vec::new()),
                FieldKind::Date => ("date", Vec::new()),
                FieldKind::List(options) => (
                    "list",
                    options
                        .iter()
                        .filter_map(|option| {
                            let option_id = option.id?.0;
                            let chosen = held.contains(&option_id.to_string().as_str());
                            Some((option_id, option.label.clone(), chosen))
                        })
                        .collect(),
                ),
            };
            let mut boxes: Vec<String> = held
                .iter()
                .filter(|value| !value.trim().is_empty())
                .map(|value| (*value).to_owned())
                .collect();
            if kind != "list" && (boxes.is_empty() || line.spec.repeatable) {
                boxes.push(String::new());
            }
            ExtraInput {
                id,
                name: line.spec.name,
                kind,
                required: line.spec.required,
                only_for: line.spec.category.and_then(|category| {
                    categories
                        .iter()
                        .find(|known| known.id == category)
                        .map(|known| known.name.clone())
                }),
                options,
                several: line.spec.repeatable,
                boxes,
                invalid: values.refused_field == id,
            }
        })
        .collect()
}

/// The field a refusal of the additional values names, `0` for none.
const fn refused_field(refusal: PatronRefusal) -> i64 {
    match refusal {
        PatronRefusal::BadExtra(field, _) => field.0,
        _ => 0,
    }
}

/// Every additional field, to read the typed values by their kind.
async fn field_kinds(desk: &Arc<dyn PatronDesk>) -> Result<Vec<PatronFieldLine>, Response> {
    let desk = Arc::clone(desk);
    match tokio::task::spawn_blocking(move || desk.fields()).await {
        Ok(Ok(kinds)) => Ok(kinds),
        Ok(Err(error)) => Err(failed(&error)),
        Err(_) => Err(StatusCode::SERVICE_UNAVAILABLE.into_response()),
    }
}

const fn status_of(refusal: PatronRefusal) -> StatusCode {
    match refusal {
        PatronRefusal::CardnumberTaken
        | PatronRefusal::CpfTaken
        | PatronRefusal::Stale
        | PatronRefusal::BadExtra(_, ExtraProblem::Taken) => StatusCode::CONFLICT,
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
        anonymize: None,
    };
    form_page(&state, &session, desk, page).await
}

/// `POST /equipe/leitores/novo`: enrolled today in the library's time zone.
pub(crate) async fn add_patron(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(pairs): Form<Vec<(String, String)>>,
) -> Response {
    let (session, desk) = match open(&state, &headers, manages) {
        Ok(opened) => opened,
        Err(response) => return *response,
    };
    let form = PatronForm::from_pairs(pairs);
    if !tokens_match(&session.csrf, &form.csrf) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let refused = |refusal: PatronRefusal| FormPage {
        action: NEW.to_owned(),
        title: "patron-new",
        row_version: 0,
        values: PatronFormValues {
            refused_field: refused_field(refusal),
            ..form.values()
        },
        saved: false,
        refused: Some(refusal.message_key()),
        status: status_of(refusal),
        password: None,
        anonymize: None,
    };
    let kinds = match field_kinds(&desk).await {
        Ok(kinds) => kinds,
        Err(response) => return response,
    };
    let fields = match form.fields(today(&state), &kinds) {
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
        anonymize: Some(crate::patron_privacy::section(id, None)),
    };
    form_page(&state, &session, desk, page).await
}

/// `POST /equipe/leitores/{id}/editar`: the day of enrolment stays as stored.
pub(crate) async fn update_patron(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Form(pairs): Form<Vec<(String, String)>>,
) -> Response {
    let (session, desk) = match open(&state, &headers, manages) {
        Ok(opened) => opened,
        Err(response) => return *response,
    };
    let form = PatronForm::from_pairs(pairs);
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
        values: PatronFormValues {
            refused_field: refused_field(refusal),
            ..form.values()
        },
        saved: false,
        refused: Some(refusal.message_key()),
        status: status_of(refusal),
        password: Some(crate::patron_password::section(
            id,
            stored.has_password,
            None,
            None,
        )),
        anonymize: Some(crate::patron_privacy::section(id, None)),
    };
    let kinds = match field_kinds(&desk).await {
        Ok(kinds) => kinds,
        Err(response) => return response,
    };
    let fields = match form.fields(stored.fields.enrolled, &kinds) {
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
