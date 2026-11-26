// SPDX-License-Identifier: GPL-3.0-or-later
//! `/oai`: the OAI-PMH repository of the public catalog (docs/specs/opac/oai-pmh.md).

use std::sync::Arc;

use axum::Form;
use axum::extract::{Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use elzevir_app::oai::{OaiCatalog, OaiSettings, respond};

use crate::AppState;

/// The most bytes the arguments of a request may carry (item 3).
const MAX_FORM: usize = 8 * 1024;

/// What the log says of one request (item 19).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OaiEvent {
    /// The verb, when it was one.
    pub verb: Option<&'static str>,
    /// The format asked for.
    pub format: Option<&'static str>,
    /// `answered`, an OAI error code or `unavailable`.
    pub outcome: &'static str,
    /// How many records or headers went out.
    pub records: usize,
    /// Records whose MARC could not be written, sent as deleted.
    pub broken: Vec<i64>,
}

/// The repository: its records, its settings and where its log goes.
#[derive(Clone)]
pub struct OaiRepository {
    /// The records.
    pub catalog: Arc<dyn OaiCatalog>,
    /// `[oai]`, checked.
    pub settings: Arc<OaiSettings>,
    /// The log.
    pub log: Arc<dyn Fn(OaiEvent) + Send + Sync>,
}

/// `GET /oai`.
pub(crate) async fn get(
    State(state): State<AppState>,
    Query(arguments): Query<Vec<(String, String)>>,
) -> Response {
    answer(&state, arguments).await
}

/// `POST /oai`, with the arguments as a form.
pub(crate) async fn post(
    State(state): State<AppState>,
    Form(arguments): Form<Vec<(String, String)>>,
) -> Response {
    answer(&state, arguments).await
}

async fn answer(state: &AppState, arguments: Vec<(String, String)>) -> Response {
    let Some(repository) = state.oai.clone() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let size: usize = arguments
        .iter()
        .map(|(name, value)| name.len() + value.len())
        .sum();
    if size > MAX_FORM {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    }
    let worker = repository.clone();
    let answered = tokio::task::spawn_blocking(move || {
        respond(
            &worker.settings,
            worker.catalog.as_ref(),
            &arguments,
            jiff::Timestamp::now(),
        )
    })
    .await;
    if let Ok(Ok(answer)) = answered {
        (repository.log)(OaiEvent {
            verb: answer.verb,
            format: answer.format,
            outcome: answer.outcome,
            records: answer.records,
            broken: answer.broken,
        });
        (
            [(header::CONTENT_TYPE, "text/xml; charset=utf-8")],
            answer.xml,
        )
            .into_response()
    } else {
        (repository.log)(OaiEvent {
            verb: None,
            format: None,
            outcome: "unavailable",
            records: 0,
            broken: Vec::new(),
        });
        (StatusCode::SERVICE_UNAVAILABLE, "catalog unavailable").into_response()
    }
}
