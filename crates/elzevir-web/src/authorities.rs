// SPDX-License-Identifier: GPL-3.0-or-later
//! The focused block of an authority: its form, which says how many records saving it
//! changes, and its edit (docs/specs/catalogo/autoridades.md).

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use elzevir_app::authorities::validate_agent_changes;
use elzevir_app::catalog::{AgentChange, AgentUpdate};
use elzevir_app::editing::Edit;
use elzevir_app::staff::Permission;
use elzevir_domain::catalog::AgentId;
use elzevir_web_templates::{FormField, FormInput, FormSection, RecordForm};

use crate::editing::{EditBody, apply, authorize, invalid};
use crate::staff::current_session;
use crate::{AppState, render};

/// `GET /equipe/acervo/autoridade/{id}/campos`.
pub(crate) async fn agent_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    let Some(session) = current_session(&state, &headers) else {
        return Redirect::to("/equipe/login").into_response();
    };
    if !session.identity.can(Permission::Cataloguing) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let agents = Arc::clone(&state.inner.staff_ports.agents);
    let read = tokio::task::spawn_blocking(move || agents.agent(AgentId(id))).await;
    let agent = match read {
        Ok(Ok(Some(agent))) => agent,
        Ok(Ok(None)) => return (StatusCode::NOT_FOUND, "no such agent").into_response(),
        Ok(Err(_)) | Err(_) => {
            return (StatusCode::SERVICE_UNAVAILABLE, "catalog unavailable").into_response();
        }
    };
    let t = &state.inner.i18n;
    let count = |n: usize| u64::try_from(n).unwrap_or(u64::MAX);
    render(&RecordForm {
        t,
        endpoint: format!("/equipe/acervo/autoridade/{id}"),
        version: agent.version,
        sections: vec![FormSection {
            heading_key: None,
            fields: vec![FormField {
                change: "Heading",
                error: "heading",
                label_key: "staff-field-authorized-name",
                input: FormInput::Text {
                    value: agent.heading,
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
            &[
                ("works", count(agent.works)),
                ("instances", count(agent.instances)),
            ],
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
    let session = match authorize(&state, &headers) {
        Ok(session) => session,
        Err(status) => return status.into_response(),
    };
    let mut changes = body.changes;
    if let Err(errors) = validate_agent_changes(&mut changes) {
        return invalid(&state, &errors);
    }
    let edit = Edit::Agent(Box::new(AgentUpdate {
        agent: AgentId(id),
        row_version: body.version,
        changes,
    }));
    apply(&state, &session, edit).await
}
