// SPDX-License-Identifier: GPL-3.0-or-later
//! The REST API under `/api/v1`: the public catalog in JSON of the model, and its
//! `OpenAPI` contract (docs/specs/opac/api.md).
//!
//! Its own design, not the Koha `api/v1`: Koha's objects carry `biblio` columns and MARC
//! tags, which never leave the edge here.

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{delete, get, post, put};
use elzevir_app::catalog::{CodeLists, ItemView, Movement};
use elzevir_app::material::{CarrierKind, ContentKind, Material};
use elzevir_app::opac::{self, RecordPage, ResultsPage};
use elzevir_domain::catalog::{
    AgentKind, IdentifierKind, InstanceId, NoteKind, SubjectKind, WorkRelationKind,
};
use serde::{Deserialize, Serialize};

use crate::AppState;

mod patron;

/// The `OpenAPI` document, written by hand and checked against the routes by the tests.
pub(crate) const OPENAPI: &str = include_str!("openapi.json");

/// Path of the `OpenAPI` document.
const OPENAPI_PATH: &str = "/api/v1/openapi.json";
/// Path of the search.
const SEARCH_PATH: &str = "/api/v1/search";
/// Path of one record.
const INSTANCE_PATH: &str = "/api/v1/instances/{id}";
/// Path of the patron's tokens.
const TOKENS_PATH: &str = "/api/v1/patron/tokens";
/// Path of the token of the request.
const CURRENT_TOKEN_PATH: &str = "/api/v1/patron/tokens/current";
/// Path of the patron's account.
const ACCOUNT_PATH: &str = "/api/v1/patron/account";
/// Path of the patron's loans.
const LOANS_PATH: &str = "/api/v1/patron/loans";
/// Path of a loan's renewal.
const RENEWAL_PATH: &str = "/api/v1/patron/loans/{item}/renewal";
/// Path of the patron's account lines.
const DEBTS_PATH: &str = "/api/v1/patron/debts";
/// Path of the patron's holds.
const HOLDS_PATH: &str = "/api/v1/patron/holds";
/// Path of one hold.
const HOLD_PATH: &str = "/api/v1/patron/holds/{id}";
/// Path of a hold's suspension.
const SUSPENSION_PATH: &str = "/api/v1/patron/holds/{id}/suspension";

/// Largest body of a request, in bytes (an explicit input limit, spec item 8).
const MAX_BODY: usize = 4 * 1024;

/// Every operation the router answers under `/api/v1`, as path and method, each of which
/// the document describes (spec, item 2).
#[cfg(test)]
const OPERATIONS: [(&str, &str); 14] = [
    (OPENAPI_PATH, "get"),
    (SEARCH_PATH, "get"),
    (INSTANCE_PATH, "get"),
    (TOKENS_PATH, "post"),
    (CURRENT_TOKEN_PATH, "delete"),
    (ACCOUNT_PATH, "get"),
    (LOANS_PATH, "get"),
    (RENEWAL_PATH, "post"),
    (DEBTS_PATH, "get"),
    (HOLDS_PATH, "get"),
    (HOLDS_PATH, "post"),
    (HOLD_PATH, "delete"),
    (SUSPENSION_PATH, "put"),
    (SUSPENSION_PATH, "delete"),
];

/// The API routes, with JSON answers for an unknown path or method (spec, item 3).
pub(crate) fn routes() -> Router<AppState> {
    let api = Router::new()
        .route(OPENAPI_PATH, get(openapi))
        .route(SEARCH_PATH, get(search))
        .route(INSTANCE_PATH, get(instance))
        .route(TOKENS_PATH, post(patron::issue))
        .route(CURRENT_TOKEN_PATH, delete(patron::revoke))
        .route(ACCOUNT_PATH, get(patron::account))
        .route(LOANS_PATH, get(patron::loans))
        .route(RENEWAL_PATH, post(patron::renew))
        .route(DEBTS_PATH, get(patron::debts))
        .route(HOLDS_PATH, get(patron::holds).post(patron::place_hold))
        .route(HOLD_PATH, delete(patron::cancel_hold))
        .route(
            SUSPENSION_PATH,
            put(patron::suspend_hold).delete(patron::resume_hold),
        )
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .method_not_allowed_fallback(|| async {
            error(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed")
        });
    Router::new()
        .merge(api)
        .route("/api/v1/{*rest}", axum::routing::any(unknown))
}

async fn unknown() -> Response {
    error(StatusCode::NOT_FOUND, "not_found")
}

/// An error as the API tells it: `{"error": code}`.
pub(crate) fn error(status: StatusCode, code: &'static str) -> Response {
    #[derive(Serialize)]
    struct Body {
        error: &'static str,
    }
    (status, Json(Body { error: code })).into_response()
}

fn unavailable() -> Response {
    error(StatusCode::SERVICE_UNAVAILABLE, "unavailable")
}

/// `GET /api/v1/openapi.json`.
async fn openapi() -> Response {
    let mut response = OPENAPI.into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    response
}

/// Query string of the search.
#[derive(Deserialize)]
pub(crate) struct SearchParams {
    #[serde(default)]
    q: String,
    #[serde(default)]
    page: Option<String>,
}

/// One page of results (spec, item 6).
#[derive(Serialize)]
struct SearchJson {
    query: String,
    total: usize,
    page: usize,
    has_next: bool,
    results: Vec<ResultJson>,
}

#[derive(Serialize)]
struct ResultJson {
    instance: i64,
    work: i64,
    hub: Option<i64>,
    title: String,
    contributor: Option<String>,
    date: Option<String>,
    call_number: Option<String>,
    content: Option<&'static str>,
    carrier: Option<&'static str>,
    items: u32,
    available: u32,
}

impl From<ResultsPage> for SearchJson {
    fn from(page: ResultsPage) -> Self {
        let results = page
            .rows
            .into_iter()
            .map(|row| {
                let (content, carrier) = material(row.hit.material);
                ResultJson {
                    instance: row.hit.instance.0,
                    work: row.hit.work.0,
                    hub: row.hit.hub.map(|hub| hub.0),
                    title: row.hit.title,
                    contributor: row.hit.main_contributor,
                    date: row.hit.date,
                    call_number: row.hit.call_number,
                    content,
                    carrier,
                    items: row.items,
                    available: row.available,
                }
            })
            .collect();
        Self {
            query: page.query,
            total: page.total,
            page: page.page,
            has_next: page.has_next,
            results,
        }
    }
}

fn material(material: Material) -> (Option<&'static str>, Option<&'static str>) {
    (
        material.content.map(ContentKind::name),
        material.carrier.map(CarrierKind::name),
    )
}

/// `GET /api/v1/search?q=&page=`.
async fn search(State(state): State<AppState>, Query(params): Query<SearchParams>) -> Response {
    // A page that is not a number is read as the first one, as on the OPAC.
    let page = params
        .page
        .and_then(|page| page.parse::<usize>().ok())
        .unwrap_or(1);
    let ports = state.inner.opac.clone();
    let found = tokio::task::spawn_blocking(move || {
        opac::search(&*ports.index, &*ports.catalog, &params.q, page)
    })
    .await;
    match found {
        Ok(Ok(results)) => Json(SearchJson::from(results)).into_response(),
        Ok(Err(_)) | Err(_) => unavailable(),
    }
}

/// One record (spec, item 7).
#[derive(Serialize)]
struct InstanceJson {
    instance: i64,
    work: i64,
    hub: Option<i64>,
    title: String,
    work_title: String,
    other_titles: Vec<String>,
    publisher: Option<String>,
    date: Option<String>,
    classification: Option<String>,
    summary: Option<String>,
    audience: Option<String>,
    genres: Vec<String>,
    languages: Vec<String>,
    content: Option<&'static str>,
    carrier: Option<&'static str>,
    contributors: Vec<Named<AgentKind>>,
    subjects: Vec<Named<SubjectKind>>,
    identifiers: Vec<Valued<IdentifierKind>>,
    notes: Vec<Valued<NoteKind>>,
    relations: Vec<Relation>,
    items: Vec<ItemJson>,
}

#[derive(Serialize)]
struct Named<K> {
    kind: K,
    name: String,
}

#[derive(Serialize)]
struct Valued<K> {
    kind: K,
    value: String,
}

#[derive(Serialize)]
struct Relation {
    kind: WorkRelationKind,
    work: i64,
}

/// An Item as the public sees it: never its barcode, inventory number or who has it.
#[derive(Serialize)]
struct ItemJson {
    branch: String,
    location: Option<String>,
    call_number: Option<String>,
    available: bool,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    due: Option<String>,
}

impl InstanceJson {
    fn new(page: RecordPage, codes: &CodeLists) -> Self {
        let RecordPage { instance, items } = page;
        let (content, carrier) = material(instance.material);
        Self {
            instance: instance.id.0,
            work: instance.work.0,
            hub: instance.hub.map(|hub| hub.0),
            title: instance.title,
            work_title: instance.work_title,
            other_titles: instance.other_titles,
            publisher: instance.publisher,
            date: instance.date,
            classification: instance.classification,
            summary: instance.summary,
            audience: instance.audience,
            genres: instance.genres,
            languages: instance.languages,
            content,
            carrier,
            contributors: named(instance.contributors),
            subjects: named(instance.subjects),
            identifiers: valued(instance.identifiers),
            notes: valued(instance.notes),
            relations: instance
                .relations
                .into_iter()
                .map(|(kind, work)| Relation { kind, work: work.0 })
                .collect(),
            items: items.iter().map(|item| item_json(item, codes)).collect(),
        }
    }
}

fn named<K>(pairs: Vec<(K, String)>) -> Vec<Named<K>> {
    pairs
        .into_iter()
        .map(|(kind, name)| Named { kind, name })
        .collect()
}

fn valued<K>(pairs: Vec<(K, String)>) -> Vec<Valued<K>> {
    pairs
        .into_iter()
        .map(|(kind, value)| Valued { kind, value })
        .collect()
}

fn item_json(item: &ItemView, codes: &CodeLists) -> ItemJson {
    ItemJson {
        branch: codes.branch(item.home_branch).to_owned(),
        location: item.location.map(|id| codes.location(id).to_owned()),
        call_number: item.call_number.clone(),
        available: item.state().is_available(),
        status: item_status(item),
        due: item.due.map(|due| due.to_string()),
    }
}

/// An Item's status in the order the OPAC tells it: lost first, then where it is in
/// circulation, then the gravest status number (busca-e-registro.md, item 7).
///
/// Koha: the order of `item-status.inc` in the OPAC.
fn item_status(item: &ItemView) -> &'static str {
    let status = item.status;
    if status.lost != 0 {
        return "lost";
    }
    match (item.due, item.movement) {
        (Some(_), _) => "on_loan",
        (None, Movement::InTransit) => "in_transit",
        (None, Movement::OnHoldShelf) => "on_hold_shelf",
        (None, Movement::Shelved) => {
            if status.withdrawn != 0 {
                "withdrawn"
            } else if status.damaged != 0 {
                "damaged"
            } else if status.restricted != 0 {
                "restricted"
            } else if status.not_for_loan != 0 {
                "not_for_loan"
            } else {
                "available"
            }
        }
    }
}

/// `GET /api/v1/instances/{id}`.
async fn instance(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let Ok(id) = id.parse::<i64>().map(InstanceId) else {
        return error(StatusCode::NOT_FOUND, "not_found");
    };
    let ports = state.inner.opac.clone();
    let read = tokio::task::spawn_blocking(move || {
        let Some(page) = opac::record(&*ports.catalog, id).map_err(drop)? else {
            return Ok(None);
        };
        let codes = ports.catalog.codes().map_err(drop)?;
        Ok::<_, ()>(Some(InstanceJson::new(page, &codes)))
    })
    .await;
    match read {
        Ok(Ok(Some(record))) => Json(record).into_response(),
        Ok(Ok(None)) => error(StatusCode::NOT_FOUND, "not_found"),
        Ok(Err(())) | Err(_) => unavailable(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_document_names_every_route_and_only_them() {
        let document: serde_json::Value = serde_json::from_str(OPENAPI).unwrap_or_default();
        assert_eq!(document["openapi"], "3.0.3");
        let mut documented: Vec<(&str, &str)> = document["paths"]
            .as_object()
            .into_iter()
            .flatten()
            .flat_map(|(path, operations)| {
                operations
                    .as_object()
                    .into_iter()
                    .flatten()
                    .map(move |(method, _)| (path.as_str(), method.as_str()))
            })
            .collect();
        documented.sort_unstable();
        let mut routed = OPERATIONS.to_vec();
        routed.sort_unstable();
        assert_eq!(documented, routed);
    }

    /// The values a schema of the document allows, as strings.
    fn allowed(schema: &str) -> Vec<String> {
        let document: serde_json::Value = serde_json::from_str(OPENAPI).unwrap_or_default();
        let schema = &document["components"]["schemas"][schema];
        let values = schema["enum"]
            .as_array()
            .or_else(|| schema["properties"]["kind"]["enum"].as_array());
        values
            .into_iter()
            .flatten()
            .filter_map(|value| value.as_str().map(str::to_owned))
            .collect()
    }

    fn names<K: Serialize>(all: &[K]) -> Vec<String> {
        all.iter()
            .filter_map(|kind| serde_json::to_value(kind).ok())
            .filter_map(|value| value.as_str().map(str::to_owned))
            .collect()
    }

    #[test]
    fn the_document_lists_every_kind_the_model_has() {
        let content: Vec<String> = ContentKind::all()
            .iter()
            .map(|kind| kind.name().to_owned())
            .collect();
        assert_eq!(allowed("Content"), content);
        let carriers: Vec<String> = [CarrierKind::Print, CarrierKind::Online, CarrierKind::Disc]
            .iter()
            .map(|kind| kind.name().to_owned())
            .collect();
        assert_eq!(allowed("Carrier"), carriers);
        assert_eq!(allowed("Contributor"), names(AgentKind::ALL));
        assert_eq!(allowed("Subject"), names(SubjectKind::ALL));
        assert_eq!(allowed("Identifier"), names(IdentifierKind::ALL));
        assert_eq!(allowed("Note"), names(NoteKind::ALL));
        assert_eq!(allowed("Relation"), names(WorkRelationKind::ALL));
    }
}
