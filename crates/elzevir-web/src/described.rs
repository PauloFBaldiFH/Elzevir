// SPDX-License-Identifier: GPL-3.0-or-later
//! The form of a Hub or an Item: every property the ontology gives it, as rows of its
//! statements (docs/specs/catalogo/bibframe-completo.md, items 24 to 26, step 6c).

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use elzevir_app::staff::Permission;
use elzevir_domain::statement::{EntityKind, EntityRef};
use elzevir_web_templates::RecordForm;

use crate::form_sections::described_sections;
use crate::staff::current_session;
use crate::{AppState, render};

/// `GET /equipe/acervo/matriz/{id}/campos`.
pub(crate) async fn hub_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    let subject = EntityRef {
        entity: EntityKind::Hub,
        id,
    };
    form(
        &state,
        &headers,
        subject,
        format!("/equipe/acervo/matriz/{id}"),
    )
    .await
}

/// `GET /equipe/acervo/exemplar/{id}/campos`.
pub(crate) async fn item_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    let subject = EntityRef {
        entity: EntityKind::Item,
        id,
    };
    form(
        &state,
        &headers,
        subject,
        format!("/equipe/acervo/exemplar/{id}"),
    )
    .await
}

async fn form(
    state: &AppState,
    headers: &HeaderMap,
    subject: EntityRef,
    endpoint: String,
) -> Response {
    let Some(session) = current_session(state, headers) else {
        return Redirect::to("/equipe/login").into_response();
    };
    if !session.identity.can(Permission::Cataloguing) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let statements = Arc::clone(&state.inner.staff_ports.statements);
    let read = tokio::task::spawn_blocking(move || {
        let version = statements.version(subject)?;
        let statements = statements.statements(subject)?;
        Ok::<_, elzevir_app::catalog::CatalogReadError>(
            version.map(|version| (version, statements)),
        )
    })
    .await;
    let (version, statements) = match read {
        Ok(Ok(Some(read))) => read,
        Ok(Ok(None)) => return (StatusCode::NOT_FOUND, "no such entity").into_response(),
        Ok(Err(_)) | Err(_) => {
            return (StatusCode::SERVICE_UNAVAILABLE, "catalog unavailable").into_response();
        }
    };
    render(&RecordForm {
        t: &state.inner.i18n,
        endpoint,
        version,
        sections: described_sections(subject.entity.bibframe(), &statements),
        history: None,
        reach: None,
    })
}
