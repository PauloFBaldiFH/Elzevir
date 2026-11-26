// SPDX-License-Identifier: GPL-3.0-or-later
//! The SRU server over the real store and index, with the 200 records of the staff export
//! (docs/specs/opac/sru.md): a client finds the records the OPAC shows, and nothing it
//! hides. Elzevir's own SRU client asks it too.
#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "integration test: helper functions fail the test on error"
)]

mod common;

use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use elzevir_app::catalog::{CatalogCommand, CatalogReader, InstanceChange, InstanceUpdate};
use elzevir_app::commands::WriteCommand;
use elzevir_app::drafts::{DraftField, DraftQuery, DraftSource, SourceOutcome, TypedQuery};
use elzevir_app::memory::{MemoryProbe, MemoryReport};
use elzevir_app::parallelism::Parallelism;
use elzevir_app::search::{IndexOptions, SearchIndex, SearchQuery, SearchScope, index_pending};
use elzevir_app::sru::SruSettings;
use elzevir_domain::catalog::InstanceId;
use elzevir_search::TantivyIndex;
use elzevir_sources::{
    DEFAULT_TIMEOUT, Dialer, DraftServer, DraftSources, HttpAnswer, SruServer as SruSource,
    SruVersion, Transport, TransportError,
};
use elzevir_store::ElzevirStore;
use elzevir_web::{AppState, OpacPorts, SruEvent, SruServer, StaffPorts, WebConfig, router};
use elzevir_web_templates::I18n;
use http_body_util::BodyExt;
use tower::ServiceExt;

use common::imported;

struct NoCounters;

impl MemoryProbe for NoCounters {
    fn report(&self) -> MemoryReport {
        MemoryReport {
            allocator: "system",
            counters: None,
        }
    }
}

type Events = Arc<Mutex<Vec<SruEvent>>>;

struct Catalog {
    _dir: tempfile::TempDir,
    store: Arc<ElzevirStore>,
    index: Arc<TantivyIndex>,
}

impl Catalog {
    fn new() -> Self {
        let (dir, store) = imported();
        let store = Arc::new(store);
        let index = Arc::new(TantivyIndex::in_memory(Parallelism::Serial).unwrap());
        let catalog = Self {
            _dir: dir,
            store,
            index,
        };
        catalog.reindex();
        catalog
    }

    fn reindex(&self) {
        let options = IndexOptions {
            parallelism: Parallelism::Serial,
            batch: NonZeroUsize::new(1024).unwrap(),
        };
        index_pending(&*self.store, &*self.store, &*self.index, &options).unwrap();
    }

    fn state(&self) -> AppState {
        let store = &self.store;
        AppState::new(
            Arc::clone(store) as _,
            Arc::new(NoCounters),
            OpacPorts {
                catalog: Arc::clone(store) as _,
                index: Arc::clone(&self.index) as _,
            },
            StaffPorts {
                listing: Arc::clone(store) as _,
                records: Arc::clone(store) as _,
                editor: Arc::clone(store) as _,
                suggestions: Arc::clone(store) as _,
                items: Arc::clone(store) as _,
                statements: Arc::clone(store) as _,
                history: Arc::clone(store) as _,
                authorities: Arc::clone(store) as _,
                work_lists: Arc::clone(store) as _,
            },
            I18n::english().unwrap(),
            WebConfig::default(),
        )
    }

    fn app(&self) -> (Router, Events) {
        let events: Events = Arc::default();
        let sink = Arc::clone(&events);
        let server = SruServer {
            catalog: Arc::clone(&self.store) as _,
            index: Arc::clone(&self.index) as _,
            settings: Arc::new(SruSettings {
                host: "biblioteca.example".to_owned(),
                port: 443,
                database: "sru".to_owned(),
                title: "Biblioteca de exemplo".to_owned(),
                default_records: 3,
                max_records: 20,
            }),
            log: Arc::new(move |event| sink.lock().unwrap().push(event)),
        };
        (router(self.state().with_sru(server)), events)
    }

    /// The Instance the OPAC lists first for `words`.
    fn first(&self, words: &str) -> InstanceId {
        self.index
            .search(&SearchQuery::words(words), SearchScope::Staff, 0, 1)
            .unwrap()
            .hits[0]
            .instance
    }

    fn hide(&self, instance: InstanceId) {
        let version = self.store.instance(instance).unwrap().unwrap().version;
        self.store
            .submit(WriteCommand::Catalog(CatalogCommand::UpdateInstance(
                Box::new(InstanceUpdate {
                    instance,
                    row_version: version,
                    changes: vec![InstanceChange::OpacHidden(true)],
                }),
            )))
            .unwrap()
            .wait()
            .unwrap();
    }
}

/// One request, on a runtime of its own: the store's writer is waited for synchronously,
/// which cannot happen inside a runtime, so the tests stay synchronous.
fn send(app: &Router, request: Request<Body>) -> (StatusCode, String, String) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime.block_on(async {
        let response = app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let kind = response
            .headers()
            .get(header::CONTENT_TYPE)
            .map(|value| value.to_str().unwrap().to_owned())
            .unwrap_or_default();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, kind, String::from_utf8(bytes.to_vec()).unwrap())
    })
}

fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

fn get(app: &Router, arguments: &[(&str, &str)]) -> String {
    let query: Vec<String> = arguments
        .iter()
        .map(|(name, value)| format!("{name}={}", encode(value)))
        .collect();
    let uri = format!("/sru?{}", query.join("&"));
    let (status, kind, body) = send(app, Request::get(uri).body(Body::empty()).unwrap());
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(kind, "text/xml; charset=utf-8");
    body
}

fn search(app: &Router, query: &str, extra: &[(&str, &str)]) -> String {
    let mut arguments = vec![
        ("operation", "searchRetrieve"),
        ("version", "1.2"),
        ("query", query),
    ];
    arguments.extend_from_slice(extra);
    get(app, &arguments)
}

fn element<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("<zs:{name}>");
    let rest = xml.split(open.as_str()).nth(1)?;
    rest.split(format!("</zs:{name}>").as_str()).next()
}

fn total(xml: &str) -> usize {
    element(xml, "numberOfRecords").unwrap().parse().unwrap()
}

/// The control numbers (`001`) of the records: the MARC carries no Instance number.
fn numbers(xml: &str) -> Vec<String> {
    xml.split("<zs:recordData>")
        .skip(1)
        .map(|data| {
            data.split("tag=\"001\">").nth(1).map_or_else(
                || "?".to_owned(),
                |rest| rest.split('<').next().unwrap().to_owned(),
            )
        })
        .collect()
}

fn diagnostic(xml: &str) -> Option<u16> {
    let rest = xml.split("info:srw/diagnostic/1/").nth(1)?;
    rest.split('<').next()?.parse().ok()
}

#[test]
fn explain_describes_the_server() {
    let catalog = Catalog::new();
    let (app, _) = catalog.app();
    let explained = get(&app, &[]);
    assert!(explained.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<zs:explainResponse xmlns:zs=\"http://www.loc.gov/zing/srw/\"><zs:version>1.2</zs:version>"));
    for part in [
        "<zs:recordSchema>http://explain.z3950.org/dtd/2.0/</zs:recordSchema>",
        "<host>biblioteca.example</host><port>443</port><database>sru</database>",
        "<title>Biblioteca de exemplo</title>",
        "<name set=\"dc\">title</name>",
        "<name set=\"bath\">isbn</name>",
        "<schema identifier=\"info:srw/schema/1/marcxml-v1.1\" name=\"marcxml\">",
        "<default type=\"numberOfRecords\">3</default>",
        "<setting type=\"maximumRecords\">20</setting>",
    ] {
        assert!(explained.contains(part), "{part}");
    }
    let v2 = get(&app, &[("operation", "explain"), ("version", "2.0")]);
    assert!(v2.contains("<sru:explainResponse xmlns:sru=\"http://docs.oasis-open.org/ns/search-ws/sruResponse\"><sru:version>2.0</sru:version>"));
    assert!(v2.contains("<sru:recordXMLEscaping>xml</sru:recordXMLEscaping>"));
}

#[test]
fn fields_relations_and_booleans_find_the_records() {
    let catalog = Catalog::new();
    let (app, events) = catalog.app();
    let label = |instance: InstanceId| {
        numbers(&search(&app, &format!("rec.id={}", instance.0), &[])).remove(0)
    };
    let perfil = label(catalog.first("anuario estatistico"));

    let by_title = search(&app, "dc.title=\"anuario estatistico\"", &[]);
    assert!(numbers(&by_title).contains(&perfil), "{by_title}");
    let by_author = search(&app, "dc.creator=\"foz do iguacu prefeitura\"", &[]);
    assert!(numbers(&by_author).contains(&perfil));
    // A phrase keeps the order.
    assert_eq!(
        total(&search(&app, "dc.creator adj \"iguacu foz\"", &[])),
        0
    );
    assert!(numbers(&search(&app, "dc.creator adj \"foz do iguacu\"", &[])).contains(&perfil));
    assert_eq!(
        numbers(&search(&app, "dc.barcode=1511", &[])),
        std::slice::from_ref(&perfil)
    );
    assert_eq!(
        numbers(&search(&app, "dc.date=1995 and dc.barcode=1511", &[])),
        std::slice::from_ref(&perfil)
    );
    assert_eq!(
        total(&search(&app, "dc.date=1996 and dc.barcode=1511", &[])),
        0
    );
    let isbn = label(catalog.first("9788560342013"));
    assert_eq!(
        numbers(&search(&app, "bath.isbn=978-85-60342-01-3", &[])),
        std::slice::from_ref(&isbn)
    );
    let either = search(
        &app,
        "(dc.barcode=1511 or dc.isbn=\"9788560342013\") not dc.barcode=1511",
        &[],
    );
    assert_eq!(numbers(&either), [isbn]);
    // A word anywhere, as the OPAC box: the barcode alone finds its record too.
    assert!(numbers(&search(&app, "1511", &[])).contains(&perfil));
    assert!(
        events
            .lock()
            .unwrap()
            .iter()
            .all(|event| event.diagnostic.is_none())
    );
}

#[test]
fn pages_follow_start_and_maximum() {
    let catalog = Catalog::new();
    let (app, events) = catalog.app();
    let all = total(&search(&app, "medici", &[("maximumRecords", "0")]));
    assert_eq!(all, 5);
    let first = search(&app, "medici", &[]);
    assert_eq!(numbers(&first).len(), 3);
    assert_eq!(element(&first, "nextRecordPosition"), Some("4"));
    assert!(first.contains("<zs:recordPosition>1</zs:recordPosition>"));
    let last = search(
        &app,
        "medici",
        &[("startRecord", "4"), ("maximumRecords", "10")],
    );
    assert_eq!(numbers(&last).len(), 2);
    assert_eq!(element(&last, "nextRecordPosition"), None);
    let mut seen = numbers(&first);
    seen.extend(numbers(&last));
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), 5);
    let beyond = search(&app, "medici", &[("startRecord", "6")]);
    assert_eq!(diagnostic(&beyond), Some(61));
    assert_eq!(total(&beyond), 5);
    // More than the maximum gets the maximum.
    let capped = search(&app, "de", &[("maximumRecords", "500")]);
    assert_eq!(numbers(&capped).len(), 20);
    let logged = events.lock().unwrap();
    assert_eq!(
        logged.first().map(|event| (event.total, event.records)),
        Some((5, 0))
    );
}

#[test]
fn records_come_in_marcxml_with_the_items_or_dublin_core() {
    let catalog = Catalog::new();
    let (app, _) = catalog.app();
    let marcxml = search(&app, "dc.barcode=1511", &[]);
    assert!(marcxml.contains("<zs:recordSchema>marcxml</zs:recordSchema><zs:recordPacking>xml</zs:recordPacking><zs:recordData><record"));
    assert!(marcxml.contains("xmlns=\"http://www.loc.gov/MARC21/slim\""));
    assert!(marcxml.contains("tag=\"952\""));
    let dc = search(
        &app,
        "dc.barcode=1511",
        &[("recordSchema", "info:srw/schema/1/dc-v1.1")],
    );
    assert!(dc.contains("<zs:recordSchema>dc</zs:recordSchema>"));
    assert!(dc.contains("<srw_dc:dc xmlns:srw_dc=\"info:srw/schema/1/dc-schema\""));
    assert!(dc.contains("<dc:title>"));
    let packed = search(&app, "dc.barcode=1511", &[("recordPacking", "string")]);
    assert!(packed.contains("<zs:recordData>&lt;record"));
    let v2 = get(
        &app,
        &[
            ("version", "2.0"),
            ("query", "dc.barcode=1511"),
            ("recordXMLEscaping", "string"),
        ],
    );
    assert!(v2.contains("<sru:searchRetrieveResponse xmlns:sru=\"http://docs.oasis-open.org/ns/search-ws/sruResponse\"><sru:version>2.0</sru:version><sru:numberOfRecords>1</sru:numberOfRecords>"));
    assert!(v2.contains(
        "<sru:recordXMLEscaping>string</sru:recordXMLEscaping><sru:recordData>&lt;record"
    ));
}

#[test]
fn a_hidden_instance_is_never_sent() {
    let catalog = Catalog::new();
    let (app, events) = catalog.app();
    let perfil = catalog.first("anuario estatistico");
    let before = search(&app, "dc.title=\"anuario estatistico\"", &[]);
    let label = numbers(&search(&app, "dc.barcode=1511", &[])).remove(0);
    assert!(numbers(&before).contains(&label));

    // Hidden but not yet reindexed: found, but sent as a diagnostic with nothing of it.
    catalog.hide(perfil);
    let stale = search(&app, "dc.barcode=1511", &[]);
    assert_eq!(total(&stale), 1);
    assert!(
        stale.contains("<zs:recordSchema>info:srw/schema/1/diagnostics-v1.1</zs:recordSchema>")
    );
    assert!(stale.contains("info:srw/diagnostic/1/64"));
    assert!(!stale.contains("Anu"));
    assert!(!stale.contains("1511</"));
    assert_eq!(
        events.lock().unwrap().last().unwrap().unavailable,
        [perfil.0]
    );

    catalog.reindex();
    assert_eq!(total(&search(&app, "dc.barcode=1511", &[])), 0);
    let after = search(&app, "dc.title=\"anuario estatistico\"", &[]);
    assert_eq!(total(&after), total(&before) - 1);
    assert!(!numbers(&after).contains(&label));
}

#[test]
fn bad_requests_get_their_diagnostics() {
    let catalog = Catalog::new();
    let (app, _) = catalog.app();
    let cases: &[(&[(&str, &str)], u16)] = &[
        (&[("operation", "scan"), ("version", "1.2")], 4),
        (
            &[
                ("operation", "searchRetrieve"),
                ("version", "3.0"),
                ("query", "a"),
            ],
            5,
        ),
        (&[("query", "a"), ("maximumRecords", "many")], 6),
        (&[("operation", "searchRetrieve")], 7),
        (&[("query", "a"), ("recordXPath", "/")], 8),
        (&[("query", "a b")], 10),
        (&[("query", "(a")], 13),
        (&[("query", "dc.shelf=a")], 16),
        (&[("query", "dc.date>1990")], 19),
        (&[("query", "dc.title=\"\"")], 27),
        (&[("query", "dc.title=rio*")], 28),
        (&[("query", "a"), ("recordSchema", "mods")], 66),
        (&[("query", "a"), ("recordPacking", "json")], 71),
    ];
    for (arguments, code) in cases {
        let body = get(&app, arguments);
        assert_eq!(diagnostic(&body), Some(*code), "{arguments:?}: {body}");
        assert!(
            body.contains(
                "<diag:diagnostic xmlns:diag=\"http://www.loc.gov/zing/srw/diagnostic/\">"
            )
        );
    }
    let refused = get(&app, &[("query", "dc.shelf=a")]);
    assert!(refused.contains("<zs:numberOfRecords>0</zs:numberOfRecords><zs:diagnostics><diag:diagnostic xmlns:diag=\"http://www.loc.gov/zing/srw/diagnostic/\"><diag:uri>info:srw/diagnostic/1/16</diag:uri><diag:details>dc.shelf</diag:details><diag:message>Unsupported index</diag:message></diag:diagnostic></zs:diagnostics>"));

    // POST carries the same parameters as a form.
    let (status, _, body) = send(
        &app,
        Request::post("/sru")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(
                "operation=searchRetrieve&version=1.2&query=dc.barcode%3D1511",
            ))
            .unwrap(),
    );
    assert_eq!(status, StatusCode::OK);
    assert_eq!(total(&body), 1);
}

#[test]
fn without_the_sru_section_there_is_no_server() {
    let catalog = Catalog::new();
    let app = router(catalog.state());
    let (status, _, _) = send(
        &app,
        Request::get("/sru?operation=explain")
            .body(Body::empty())
            .unwrap(),
    );
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Elzevir's SRU client, its requests sent to the router instead of the network.
struct Routed(Router);

impl Transport for Routed {
    fn get(&self, url: &str, _: Duration) -> Result<HttpAnswer, TransportError> {
        let path = url
            .strip_prefix("https://biblioteca.example")
            .ok_or(TransportError::NoResponse)?;
        let (status, _, body) = send(&self.0, Request::get(path).body(Body::empty()).unwrap());
        Ok(HttpAnswer {
            status: status.as_u16(),
            body: body.into_bytes(),
        })
    }
}

struct NoDialer;

impl Dialer for NoDialer {
    type Stream = std::io::Cursor<Vec<u8>>;

    fn dial(&self, _: &str, _: u16, _: Instant) -> std::io::Result<Self::Stream> {
        Err(std::io::ErrorKind::ConnectionRefused.into())
    }
}

#[test]
fn elzevirs_own_client_finds_the_records() {
    let catalog = Catalog::new();
    let (app, _) = catalog.app();
    let server = |version| {
        DraftServer::Sru(SruSource {
            name: format!("Elzevir {version:?}"),
            url: "https://biblioteca.example/sru".to_owned(),
            version,
            record_schema: "marcxml".to_owned(),
            timeout: DEFAULT_TIMEOUT,
            fields: HashMap::from([
                (DraftField::Isbn, "bath.isbn".to_owned()),
                (DraftField::Title, "dc.title".to_owned()),
                (DraftField::Author, "dc.creator".to_owned()),
            ]),
        })
    };
    let sources = DraftSources::new(
        vec![server(SruVersion::V1_2), server(SruVersion::V2_0)],
        Routed(app),
        NoDialer,
    );
    let query = DraftQuery::new(&TypedQuery {
        title: "anuario estatistico",
        author: "foz do iguacu",
        ..TypedQuery::default()
    })
    .unwrap();
    let results = sources.search(&query, 1);
    assert_eq!(results.len(), 2);
    for result in results {
        let SourceOutcome::Found {
            total,
            drafts,
            skipped,
        } = &result.outcome
        else {
            panic!("{}: {:?}", result.source, result.outcome);
        };
        assert_eq!((*total, *skipped), (1, 0), "{}", result.source);
        assert_eq!(drafts[0].summary.title, "Perfil - Anuário Estatístico");
    }
    let by_isbn = DraftQuery::new(&TypedQuery {
        isbn: "978-85-60342-01-3",
        ..TypedQuery::default()
    })
    .unwrap();
    let SourceOutcome::Found { drafts, .. } = &sources.search(&by_isbn, 1)[0].outcome else {
        panic!("no drafts");
    };
    assert_eq!(drafts[0].summary.isbns.len(), 1);
}
