// SPDX-License-Identifier: GPL-3.0-or-later
//! Batch actions on the families selected in the listing (docs/specs/catalogo/lote.md).

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use elzevir_app::batch::{instances_of, selection};
use elzevir_app::export::{ExportOptions, RecordFormat, export_selection};

use crate::AppState;
use crate::staff::current_session;

/// Response header naming the Instances left out of an export (lote.md, item 8).
const NOT_EXPORTED: &str = "x-elzevir-not-exported";

/// `GET /equipe/acervo/lote/marc?format=marc21|marcxml&head=…`: the records of the
/// selected families as one attachment (lote.md, items 5 to 8).
pub(crate) async fn export_marc(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(pairs): Query<Vec<(String, String)>>,
) -> Response {
    if current_session(&state, &headers).is_none() {
        return Redirect::to("/equipe/login").into_response();
    }
    let format = pairs
        .iter()
        .rev()
        .find(|(name, _)| name == "format")
        .map(|(_, value)| value.as_str());
    let (format, content_type, extension) = match format {
        None | Some("marc21") => (RecordFormat::Marc21, "application/marc", "mrc"),
        Some("marcxml") => (
            RecordFormat::MarcXml,
            "application/marcxml+xml; charset=utf-8",
            "xml",
        ),
        Some(_) => return (StatusCode::BAD_REQUEST, "unknown format").into_response(),
    };
    let heads = selection(
        pairs
            .iter()
            .filter(|(name, _)| name == "head")
            .map(|(_, value)| value.as_str()),
    );
    if heads.is_empty() {
        return (StatusCode::BAD_REQUEST, "no family selected").into_response();
    }
    let listing = Arc::clone(&state.inner.staff_ports.listing);
    let records = Arc::clone(&state.inner.staff_ports.records);
    let written = tokio::task::spawn_blocking(move || {
        let instances = instances_of(&*listing, &heads)?;
        export_selection(
            &*records,
            &instances,
            format,
            &ExportOptions::default().limits,
        )
    })
    .await;
    let Ok(Ok(export)) = written else {
        return (StatusCode::SERVICE_UNAVAILABLE, "catalog unavailable").into_response();
    };
    let left_out = export
        .failures
        .iter()
        .map(|failed| failed.instance.0.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let mut response = (
        [
            (header::CONTENT_TYPE, content_type.to_owned()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"elzevir-lote.{extension}\""),
            ),
        ],
        export.bytes,
    )
        .into_response();
    if let Ok(value) = left_out.parse()
        && !left_out.is_empty()
    {
        response.headers_mut().insert(NOT_EXPORTED, value);
    }
    response
}
