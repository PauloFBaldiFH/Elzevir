// SPDX-License-Identifier: GPL-3.0-or-later
//! `/sru`: the SRU server of the public catalog (docs/specs/opac/sru.md).

use std::sync::Arc;

use axum::Form;
use axum::extract::{Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use elzevir_app::field_search::FieldSearch;
use elzevir_app::oai::OaiCatalog;
use elzevir_app::sru::{SruSettings, respond};

use crate::AppState;

/// The most bytes the parameters of a request may carry (item 3).
const MAX_FORM: usize = 8 * 1024;

/// What the log says of one request (item 19).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SruEvent {
    /// `explain` or `searchRetrieve`.
    pub operation: &'static str,
    /// The version answered.
    pub version: &'static str,
    /// The diagnostic that refused the request, if any.
    pub diagnostic: Option<u16>,
    /// The matches in all.
    pub total: usize,
    /// Records sent.
    pub records: usize,
    /// Instances sent as a surrogate diagnostic.
    pub unavailable: Vec<i64>,
    /// Why storage or the index failed.
    pub failure: Option<String>,
}

/// The server: the records, the index, the settings and where the log goes.
#[derive(Clone)]
pub struct SruServer {
    /// The stored records.
    pub catalog: Arc<dyn OaiCatalog>,
    /// The search by field.
    pub index: Arc<dyn FieldSearch>,
    /// `[sru]`, checked.
    pub settings: Arc<SruSettings>,
    /// The log.
    pub log: Arc<dyn Fn(SruEvent) + Send + Sync>,
}

/// `GET /sru`.
pub(crate) async fn get(
    State(state): State<AppState>,
    Query(arguments): Query<Vec<(String, String)>>,
) -> Response {
    answer(&state, arguments).await
}

/// `POST /sru`, with the parameters as a form.
pub(crate) async fn post(
    State(state): State<AppState>,
    Form(arguments): Form<Vec<(String, String)>>,
) -> Response {
    answer(&state, arguments).await
}

async fn answer(state: &AppState, arguments: Vec<(String, String)>) -> Response {
    let Some(server) = state.sru.clone() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let size: usize = arguments
        .iter()
        .map(|(name, value)| name.len() + value.len())
        .sum();
    if size > MAX_FORM {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    }
    let worker = server.clone();
    let answered = tokio::task::spawn_blocking(move || {
        respond(
            &worker.settings,
            worker.catalog.as_ref(),
            worker.index.as_ref(),
            &arguments,
        )
    })
    .await;
    if let Ok(answer) = answered {
        (server.log)(SruEvent {
            operation: answer.operation,
            version: answer.version.name(),
            diagnostic: answer.diagnostic,
            total: answer.total,
            records: answer.records,
            unavailable: answer.unavailable,
            failure: answer.failure,
        });
        (
            [(header::CONTENT_TYPE, "text/xml; charset=utf-8")],
            answer.xml,
        )
            .into_response()
    } else {
        (server.log)(SruEvent {
            operation: "unknown",
            version: "unknown",
            diagnostic: Some(1),
            total: 0,
            records: 0,
            unavailable: Vec::new(),
            failure: Some("the request's task stopped".to_owned()),
        });
        (StatusCode::SERVICE_UNAVAILABLE, "catalog unavailable").into_response()
    }
}
