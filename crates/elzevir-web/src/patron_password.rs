// SPDX-License-Identifier: GPL-3.0-or-later
//! The staff setting or taking away a patron's OPAC password
//! (docs/specs/opac/conta-do-leitor.md, items 1 to 4).

use std::sync::Arc;

use axum::Form;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use elzevir_app::patrons::{PasswordRefusal, PatronCommand, PatronDeskError, check_password};
use elzevir_app::staff::hash_password;
use elzevir_domain::patron::PatronId;
use elzevir_web_templates::PasswordSection;
use serde::Deserialize;

use crate::AppState;
use crate::patron_form::{FormPage, form_page, manages, stored_values};
use crate::patrons::{failed, open};
use crate::security::tokens_match;

/// The password part of the edit form of patron `id`.
pub(crate) fn section(
    id: i64,
    has_password: bool,
    notice: Option<&'static str>,
    error: Option<&'static str>,
) -> PasswordSection {
    PasswordSection {
        action: format!("/equipe/leitores/{id}/senha"),
        has_password,
        notice,
        error,
    }
}

/// The notice of `?senha=` after a change.
pub(crate) fn notice(senha: Option<&str>) -> Option<&'static str> {
    match senha {
        Some("1") => Some("patron-password-saved"),
        Some("0") => Some("patron-password-removed"),
        _ => None,
    }
}

const fn refusal_key(refusal: PasswordRefusal) -> &'static str {
    match refusal {
        PasswordRefusal::TooShort => "patron-password-short",
        PasswordRefusal::TooLong => "patron-password-long",
        PasswordRefusal::Guessable => "patron-password-guessable",
    }
}

/// Fields of the password forms.
#[derive(Deserialize)]
pub(crate) struct PasswordForm {
    csrf: String,
    #[serde(default)]
    password: String,
    /// Present when the access is taken away.
    remove: Option<String>,
}

/// `POST /equipe/leitores/{id}/senha`: sets the password, or with `remove` takes it away.
pub(crate) async fn set_password(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Form(form): Form<PasswordForm>,
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
    let removing = form.remove.is_some();
    if !removing && let Err(refusal) = check_password(&form.password, &stored.fields) {
        let page = FormPage {
            action: format!("/equipe/leitores/{id}/editar"),
            title: "patron-edit",
            row_version: stored.row_version,
            values: stored_values(&stored),
            saved: false,
            refused: None,
            status: StatusCode::UNPROCESSABLE_ENTITY,
            password: Some(section(
                id,
                stored.has_password,
                None,
                Some(refusal_key(refusal)),
            )),
            anonymize: Some(crate::patron_privacy::section(id, None)),
        };
        return form_page(&state, &session, desk, page).await;
    }
    let actor = session.identity.id;
    let password = form.password;
    // Argon2 is blocking work, like the write.
    let outcome = tokio::task::spawn_blocking(move || {
        let password_hash = if removing {
            None
        } else {
            Some(
                hash_password(&password)
                    .map_err(|error| PatronDeskError::Unavailable(error.to_string()))?,
            )
        };
        desk.apply(
            actor,
            PatronCommand::SetPassword {
                patron: PatronId(id),
                password_hash,
            },
        )
    })
    .await;
    match outcome {
        Ok(Ok(_)) => {
            let done = if removing { "0" } else { "1" };
            Redirect::to(&format!(
                "/equipe/leitores/{id}/editar?senha={done}#password-title"
            ))
            .into_response()
        }
        Ok(Err(error)) => failed(&error),
        Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}
