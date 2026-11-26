// SPDX-License-Identifier: GPL-3.0-or-later
//! The SRU client against recorded answers (docs/specs/catalogo/fontes-de-rascunho.md).
//! No test touches the network: the transport is a table of answers.
#![expect(
    clippy::unwrap_used,
    clippy::unnecessary_wraps,
    reason = "integration test: helpers fail the test on error and build transport answers"
)]

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use elzevir_app::authority_sources::{AuthorityQuery, AuthoritySource};
use elzevir_app::drafts::{
    DraftField, DraftQuery, DraftSource, DraftSummary, SourceOutcome, TypedQuery,
};
use elzevir_domain::catalog::SubjectKind;
use elzevir_sources::{
    DEFAULT_TIMEOUT, Dialer, DraftServer, DraftSources, HttpAnswer, SruServer, SruVersion,
    Transport, TransportError,
};

const TWO_RECORDS: &[u8] = include_bytes!("data/sru-1.2-two-records.xml");
const DIAGNOSTIC: &[u8] = include_bytes!("data/sru-1.2-diagnostic.xml");
const EMPTY: &[u8] = include_bytes!("data/sru-2.0-empty.xml");
const AUTHORITIES: &[u8] = include_bytes!("data/sru-1.2-authorities.xml");

/// Answers by server URL prefix, and remembers what was asked.
struct Recorded {
    answers: Vec<(&'static str, Result<HttpAnswer, TransportError>)>,
    asked: Mutex<Vec<(String, Duration)>>,
}

impl Transport for Recorded {
    fn get(&self, url: &str, timeout: Duration) -> Result<HttpAnswer, TransportError> {
        self.asked.lock().unwrap().push((url.to_owned(), timeout));
        self.answers
            .iter()
            .find(|(prefix, _)| url.starts_with(prefix))
            .map_or(Err(TransportError::NoResponse), |(_, answer)| {
                answer.clone()
            })
    }
}

fn ok(body: &[u8]) -> Result<HttpAnswer, TransportError> {
    Ok(HttpAnswer {
        status: 200,
        body: body.to_vec(),
    })
}

fn server(name: &str, url: &str) -> SruServer {
    SruServer {
        name: name.to_owned(),
        url: url.to_owned(),
        version: SruVersion::V1_2,
        record_schema: "marcxml".to_owned(),
        timeout: DEFAULT_TIMEOUT,
        fields: HashMap::from([
            (DraftField::Isbn, "bath.isbn".to_owned()),
            (DraftField::Title, "dc.title".to_owned()),
            (DraftField::Any, "cql.anywhere".to_owned()),
        ]),
    }
}

/// No Z39.50 server here: every connection is refused.
struct NoDialer;

impl Dialer for NoDialer {
    type Stream = std::io::Cursor<Vec<u8>>;

    fn dial(&self, _: &str, _: u16, _: Instant) -> std::io::Result<Self::Stream> {
        Err(std::io::ErrorKind::ConnectionRefused.into())
    }
}

fn sources(
    servers: Vec<SruServer>,
    answers: Vec<(&'static str, Result<HttpAnswer, TransportError>)>,
) -> DraftSources<Recorded, NoDialer> {
    DraftSources::new(
        servers.into_iter().map(DraftServer::Sru).collect(),
        Recorded {
            answers,
            asked: Mutex::new(Vec::new()),
        },
        NoDialer,
    )
}

fn isbn_and_title() -> DraftQuery {
    DraftQuery::new(&TypedQuery {
        isbn: "978-0-00-000000-2",
        title: "rio",
        ..TypedQuery::default()
    })
    .unwrap()
}

#[test]
fn a_search_brings_each_servers_drafts_with_their_origin() {
    let mut slow = server("Lenta", "https://lenta.example.org/sru");
    slow.timeout = Duration::from_secs(3);
    let asked = sources(
        vec![
            server("Exemplo", "https://sru.example.org/cat"),
            slow,
            server("Recusa", "http://recusa.example.org/sru"),
            server("Fora", "https://fora.example.org/sru"),
            server("Vazia", "https://vazia.example.org/sru"),
        ],
        vec![
            ("https://sru.example.org/cat?", ok(TWO_RECORDS)),
            (
                "https://lenta.example.org/",
                Err(TransportError::NoResponse),
            ),
            ("http://recusa.example.org/", ok(DIAGNOSTIC)),
            (
                "https://fora.example.org/",
                Ok(HttpAnswer {
                    status: 503,
                    body: Vec::new(),
                }),
            ),
            ("https://vazia.example.org/", ok(EMPTY)),
        ],
    );
    let results = asked.search(&isbn_and_title(), 2);
    let names: Vec<_> = results
        .iter()
        .map(|result| result.source.as_str())
        .collect();
    assert_eq!(names, ["Exemplo", "Lenta", "Recusa", "Fora", "Vazia"]);
    assert_eq!(asked.sources(), names);
    // One server alone answers as it does among the others.
    assert_eq!(
        asked.search_one(2, &isbn_and_title(), 2).as_ref(),
        results.get(2)
    );
    assert_eq!(asked.search_one(5, &isbn_and_title(), 2), None);

    let SourceOutcome::Found {
        total,
        drafts,
        skipped,
    } = &results[0].outcome
    else {
        panic!("{:?}", results[0].outcome);
    };
    // The third record has a broken leader: left out and counted.
    assert_eq!((*total, drafts.len(), *skipped), (37, 2, 1));
    assert!(drafts.iter().all(|draft| draft.origin == "sru:Exemplo"));
    assert_eq!(
        drafts[0].summary,
        DraftSummary {
            title: "O rio das águas claras : romance".to_owned(),
            author: Some("Lemos, Teodora, 1950-".to_owned()),
            edition: Some("2. ed".to_owned()),
            publication: Some("Curitiba · Editora Exemplo · 2020".to_owned()),
            isbns: vec!["9780000000002".to_owned()],
        },
        "{:?}",
        drafts[0].summary
    );
    assert_eq!(drafts[1].summary.title, "Contos do interior \"reunidos\"");
    assert_eq!(results[1].outcome, SourceOutcome::NoResponse);
    assert_eq!(
        results[2].outcome,
        SourceOutcome::Refused("bath.isbn: Unsupported index".to_owned())
    );
    assert_eq!(
        results[3].outcome,
        SourceOutcome::Refused("HTTP 503".to_owned())
    );
    assert_eq!(
        results[4].outcome,
        SourceOutcome::Found {
            total: 0,
            drafts: Vec::new(),
            skipped: 0
        }
    );
}

#[test]
fn the_request_carries_the_koha_cql_the_page_and_each_servers_time_limit() {
    let mut slow = server("Lenta", "https://lenta.example.org/sru");
    slow.timeout = Duration::from_secs(3);
    slow.version = SruVersion::V2_0;
    let asked = sources(
        vec![server("Exemplo", "https://sru.example.org/cat"), slow],
        Vec::new(),
    );
    asked.search(&isbn_and_title(), 2);
    let mut seen = asked_urls(&asked);
    seen.sort();
    assert_eq!(
        seen,
        [
            (
                "https://lenta.example.org/sru?operation=searchRetrieve&version=2.0\
                 &query=bath.isbn%3D%229780000000002%22%20and%20dc.title%3D%22rio%22\
                 &startRecord=21&maximumRecords=20&recordSchema=marcxml&recordXMLEscaping=xml"
                    .to_owned(),
                Duration::from_secs(3)
            ),
            (
                "https://sru.example.org/cat?operation=searchRetrieve&version=1.2\
                 &query=bath.isbn%3D%229780000000002%22%20and%20dc.title%3D%22rio%22\
                 &startRecord=21&maximumRecords=20&recordSchema=marcxml&recordPacking=xml"
                    .to_owned(),
                DEFAULT_TIMEOUT
            ),
        ]
    );
}

fn asked_urls(sources: &DraftSources<Recorded, NoDialer>) -> Vec<(String, Duration)> {
    sources.transport().asked.lock().unwrap().clone()
}

#[test]
fn answers_that_are_not_sru_are_refusals() {
    let asked = sources(
        vec![server("Html", "https://html.example.org/")],
        vec![(
            "https://html.example.org/",
            ok(b"<html><body>oops</body></html>"),
        )],
    );
    assert_eq!(
        asked.search(&isbn_and_title(), 1)[0].outcome,
        SourceOutcome::Refused("not an SRU searchRetrieve response".to_owned())
    );
    let doctype = sources(
        vec![server("Dtd", "https://dtd.example.org/")],
        vec![(
            "https://dtd.example.org/",
            ok(b"<!DOCTYPE x [<!ENTITY a \"b\">]><x>&a;</x>"),
        )],
    );
    assert_eq!(
        doctype.search(&isbn_and_title(), 1)[0].outcome,
        SourceOutcome::Refused("unreadable SRU response".to_owned())
    );
}

#[test]
fn an_authority_search_uses_the_index_of_the_class_and_skips_other_records() {
    let mut authorities = server("Autoridades", "https://auth.example.org/sru");
    authorities.fields = HashMap::from([
        (DraftField::PersonalName, "local.personalName".to_owned()),
        (DraftField::Any, "cql.anywhere".to_owned()),
    ]);
    let asked = sources(
        vec![authorities],
        vec![("https://auth.example.org/", ok(AUTHORITIES))],
    );
    let query = AuthorityQuery::new("Lemos", SubjectKind::Person).unwrap();
    let results = asked.search_authorities(&query, 1);
    assert_eq!(asked.authority_sources(), ["Autoridades"]);
    let SourceOutcome::Found {
        total,
        drafts,
        skipped,
    } = &results.first().unwrap().outcome
    else {
        panic!("{results:?}");
    };
    // The bibliographic record is not an authority: left out and counted.
    assert_eq!((*total, drafts.len(), *skipped), (3, 2, 1));
    let headings: Vec<_> = drafts
        .iter()
        .map(|draft| draft.heading.heading.as_str())
        .collect();
    assert_eq!(
        headings,
        ["Lemos, Teodora, 1950-", "Lemos, Teodoro, 1901-1977"]
    );
    assert_eq!(
        drafts.first().unwrap().heading.identifier.as_deref(),
        Some("(XX-Ex)n  00000001")
    );
    let (url, _) = asked_urls(&asked).pop().unwrap();
    assert!(
        url.contains("query=local.personalName%3D%22Lemos%22&"),
        "{url}"
    );
    // A family has no index of its own here: `any` stands in.
    asked.search_one_authority(
        0,
        &AuthorityQuery::new("Lemos", SubjectKind::Family).unwrap(),
        1,
    );
    let (url, _) = asked_urls(&asked).pop().unwrap();
    assert!(url.contains("query=cql.anywhere%3D%22Lemos%22&"), "{url}");
}
