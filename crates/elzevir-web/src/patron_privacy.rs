// SPDX-License-Identifier: GPL-3.0-or-later
//! The staff anonymizing a patron, or taking a copy of their data
//! (docs/specs/circulacao/lgpd.md, items 1 to 11).

use std::sync::Arc;

use axum::Form;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use elzevir_app::patrons::{PatronCommand, PatronDeskError};
use elzevir_domain::patron::PatronId;
use elzevir_web_templates::AnonymizeSection;
use serde::Deserialize;

use crate::AppState;
use crate::patron_form::{FormPage, form_page, manages, stored_values};
use crate::patrons::{failed, open};
use crate::security::tokens_match;

/// The anonymization part of the edit form of patron `id`.
pub(crate) fn section(id: i64, refused: Option<&'static str>) -> AnonymizeSection {
    AnonymizeSection {
        export: format!("/equipe/leitores/{id}/exportar"),
        action: format!("/equipe/leitores/{id}/anonimizar"),
        refused,
    }
}

/// Fields of the export form.
#[derive(Deserialize)]
pub(crate) struct ExportForm {
    csrf: String,
}

/// `POST /equipe/leitores/{id}/exportar`: the logged copy of everything kept about the
/// patron, as a JSON download (item 11).
pub(crate) async fn export(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Form(form): Form<ExportForm>,
) -> Response {
    let (session, desk) = match open(&state, &headers, manages) {
        Ok(opened) => opened,
        Err(response) => return *response,
    };
    if !tokens_match(&session.csrf, &form.csrf) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let actor = session.identity.id;
    let patron = PatronId(id);
    // The export is logged before the data leaves the server, as the opening of the record.
    let outcome = tokio::task::spawn_blocking(move || {
        desk.apply(actor, PatronCommand::Export(patron))?;
        desk.export(patron)
    })
    .await;
    let document = match outcome {
        Ok(Ok(Some(document))) => document,
        Ok(Ok(None) | Err(PatronDeskError::Refused(_))) => {
            return StatusCode::NOT_FOUND.into_response();
        }
        Ok(Err(error)) => return failed(&error),
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    let Ok(body) = serde_json::to_string_pretty(&document) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    (
        [
            (
                header::CONTENT_TYPE,
                "application/json; charset=utf-8".to_owned(),
            ),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"leitor-{id}.json\""),
            ),
            (header::CACHE_CONTROL, "no-store".to_owned()),
        ],
        body,
    )
        .into_response()
}

/// Fields of the anonymization form.
#[derive(Deserialize)]
pub(crate) struct AnonymizeForm {
    csrf: String,
    /// The box that says it cannot be undone, ticked.
    confirm: Option<String>,
}

/// `POST /equipe/leitores/{id}/anonimizar`: only with the box ticked.
pub(crate) async fn anonymize(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Form(form): Form<AnonymizeForm>,
) -> Response {
    let (session, desk) = match open(&state, &headers, manages) {
        Ok(opened) => opened,
        Err(response) => return *response,
    };
    if !tokens_match(&session.csrf, &form.csrf) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let refusal = if form.confirm.as_deref() == Some("1") {
        let actor = session.identity.id;
        let writer = Arc::clone(&desk);
        let outcome = tokio::task::spawn_blocking(move || {
            writer.apply(actor, PatronCommand::Anonymize(PatronId(id)))
        })
        .await;
        match outcome {
            Ok(Ok(_)) => return Redirect::to("/equipe/leitores?anonimizado=1").into_response(),
            Ok(Err(PatronDeskError::Refused(refusal))) => refusal.message_key(),
            Ok(Err(error)) => return failed(&error),
            Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
        }
    } else {
        "patron-anonymize-unconfirmed"
    };
    let reader = Arc::clone(&desk);
    let stored = match tokio::task::spawn_blocking(move || reader.patron(PatronId(id))).await {
        Ok(Ok(Some(patron))) => patron,
        Ok(Ok(None)) => return StatusCode::NOT_FOUND.into_response(),
        Ok(Err(error)) => return failed(&error),
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    let page = FormPage {
        action: format!("/equipe/leitores/{id}/editar"),
        title: "patron-edit",
        row_version: stored.row_version,
        values: stored_values(&stored),
        saved: false,
        refused: None,
        status: StatusCode::UNPROCESSABLE_ENTITY,
        password: Some(crate::patron_password::section(
            id,
            stored.has_password,
            None,
            None,
        )),
        anonymize: Some(section(id, Some(refusal))),
    };
    form_page(&state, &session, desk, page).await
}
