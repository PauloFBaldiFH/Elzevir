// SPDX-License-Identifier: GPL-3.0-or-later
//! The page that lists the code tables and renames their codes
//! (docs/specs/catalogo/tabelas-de-codigos.md, items 14 to 19).

use std::sync::Arc;

use axum::Form;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use elzevir_app::catalog::{CatalogOutput, CatalogRefusal, CodeLists};
use elzevir_app::codes::{CodeTable, NamedCode};
use elzevir_app::editing::{Edit, EditFailed};
use elzevir_app::staff::Permission;
use elzevir_web_templates::{CodeRow, CodeTablePart, StaffCodes};
use serde::Deserialize;

use crate::security::tokens_match;
use crate::session::ActiveSession;
use crate::staff::current_session;
use crate::{AppState, render};

/// The tables in page order: the name in forms and anchors, and the heading.
const TABLES: [(CodeTable, &str, &str); 4] = [
    (CodeTable::Branch, "branches", "codes-table-branches"),
    (CodeTable::ItemType, "item-types", "codes-table-item-types"),
    (CodeTable::Location, "locations", "codes-table-locations"),
    (
        CodeTable::Collection,
        "collections",
        "codes-table-collections",
    ),
];

/// Query string of the page.
#[derive(Deserialize)]
pub(crate) struct CodesParams {
    /// Present after a change was stored.
    saved: Option<String>,
}

/// `GET /equipe/admin/codigos` (item 14).
pub(crate) async fn code_tables(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<CodesParams>,
) -> Response {
    let Some(session) = current_session(&state, &headers) else {
        return Redirect::to("/equipe/login").into_response();
    };
    if !session.identity.can(Permission::Cataloguing) {
        return StatusCode::FORBIDDEN.into_response();
    }
    page(
        &state,
        &session,
        params.saved.is_some(),
        None,
        StatusCode::OK,
    )
    .await
}

/// One posted form: a code of a table and the name it should have.
#[derive(Deserialize)]
pub(crate) struct CodeForm {
    csrf: String,
    table: String,
    code: String,
    name: String,
}

/// `POST /equipe/admin/codigos`: names one code, creating it if new (items 15 to 17).
pub(crate) async fn name_code(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<CodeForm>,
) -> Response {
    let Some(session) = current_session(&state, &headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if !tokens_match(&session.csrf, &form.csrf) || !session.identity.can(Permission::Cataloguing) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some((table, anchor, _)) = TABLES.iter().find(|(_, name, _)| *name == form.table) else {
        return (StatusCode::BAD_REQUEST, "unknown table").into_response();
    };
    let edit = Edit::Codes(vec![NamedCode {
        table: *table,
        code: form.code,
        name: form.name,
    }]);
    let editor = Arc::clone(&state.inner.staff_ports.editor);
    let actor = session.identity.id;
    let outcome = tokio::task::spawn_blocking(move || editor.apply(actor, edit)).await;
    match outcome {
        Ok(Ok(CatalogOutput::CodesNamed(_))) => {
            // The OPAC keeps pages by record version, and a name is in none (item 5).
            state.inner.records.clear();
            Redirect::to(&format!("/equipe/admin/codigos?saved=1#codes-{anchor}")).into_response()
        }
        Ok(Ok(_)) => (StatusCode::INTERNAL_SERVER_ERROR, "unexpected answer").into_response(),
        Ok(Err(EditFailed::Refused(CatalogRefusal::BadCodeName(_)))) => {
            page(
                &state,
                &session,
                false,
                Some("codes-error-name"),
                StatusCode::UNPROCESSABLE_ENTITY,
            )
            .await
        }
        Ok(Err(EditFailed::Refused(refusal))) => {
            (StatusCode::UNPROCESSABLE_ENTITY, refusal.to_string()).into_response()
        }
        Ok(Err(EditFailed::Busy | EditFailed::Unavailable(_))) | Err(_) => {
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

async fn page(
    state: &AppState,
    session: &ActiveSession,
    saved: bool,
    refused: Option<&'static str>,
    status: StatusCode,
) -> Response {
    let catalog = Arc::clone(&state.inner.opac.catalog);
    let Ok(Ok(codes)) = tokio::task::spawn_blocking(move || catalog.codes()).await else {
        return (StatusCode::SERVICE_UNAVAILABLE, "catalog unavailable").into_response();
    };
    let mut response = render(&StaffCodes {
        t: &state.inner.i18n,
        csrf: &session.csrf,
        tables: TABLES
            .iter()
            .map(|(table, name, heading_key)| CodeTablePart {
                name,
                heading_key,
                rows: rows(&codes, *table),
            })
            .collect(),
        saved,
        refused,
    });
    *response.status_mut() = status;
    response
}

/// The codes of one table with their names: item types from their table, the rest from
/// the edge.
fn rows(codes: &CodeLists, table: CodeTable) -> Vec<CodeRow> {
    if table == CodeTable::ItemType {
        return codes
            .item_types
            .iter()
            .map(|entry| CodeRow {
                code: entry.code.clone(),
                name: entry.name.clone(),
            })
            .collect();
    }
    codes
        .edge
        .iter()
        .filter(|entry| entry.table == table)
        .map(|entry| CodeRow {
            code: entry.code.clone(),
            name: entry.name.clone(),
        })
        .collect()
}
