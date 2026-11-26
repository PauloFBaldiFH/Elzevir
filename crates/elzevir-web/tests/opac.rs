// SPDX-License-Identifier: GPL-3.0-or-later
//! HTTP-level tests of the OPAC search and record pages over fakes
//! (docs/specs/opac/busca-e-registro.md).
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "integration test: helper functions fail the test on error"
)]

mod common;
mod contract;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use elzevir_app::catalog::{InstanceView, ItemStatus, ItemView, Movement};
use elzevir_app::material::{CarrierKind, ContentKind, Material};
use elzevir_app::memory::{MemoryProbe, MemoryReport};
use elzevir_app::search::SearchHit;
use elzevir_app::staff::{StaffAccount, StaffDirectory, StaffDirectoryError};
use elzevir_domain::catalog::{
    AgentKind, BranchId, InstanceId, ItemId, NoteKind, PolicyId, SublocationId, WorkId,
};
use elzevir_web::{AppState, OpacPorts, StaffPorts, WebConfig, router};
use elzevir_web_templates::I18n;
use http_body_util::BodyExt;
use tower::ServiceExt;

use common::{Catalog, Index};

struct NoStaff;

impl StaffDirectory for NoStaff {
    fn by_login(&self, _: &str) -> Result<Option<StaffAccount>, StaffDirectoryError> {
        Ok(None)
    }
}

struct NoCounters;

impl MemoryProbe for NoCounters {
    fn report(&self) -> MemoryReport {
        MemoryReport {
            allocator: "system",
            counters: None,
        }
    }
}

fn view(id: i64, title: &str) -> InstanceView {
    InstanceView {
        id: InstanceId(id),
        version: 1,
        work: WorkId(id),
        work_version: 1,
        work_title: title.to_owned(),
        hub: None,
        material: Material {
            content: Some(ContentKind::Text),
            carrier: Some(CarrierKind::Print),
        },
        relations: Vec::new(),
        title: title.to_owned(),
        other_titles: Vec::new(),
        typed_notes: Vec::new(),
        publisher: Some("Editora".to_owned()),
        date: Some("1995".to_owned()),
        classification: Some("310.62".to_owned()),
        summary: None,
        audience: Some("Ensino médio.".to_owned()),
        genres: vec!["Romances".to_owned()],
        contributors: vec![(AgentKind::Person, "Silva, Ana".to_owned())],
        subjects: Vec::new(),
        identifiers: Vec::new(),
        notes: vec![
            (NoteKind::General, "Inclui índice.".to_owned()),
            (NoteKind::Contents, "Parte 1 -- Parte 2.".to_owned()),
        ],
        languages: vec!["por".to_owned()],
        opac_hidden: false,
    }
}

fn item(id: i64, status: ItemStatus) -> ItemView {
    ItemView {
        id: ItemId(id),
        barcode: Some(format!("B{id}")),
        inventory_number: None,
        home_branch: BranchId(1),
        policy: PolicyId(1),
        call_number: Some("310.62 P94".to_owned()),
        location: Some(SublocationId(1)),
        status,
        due: None,
        movement: Movement::Shelved,
    }
}

fn hit(id: i64, title: &str) -> SearchHit {
    SearchHit {
        instance: InstanceId(id),
        work: WorkId(id),
        hub: None,
        score: 1.0,
        title: title.to_owned(),
        main_contributor: Some("Prefeitura".to_owned()),
        date: Some("1995".to_owned()),
        material: Material {
            content: Some(ContentKind::Text),
            carrier: Some(CarrierKind::Print),
        },
        call_number: Some("310.62 P94".to_owned()),
    }
}

const LOST: ItemStatus = ItemStatus {
    not_for_loan: 0,
    damaged: 0,
    lost: 1,
    withdrawn: 0,
    restricted: 0,
};

fn app(catalog: Arc<Catalog>, hits: Vec<SearchHit>) -> Router {
    router(AppState::new(
        Arc::new(NoStaff),
        Arc::new(NoCounters),
        OpacPorts {
            catalog,
            index: Arc::new(Index(hits)),
        },
        StaffPorts {
            listing: Arc::new(common::Families::default()),
            records: Arc::new(common::Records(Vec::new())),
            editor: Arc::new(common::Editor::default()),
            suggestions: Arc::new(common::Suggestions),
            items: Arc::new(common::ItemGrid),
            statements: Arc::new(common::Statements),
            history: Arc::new(common::History),
            authorities: Arc::new(common::Authorities),
            work_lists: Arc::new(common::WorkLists),
        },
        I18n::english().unwrap(),
        WebConfig::default(),
    ))
}

fn catalog() -> Catalog {
    let catalog = Catalog::default();
    let mut records = catalog.records.lock().unwrap();
    records.insert(
        1,
        (
            1,
            view(1, "Perfil <Anuário>"),
            vec![item(1, ItemStatus::default()), item(2, LOST)],
        ),
    );
    records.insert(2, (1, view(2, "Sem exemplares"), Vec::new()));
    drop(records);
    catalog
}

async fn get(app: &Router, uri: &str, htmx: bool) -> (StatusCode, String) {
    let mut request = Request::get(uri);
    if htmx {
        request = request.header("HX-Request", "true");
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

#[tokio::test]
async fn results_show_availability_and_escape_titles() {
    let app = app(
        Arc::new(catalog()),
        vec![hit(1, "Perfil <Anuário>"), hit(2, "Sem exemplares")],
    );
    let (status, html) = get(&app, "/search?q=perfil", false).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("<html"));
    assert!(html.contains("2 results"));
    assert!(html.contains(r#"href="/record/1""#));
    assert!(html.contains("Perfil &#60;Anuário&#62;") || html.contains("Perfil &lt;Anuário&gt;"));
    assert!(html.contains("1 of 2 available"));
    assert!(html.contains("No copies"));
    assert!(
        html.contains(
            r#"<use href="/static/elzevir-icons.svg#content-text"></use></svg>Text</span>"#
        )
    );
    assert!(html.contains("#carrier-print"));
}

#[tokio::test]
async fn an_htmx_request_gets_only_the_result_list() {
    let app = app(Arc::new(catalog()), vec![hit(1, "Perfil")]);
    let (status, html) = get(&app, "/search?q=perfil", true).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!html.contains("<html"));
    assert!(html.trim_start().starts_with(r#"<section id="results""#));
    assert!(html.contains("1 result"));
}

#[tokio::test]
async fn pages_link_with_the_query_encoded() {
    let hits = (1..=45).map(|id| hit(id, "Livro")).collect();
    let app = app(Arc::new(Catalog::default()), hits);
    let (_, first) = get(&app, "/search?q=caf%C3%A9+%26+p%C3%A3o", false).await;
    assert!(first.contains("45 results"));
    assert!(first.contains("/search?q=caf%C3%A9%20%26%20p%C3%A3o&amp;page=2"));
    assert!(!first.contains("rel=\"prev\""));
    let (_, last) = get(&app, "/search?q=livro&page=3", false).await;
    assert!(last.contains("rel=\"prev\""));
    assert!(!last.contains("rel=\"next\""));
    // A page that is not a number is the first one.
    let (status, _) = get(&app, "/search?q=livro&page=abc", false).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn an_empty_query_shows_no_list() {
    let app = app(Arc::new(catalog()), vec![hit(1, "Perfil")]);
    let (status, html) = get(&app, "/search?q=+++", false).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!html.contains(r#"class="results""#));
    assert!(!html.contains("Nothing found"));
}

#[tokio::test]
async fn a_record_shows_its_items_and_is_rendered_once_per_version() {
    let catalog = Arc::new(catalog());
    let app = app(Arc::clone(&catalog), Vec::new());
    let (status, html) = get(&app, "/record/1", false).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        html.contains("<title>Perfil &#60;Anuário&#62;")
            || html.contains("<title>Perfil &lt;Anuário&gt;")
    );
    assert!(html.contains("Editora, 1995"));
    assert!(html.contains("Available"));
    assert!(html.contains("Lost"));
    // Names in place of codes (docs/specs/catalogo/exemplares.md, item 29).
    assert!(html.contains("<td>Main Library</td>"));
    assert!(html.contains("General stacks"));
    assert!(!html.contains(">1<"));
    assert!(html.contains("<dt>Contents</dt>\n    <dd>Parte 1 -- Parte 2.</dd>"));
    // Audience, genre, agents and item status each carry their icon (bibframe-completo.md,
    // item 17b).
    assert!(html.contains(
        r#"<dt>Audience</dt><dd><span class="chip"><svg class="badge-icon" aria-hidden="true" focusable="false"><use href="/static/elzevir-icons.svg#audience"></use></svg>Ensino médio.</span></dd>"#
    ));
    assert!(html.contains("<dt>Genre and form</dt>"));
    assert!(html.contains(r#"elzevir-icons.svg#genre"></use></svg>Romances</span>"#));
    assert!(html.contains("elzevir-icons.svg#agent-person"));
    assert!(html.contains("elzevir-icons.svg#status-available"));
    assert!(html.contains("elzevir-icons.svg#status-unavailable"));
    assert!(html.contains("<dt>Notes</dt>\n    <dd>Inclui índice.</dd>"));
    assert!(!html.contains("<Anuário>"));
    // The four citation formats (docs/specs/catalogo/citacao-e-intercambio.md, item 2).
    assert!(html.contains("<summary>Save record</summary>"));
    for format in ["ris", "bibtex", "mods", "dc"] {
        assert!(html.contains(&format!(r#"href="/record/1/export?format={format}""#)));
    }
    assert_eq!(catalog.reads.load(Ordering::SeqCst), 1);
    let (_, again) = get(&app, "/record/1", false).await;
    assert_eq!(again, html);
    assert_eq!(catalog.reads.load(Ordering::SeqCst), 1);
    let (_, empty) = get(&app, "/record/2", false).await;
    assert!(empty.contains("This record has no copies."));
}

#[tokio::test]
async fn a_new_version_is_rendered_again() {
    let catalog = Arc::new(catalog());
    let app = app(Arc::clone(&catalog), Vec::new());
    let (_, before) = get(&app, "/record/1", false).await;
    {
        let mut records = catalog.records.lock().unwrap();
        let record = records.get_mut(&1).unwrap();
        record.0 = 2;
        record.2.push(item(3, ItemStatus::default()));
    }
    let (_, after) = get(&app, "/record/1", false).await;
    assert_eq!(catalog.reads.load(Ordering::SeqCst), 2);
    assert_ne!(before, after);
    assert_eq!(after.matches(">Available<").count(), 2);
}

#[tokio::test]
async fn loans_transfers_and_holds_show_at_once_without_a_new_version() {
    let catalog = Arc::new(catalog());
    let app = app(Arc::clone(&catalog), vec![hit(1, "Perfil")]);
    let (_, before) = get(&app, "/record/1", false).await;
    assert!(before.contains(">Available<"));
    {
        let mut records = catalog.records.lock().unwrap();
        let items = &mut records.get_mut(&1).unwrap().2;
        items[0].due = Some(jiff::civil::date(2026, 12, 4));
        items.push(ItemView {
            movement: Movement::InTransit,
            ..item(3, ItemStatus::default())
        });
        items.push(ItemView {
            movement: Movement::OnHoldShelf,
            ..item(4, ItemStatus::default())
        });
    }
    let (_, after) = get(&app, "/record/1", false).await;
    // The body comes from the cache; only the Items are read again.
    assert_eq!(catalog.reads.load(Ordering::SeqCst), 1);
    assert!(after.contains("On loan, due 2026-12-04"), "{after}");
    assert!(after.contains("In transit") && after.contains("On hold for a reader"));
    assert!(!after.contains(">Available<"));
    assert!(after.contains("elzevir-icons.svg#status-on-loan"));
    let (_, results) = get(&app, "/search?q=perfil", false).await;
    assert!(results.contains("No copies available"), "{results}");
}

#[tokio::test]
async fn unknown_records_are_404() {
    let app = app(Arc::new(catalog()), Vec::new());
    for uri in ["/record/99", "/record/abc", "/record/-1"] {
        let (status, html) = get(&app, uri, false).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
        assert!(html.contains("Record not found"));
    }
}

// The REST API (docs/specs/opac/api.md).

async fn call(
    app: &Router,
    method: &str,
    uri: &str,
) -> (StatusCode, Option<String>, serde_json::Value) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let content_type = response
        .headers()
        .get("content-type")
        .map(|value| value.to_str().unwrap().to_owned());
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        content_type,
        serde_json::from_slice(&bytes).unwrap(),
    )
}

/// Calls `uri` and checks the answer against the response schema the document gives for
/// `path` and the status.
async fn checked(app: &Router, uri: &str, path: &str) -> (StatusCode, serde_json::Value) {
    let (status, content_type, body) = call(app, "GET", uri).await;
    assert_eq!(content_type.as_deref(), Some("application/json"), "{uri}");
    contract::check(path, "get", status.as_u16(), &body);
    (status, body)
}

#[tokio::test]
async fn the_api_publishes_its_contract() {
    let app = app(Arc::new(catalog()), Vec::new());
    let (status, content_type, document) = call(&app, "GET", "/api/v1/openapi.json").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        content_type.as_deref(),
        Some("application/json; charset=utf-8")
    );
    assert_eq!(document["openapi"], "3.0.3");
    assert_eq!(document, contract::document());
    // Every documented operation is routed: none falls to the unknown-route answer.
    for (path, operations) in document["paths"].as_object().unwrap() {
        for method in operations.as_object().unwrap().keys() {
            let uri = path.replace("{id}", "1").replace("{item}", "1");
            let (status, _, body) = call(&app, &method.to_uppercase(), &uri).await;
            assert_ne!(status, StatusCode::METHOD_NOT_ALLOWED, "{method} {uri}");
            assert!(
                status != StatusCode::NOT_FOUND || path.contains("/patron/"),
                "{method} {uri}: {body}"
            );
        }
    }
}

#[tokio::test]
async fn the_api_searches_like_the_opac() {
    let app = app(
        Arc::new(catalog()),
        vec![hit(1, "Perfil <Anuário>"), hit(2, "Sem exemplares")],
    );
    let (status, body) = checked(&app, "/api/v1/search?q=perfil&page=x", "/api/v1/search").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        (body["total"].as_u64(), body["page"].as_u64()),
        (Some(2), Some(1))
    );
    let first = &body["results"][0];
    assert_eq!(first["title"], "Perfil <Anuário>");
    assert_eq!(
        (first["items"].as_u64(), first["available"].as_u64()),
        (Some(2), Some(1))
    );
    assert_eq!(
        (&first["content"], &first["carrier"]),
        (&"text".into(), &"print".into())
    );
    let (_, empty) = checked(&app, "/api/v1/search?q=+", "/api/v1/search").await;
    assert_eq!(empty["results"], serde_json::json!([]));
}

#[tokio::test]
async fn the_api_gives_a_record_without_who_has_its_items() {
    let catalog = Arc::new(catalog());
    {
        let mut records = catalog.records.lock().unwrap();
        let items = &mut records.get_mut(&1).unwrap().2;
        items[0].due = Some(jiff::civil::date(2026, 12, 4));
        items.push(ItemView {
            movement: Movement::InTransit,
            location: None,
            ..item(3, ItemStatus::default())
        });
        let mut hidden = view(3, "Oculto");
        hidden.opac_hidden = true;
        records.insert(3, (1, hidden, vec![item(5, ItemStatus::default())]));
    }
    let app = app(catalog, Vec::new());
    let path = "/api/v1/instances/{id}";
    let (status, body) = checked(&app, "/api/v1/instances/1", path).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["contributors"][0],
        serde_json::json!({"kind": "person", "name": "Silva, Ana"})
    );
    assert_eq!(
        body["notes"][1],
        serde_json::json!({"kind": "contents", "value": "Parte 1 -- Parte 2."})
    );
    let items = &body["items"];
    assert_eq!(items[0]["status"], "on_loan");
    assert_eq!(items[0]["due"], "2026-12-04");
    assert_eq!(items[0]["branch"], "Main Library");
    assert_eq!(items[0]["location"], "General stacks");
    assert_eq!(items[1]["status"], "lost");
    assert_eq!(
        (&items[2]["status"], &items[2]["location"]),
        (&"in_transit".into(), &serde_json::Value::Null)
    );
    assert!(!body.to_string().contains("B1"), "no barcode: {body}");
    for uri in [
        "/api/v1/instances/3",
        "/api/v1/instances/99",
        "/api/v1/instances/abc",
    ] {
        let (status, body) = checked(&app, uri, path).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
        assert_eq!(body, serde_json::json!({"error": "not_found"}));
    }
}

#[tokio::test]
async fn unknown_api_routes_and_methods_answer_in_json() {
    let app = app(Arc::new(catalog()), Vec::new());
    let (status, _, body) = call(&app, "GET", "/api/v1/biblios/1").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body, serde_json::json!({"error": "not_found"}));
    let (status, _, body) = call(&app, "POST", "/api/v1/search").await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(body, serde_json::json!({"error": "method_not_allowed"}));
}
