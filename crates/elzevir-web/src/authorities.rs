// SPDX-License-Identifier: GPL-3.0-or-later
//! The focused block of an authority, an agent or a subject: its form, which says how
//! many records saving it changes, and its edit (docs/specs/catalogo/autoridades.md).

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use elzevir_app::authorities::{LinkRequest, link_request, validate_agent_changes};
use elzevir_app::catalog::{AgentChange, AgentUpdate, SubjectUpdate};
use elzevir_app::editing::{Edit, FieldError};
use elzevir_app::staff::Permission;
use elzevir_domain::catalog::{AgentId, SubjectId, WorkId};
use elzevir_web_templates::{
    FormField, FormInput, FormPart, FormSection, NewRecordForm, RecordForm,
};
use serde::Deserialize;

use crate::editing::{EditBody, apply, authorize, invalid};
use crate::record_form::link_field;
use crate::staff::current_session;
use crate::{AppState, render};

/// `GET /equipe/acervo/autoridade/{id}/campos`.
pub(crate) async fn agent_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    if let Some(refused) = refused(&state, &headers) {
        return refused;
    }
    let authorities = Arc::clone(&state.inner.staff_ports.authorities);
    match tokio::task::spawn_blocking(move || authorities.agent(AgentId(id))).await {
        Ok(Ok(Some(agent))) => heading_block(
            &state,
            format!("/equipe/acervo/autoridade/{id}"),
            agent.heading,
            agent.version,
            (agent.works, agent.instances),
        ),
        Ok(Ok(None)) => (StatusCode::NOT_FOUND, "no such agent").into_response(),
        Ok(Err(_)) | Err(_) => unavailable(),
    }
}

/// `GET /equipe/acervo/assunto/{id}/campos`.
pub(crate) async fn subject_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    if let Some(refused) = refused(&state, &headers) {
        return refused;
    }
    let authorities = Arc::clone(&state.inner.staff_ports.authorities);
    match tokio::task::spawn_blocking(move || authorities.subject(SubjectId(id))).await {
        Ok(Ok(Some(subject))) => heading_block(
            &state,
            format!("/equipe/acervo/assunto/{id}"),
            subject.heading,
            subject.version,
            (subject.works, subject.instances),
        ),
        Ok(Ok(None)) => (StatusCode::NOT_FOUND, "no such subject").into_response(),
        Ok(Err(_)) | Err(_) => unavailable(),
    }
}

/// No session: the login; no cataloguing permission: 403.
fn refused(state: &AppState, headers: &HeaderMap) -> Option<Response> {
    let Some(session) = current_session(state, headers) else {
        return Some(Redirect::to("/equipe/login").into_response());
    };
    (!session.identity.can(Permission::Cataloguing)).then(|| StatusCode::FORBIDDEN.into_response())
}

fn unavailable() -> Response {
    (StatusCode::SERVICE_UNAVAILABLE, "catalog unavailable").into_response()
}

/// The block of an authority: its heading, and how many Works and records saving it
/// changes.
fn heading_block(
    state: &AppState,
    endpoint: String,
    heading: String,
    version: i64,
    (works, instances): (usize, usize),
) -> Response {
    let t = &state.inner.i18n;
    let count = |n: usize| u64::try_from(n).unwrap_or(u64::MAX);
    render(&RecordForm {
        t,
        endpoint,
        version,
        sections: vec![FormSection {
            heading_key: None,
            fields: vec![FormField {
                change: "Heading",
                error: "heading",
                label_key: "staff-field-authorized-name",
                input: FormInput::Text {
                    value: heading,
                    required: true,
                },
                property: "",
                ontology_label: "",
                inherited: false,
            }],
        }],
        history: None,
        reach: Some(t.get_numbers(
            "authority-reach",
            &[("works", count(works)), ("instances", count(instances))],
        )),
    })
}

/// `POST /equipe/acervo/autoridade/{id}`.
pub(crate) async fn edit_agent(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<EditBody<AgentChange>>,
) -> Response {
    edit_heading(&state, &headers, body, |row_version, changes| {
        Edit::Agent(Box::new(AgentUpdate {
            agent: AgentId(id),
            row_version,
            changes,
        }))
    })
    .await
}

/// `POST /equipe/acervo/assunto/{id}`.
pub(crate) async fn edit_subject(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<EditBody<AgentChange>>,
) -> Response {
    edit_heading(&state, &headers, body, |row_version, changes| {
        Edit::Subject(Box::new(SubjectUpdate {
            subject: SubjectId(id),
            row_version,
            changes,
        }))
    })
    .await
}

async fn edit_heading(
    state: &AppState,
    headers: &HeaderMap,
    body: EditBody<AgentChange>,
    edit: impl FnOnce(i64, Vec<AgentChange>) -> Edit,
) -> Response {
    let session = match authorize(state, headers) {
        Ok(session) => session,
        Err(status) => return status.into_response(),
    };
    let mut changes = body.changes;
    if let Err(errors) = validate_agent_changes(&mut changes) {
        return invalid(state, &errors);
    }
    apply(state, &session, edit(body.version, changes)).await
}

/// `GET /equipe/acervo/obra/{id}/autoridade/nova`: the "+ Authority" form of a Work.
pub(crate) async fn link_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    if let Some(refused) = refused(&state, &headers) {
        return refused;
    }
    let t = &state.inner.i18n;
    render(&NewRecordForm {
        t,
        endpoint: format!("/equipe/acervo/obra/{id}/autoridades"),
        create: "authority",
        heading_key: "link-heading",
        submit_key: "link-submit",
        parts: vec![FormPart {
            name: "authority",
            heading_key: "link-heading",
            sections: vec![FormSection {
                heading_key: None,
                fields: vec![link_field(t)],
            }],
        }],
    })
}

/// What the "+ Authority" form sends: its one field, as every form sends its changes.
#[derive(Deserialize)]
pub(crate) struct LinkBody {
    changes: Vec<LinkChange>,
}

#[derive(Deserialize)]
enum LinkChange {
    Link(LinkRequest),
}

/// `POST /equipe/acervo/obra/{id}/autoridades`: links the authority by its role.
pub(crate) async fn link_authority(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<LinkBody>,
) -> Response {
    let session = match authorize(&state, &headers) {
        Ok(session) => session,
        Err(status) => return status.into_response(),
    };
    let Some(LinkChange::Link(request)) = body.changes.into_iter().next() else {
        return invalid(
            &state,
            &[FieldError {
                field: "link",
                message_key: "edit-error-heading-empty",
            }],
        );
    };
    match link_request(WorkId(id), request) {
        Ok(link) => apply(&state, &session, Edit::Link(Box::new(link))).await,
        Err(errors) => invalid(&state, &errors),
    }
}
