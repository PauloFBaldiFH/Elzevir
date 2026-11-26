// SPDX-License-Identifier: GPL-3.0-or-later
//! The citation and interchange formats of one record, for the staff and for the OPAC
//! (docs/specs/catalogo/citacao-e-intercambio.md, items 1 to 5).

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use elzevir_app::catalog::CatalogReader;
use elzevir_app::citation::{CitationFormat, cite};
use elzevir_app::export::{ExportOptions, RecordExportError, RecordSource, marc_record};
use elzevir_domain::catalog::InstanceId;
use serde::Deserialize;

use crate::AppState;
use crate::staff::current_session;

/// `?format=`.
#[derive(Deserialize)]
pub(crate) struct FormatParams {
    format: Option<String>,
}

/// Why the record did not come out.
enum Refusal {
    /// The Instance does not exist, or the OPAC does not show it.
    Missing,
    /// The record cannot be written as MARC, with the reason.
    Unwritable(String),
    /// The catalog could not be read.
    Unavailable,
}

/// `GET /equipe/acervo/instancia/{id}/citacao?format=ris|bibtex|mods|dc` (item 1).
pub(crate) async fn staff(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Query(params): Query<FormatParams>,
) -> Response {
    if current_session(&state, &headers).is_none() {
        return Redirect::to("/equipe/login").into_response();
    }
    let Some(format) = params.format.as_deref().and_then(CitationFormat::of) else {
        return (StatusCode::BAD_REQUEST, "unknown format").into_response();
    };
    match written(&state, &headers, InstanceId(id), format, None).await {
        Ok(text) => attachment(id, format, text),
        Err(Refusal::Missing) => (StatusCode::NOT_FOUND, "no such record").into_response(),
        // Staff only: the reason helps the cataloguer find what to fix in the record.
        Err(Refusal::Unwritable(failure)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("the record cannot be exported: {failure}"),
        )
            .into_response(),
        Err(Refusal::Unavailable) => {
            (StatusCode::SERVICE_UNAVAILABLE, "catalog unavailable").into_response()
        }
    }
}

/// `GET /record/{id}/export?format=ris|bibtex|mods|dc` (item 2): only what the OPAC
/// shows, and no reason for the visitor when the record cannot be written.
pub(crate) async fn opac(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(params): Query<FormatParams>,
) -> Response {
    let Ok(id) = id.parse::<i64>() else {
        return crate::opac::not_found(&state);
    };
    let Some(format) = params.format.as_deref().and_then(CitationFormat::of) else {
        return (StatusCode::BAD_REQUEST, "unknown format").into_response();
    };
    let shown = Arc::clone(&state.inner.opac.catalog);
    match written(&state, &headers, InstanceId(id), format, Some(shown)).await {
        Ok(text) => attachment(id, format, text),
        Err(Refusal::Missing | Refusal::Unwritable(_)) => crate::opac::not_found(&state),
        Err(Refusal::Unavailable) => crate::opac::unavailable(),
    }
}

/// The record in `format`; with `shown`, only when the OPAC shows the Instance.
async fn written(
    state: &AppState,
    headers: &HeaderMap,
    id: InstanceId,
    format: CitationFormat,
    shown: Option<Arc<dyn CatalogReader>>,
) -> Result<String, Refusal> {
    let records: Arc<dyn RecordSource + Send + Sync> = Arc::clone(&state.inner.staff_ports.records);
    let origin = crate::catalog::request_origin(state, headers);
    let worker_state = state.clone();
    tokio::task::spawn_blocking(move || {
        if let Some(catalog) = shown {
            let visible = catalog
                .instance(id)
                .map_err(|_| Refusal::Unavailable)?
                .is_some_and(|instance| !instance.opac_hidden);
            if !visible {
                return Err(Refusal::Missing);
            }
        }
        let record = match marc_record(&*records, id, &ExportOptions::default().limits) {
            Ok(Some(record)) => record,
            Ok(None) => return Err(Refusal::Missing),
            Err(RecordExportError::Failure(failure)) => {
                return Err(Refusal::Unwritable(failure.to_string()));
            }
            Err(RecordExportError::Read(_)) => return Err(Refusal::Unavailable),
        };
        let deposit = origin.and_then(|origin| deposit_url(&worker_state, id, &origin));
        Ok(cite(&record, id, deposit.as_deref(), format))
    })
    .await
    .unwrap_or(Err(Refusal::Unavailable))
}

/// The address of the Instance's published deposit, when it has one (item 5). A
/// repository that cannot answer leaves it out rather than failing the export.
fn deposit_url(state: &AppState, id: InstanceId, origin: &str) -> Option<String> {
    let deposit = state
        .repository
        .as_ref()?
        .port
        .for_instance(id)
        .ok()
        .flatten()?;
    Some(format!("{origin}/repository/{:032x}", deposit.0))
}

fn attachment(id: i64, format: CitationFormat, text: String) -> Response {
    (
        [
            (header::CONTENT_TYPE, format.content_type().to_owned()),
            (
                header::CONTENT_DISPOSITION,
                format!(
                    "attachment; filename=\"elzevir-{id}{}\"",
                    format.file_suffix()
                ),
            ),
        ],
        text,
    )
        .into_response()
}
