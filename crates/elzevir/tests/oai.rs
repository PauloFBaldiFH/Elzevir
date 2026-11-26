// SPDX-License-Identifier: GPL-3.0-or-later
//! The OAI-PMH repository over the real store, with the 200 records of the staff export
//! (docs/specs/opac/oai-pmh.md): a harvester gets every record visible in the OPAC, and a
//! hidden one only as deleted.
#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "integration test: helper functions fail the test on error"
)]

mod common;

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use elzevir_app::catalog::{CatalogCommand, CatalogReader, InstanceChange, InstanceUpdate};
use elzevir_app::commands::WriteCommand;
use elzevir_app::memory::{MemoryProbe, MemoryReport};
use elzevir_app::oai::OaiSettings;
use elzevir_app::parallelism::Parallelism;
use elzevir_domain::catalog::InstanceId;
use elzevir_search::TantivyIndex;
use elzevir_store::ElzevirStore;
use elzevir_web::{AppState, OaiEvent, OaiRepository, OpacPorts, StaffPorts, WebConfig, router};
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

fn state(store: &Arc<ElzevirStore>) -> AppState {
    let index = Arc::new(TantivyIndex::in_memory(Parallelism::Serial).unwrap());
    AppState::new(
        Arc::clone(store) as _,
        Arc::new(NoCounters),
        OpacPorts {
            catalog: Arc::clone(store) as _,
            index: index as _,
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

fn settings(max_count: usize) -> OaiSettings {
    OaiSettings {
        repository_identifier: "biblioteca.example".to_owned(),
        base_url: "https://biblioteca.example/oai".to_owned(),
        repository_name: "Biblioteca de exemplo".to_owned(),
        admin_emails: vec!["admin@biblioteca.example".to_owned()],
        max_count,
    }
}

type Events = Arc<Mutex<Vec<OaiEvent>>>;

fn app(store: &Arc<ElzevirStore>, max_count: usize) -> (Router, Events) {
    let events: Events = Arc::default();
    let sink = Arc::clone(&events);
    let repository = OaiRepository {
        catalog: Arc::clone(store) as _,
        settings: Arc::new(settings(max_count)),
        log: Arc::new(move |event| sink.lock().unwrap().push(event)),
    };
    (router(state(store).with_oai(repository)), events)
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
        .collect::<Vec<_>>()
        .concat()
}

fn query(arguments: &[(&str, &str)]) -> String {
    arguments
        .iter()
        .map(|(name, value)| format!("{name}={}", encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

fn get(app: &Router, arguments: &[(&str, &str)]) -> String {
    let uri = format!("/oai?{}", query(arguments));
    let (status, kind, body) = send(app, Request::get(uri).body(Body::empty()).unwrap());
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(kind, "text/xml; charset=utf-8");
    body
}

/// The text between `open` and the next `close`, for every occurrence.
fn between<'a>(xml: &'a str, open: &str, close: &str) -> Vec<&'a str> {
    xml.split(open)
        .skip(1)
        .map(|rest| rest.split(close).next().unwrap())
        .collect()
}

fn token(xml: &str) -> Option<String> {
    let rest = xml.split("<resumptionToken").nth(1)?;
    let (_, after) = rest.split_once('>')?;
    after
        .split_once("</resumptionToken>")
        .map(|(token, _)| token.to_owned())
}

fn first_instance(store: &ElzevirStore) -> InstanceId {
    use elzevir_app::oai::OaiCatalog;
    store.headers(0, None, None, 1).unwrap()[0].instance
}

fn hide(store: &ElzevirStore, instance: InstanceId) {
    let version = store.instance(instance).unwrap().unwrap().version;
    store
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

#[test]
fn identify_and_the_formats_describe_the_repository() {
    let (_dir, store) = imported();
    let store = Arc::new(store);
    let (app, events) = app(&store, 50);

    let identify = get(&app, &[("verb", "Identify")]);
    assert!(identify.contains("<repositoryName>Biblioteca de exemplo</repositoryName>"));
    assert!(identify.contains("<baseURL>https://biblioteca.example/oai</baseURL>"));
    assert!(identify.contains("<protocolVersion>2.0</protocolVersion>"));
    assert!(identify.contains("<adminEmail>admin@biblioteca.example</adminEmail>"));
    assert!(identify.contains("<deletedRecord>persistent</deletedRecord>"));
    assert!(identify.contains("<granularity>YYYY-MM-DDThh:mm:ssZ</granularity>"));
    assert!(!identify.contains("0001-01-01"));

    let formats = get(&app, &[("verb", "ListMetadataFormats")]);
    for prefix in ["oai_dc", "marc21", "marcxml"] {
        assert!(formats.contains(&format!("<metadataPrefix>{prefix}</metadataPrefix>")));
    }
    let events = events.lock().unwrap();
    assert_eq!(events.len(), 2);
    assert!(events.iter().all(|event| event.outcome == "answered"));
}

#[test]
fn get_record_writes_dublin_core_and_marcxml_without_the_items() {
    let (_dir, store) = imported();
    let store = Arc::new(store);
    let (app, _) = app(&store, 50);
    let instance = first_instance(&store);
    let identifier = format!("biblioteca.example:{}", instance.0);

    let dc = get(
        &app,
        &[
            ("verb", "GetRecord"),
            ("identifier", &identifier),
            ("metadataPrefix", "oai_dc"),
        ],
    );
    assert!(dc.contains(&format!("<identifier>{identifier}</identifier>")));
    assert!(dc.contains("<oai_dc:dc"));
    assert!(dc.contains("<dc:title>"));
    assert!(!dc.contains("status=\"deleted\""));

    let marcxml = get(
        &app,
        &[
            ("verb", "GetRecord"),
            ("identifier", &identifier),
            ("metadataPrefix", "marcxml"),
        ],
    );
    assert!(marcxml.contains("<metadata><record"));
    assert!(marcxml.contains("xmlns=\"http://www.loc.gov/MARC21/slim\""));
    assert!(marcxml.contains("tag=\"245\""));
    assert!(!marcxml.contains("tag=\"952\""));
    assert!(!marcxml.contains("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<record"));

    let missing = get(
        &app,
        &[
            ("verb", "GetRecord"),
            ("identifier", "biblioteca.example:999999"),
            ("metadataPrefix", "oai_dc"),
        ],
    );
    assert!(missing.contains("<error code=\"idDoesNotExist\">"));
    let foreign = get(
        &app,
        &[
            ("verb", "GetRecord"),
            ("identifier", &format!("other.example:{}", instance.0)),
            ("metadataPrefix", "oai_dc"),
        ],
    );
    assert!(foreign.contains("<error code=\"idDoesNotExist\">"));
}

#[test]
fn a_harvest_in_pages_gets_every_record_and_a_hidden_one_as_deleted() {
    let (_dir, store) = imported();
    let store = Arc::new(store);
    let hidden = first_instance(&store);
    hide(&store, hidden);
    let (app, events) = app(&store, 60);

    let mut identifiers = BTreeSet::new();
    let mut deleted = Vec::new();
    let mut page = get(
        &app,
        &[("verb", "ListRecords"), ("metadataPrefix", "oai_dc")],
    );
    let mut pages = 1;
    loop {
        for record in between(&page, "<record>", "</record>") {
            let identifier = between(record, "<identifier>", "</identifier>")[0];
            assert!(
                identifiers.insert(identifier.to_owned()),
                "{identifier} twice"
            );
            if record.contains("<header status=\"deleted\">") {
                assert!(!record.contains("<metadata>"));
                deleted.push(identifier.to_owned());
            } else {
                assert!(record.contains("<metadata>"));
            }
        }
        match token(&page) {
            Some(next) if !next.is_empty() => {
                page = get(&app, &[("verb", "ListRecords"), ("resumptionToken", &next)]);
                pages += 1;
            }
            _ => break,
        }
    }
    assert_eq!(pages, 4);
    assert_eq!(identifiers.len(), 200);
    assert_eq!(deleted, vec![format!("biblioteca.example:{}", hidden.0)]);
    // The last page of a resumed list closes it with an empty token.
    assert!(page.contains("<resumptionToken cursor=\"180\"/>"));
    let harvested: usize = events
        .lock()
        .unwrap()
        .iter()
        .map(|event| event.records)
        .sum();
    assert_eq!(harvested, 200);

    // GetRecord of the hidden one: the header alone.
    let record = get(
        &app,
        &[
            ("verb", "GetRecord"),
            ("identifier", &format!("biblioteca.example:{}", hidden.0)),
            ("metadataPrefix", "marc21"),
        ],
    );
    assert!(record.contains("<header status=\"deleted\">"));
    assert!(!record.contains("<metadata>"));
}

#[test]
fn identifiers_follow_the_dates_asked_for() {
    let (_dir, store) = imported();
    let store = Arc::new(store);
    let (app, _) = app(&store, 1000);

    let all = get(
        &app,
        &[
            ("verb", "ListIdentifiers"),
            ("metadataPrefix", "marcxml"),
            ("from", "2000-01-01"),
            ("until", "2999-12-31"),
        ],
    );
    assert_eq!(all.matches("<header>").count(), 200);
    assert!(!all.contains("<resumptionToken"));
    assert!(!all.contains("<metadata>"));

    for (name, day) in [("from", "2999-01-01"), ("until", "2000-01-01")] {
        let none = get(
            &app,
            &[
                ("verb", "ListIdentifiers"),
                ("metadataPrefix", "oai_dc"),
                (name, day),
            ],
        );
        assert!(none.contains("<error code=\"noRecordsMatch\">"), "{name}");
    }
}

#[test]
fn bad_requests_get_the_protocol_errors() {
    let (_dir, store) = imported();
    let store = Arc::new(store);
    let (app, events) = app(&store, 50);

    let cases: &[(&[(&str, &str)], &str)] = &[
        (&[("verb", "Harvest")], "badVerb"),
        (&[], "badVerb"),
        (&[("verb", "Identify"), ("verb", "Identify")], "badVerb"),
        (&[("verb", "Identify"), ("extra", "1")], "badArgument"),
        (&[("verb", "ListRecords")], "badArgument"),
        (
            &[("verb", "ListRecords"), ("metadataPrefix", "mods")],
            "cannotDisseminateFormat",
        ),
        (
            &[
                ("verb", "ListRecords"),
                ("metadataPrefix", "oai_dc"),
                ("from", "2020-01-01"),
                ("until", "2020-01-01T00:00:00Z"),
            ],
            "badArgument",
        ),
        (
            &[("verb", "ListRecords"), ("resumptionToken", "nonsense")],
            "badResumptionToken",
        ),
        (
            &[
                ("verb", "ListRecords"),
                ("resumptionToken", "oai_dc/0////1"),
                ("metadataPrefix", "oai_dc"),
            ],
            "badArgument",
        ),
        (&[("verb", "ListSets")], "noSetHierarchy"),
    ];
    for (arguments, code) in cases {
        let body = get(&app, arguments);
        assert!(
            body.contains(&format!("<error code=\"{code}\">")),
            "{arguments:?}: {body}"
        );
    }
    // Arguments echo only when the request was valid enough.
    let bad_verb = get(&app, &[("verb", "Harvest")]);
    assert!(bad_verb.contains("<request>https://biblioteca.example/oai</request>"));
    assert_eq!(events.lock().unwrap().len(), cases.len() + 1);

    // POST carries the same arguments as a form.
    let (status, _, body) = send(
        &app,
        Request::post("/oai")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from("verb=Identify"))
            .unwrap(),
    );
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("<Identify>"));
}

#[test]
fn without_the_oai_section_there_is_no_repository() {
    let (_dir, store) = imported();
    let store = Arc::new(store);
    let app = router(state(&store));
    let (status, _, _) = send(
        &app,
        Request::get("/oai?verb=Identify")
            .body(Body::empty())
            .unwrap(),
    );
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[test]
fn a_change_moves_the_datestamp() {
    use elzevir_app::oai::OaiCatalog;
    let (_dir, store) = imported();
    let instance = first_instance(&store);
    let before = store.header(instance).unwrap().unwrap();
    assert!(!before.hidden);
    hide(&store, instance);
    let after = store.header(instance).unwrap().unwrap();
    assert!(after.hidden);
    assert!(after.datestamp >= before.datestamp);
    assert_eq!(store.earliest().unwrap().unwrap().as_str().len(), 20);
}
