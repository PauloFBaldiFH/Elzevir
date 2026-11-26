// SPDX-License-Identifier: GPL-3.0-or-later
//! The Z39.50 client against recorded answers (docs/specs/catalogo/cliente-z3950.md,
//! items 18 to 22). No test touches the network: each connection replays messages kept in
//! `tests/data/z3950-*.ber`, built byte by byte from the standard's ASN.1, since the
//! development environment reaches no Z39.50 server.
#![expect(
    clippy::unwrap_used,
    reason = "integration test: helpers fail the test on error"
)]

use std::collections::HashMap;
use std::io::{self, Cursor, Read, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use elzevir_app::authority_sources::{AuthorityQuery, AuthoritySource};
use elzevir_app::drafts::{DraftQuery, DraftSource, SourceOutcome, TypedQuery};
use elzevir_domain::catalog::SubjectKind;
use elzevir_sources::ber;
use elzevir_sources::{
    DEFAULT_TIMEOUT, Dialer, DraftServer, DraftSources, HttpAnswer, RecordCharset, Transport,
    TransportError, Z3950Server,
};

const INIT_ACCEPTED: &[u8] = include_bytes!("data/z3950-init-accepted.ber");
const INIT_REFUSED: &[u8] = include_bytes!("data/z3950-init-refused.ber");
const SEARCH_FOUND: &[u8] = include_bytes!("data/z3950-search-found.ber");
const SEARCH_DIAGNOSTIC: &[u8] = include_bytes!("data/z3950-search-diagnostic.ber");
const PRESENT_RECORDS: &[u8] = include_bytes!("data/z3950-present-records.ber");
const CLOSE: &[u8] = include_bytes!("data/z3950-close.ber");
const SEARCH_ONE: &[u8] = include_bytes!("data/z3950-search-one.ber");
const PRESENT_AUTHORITIES: &[u8] = include_bytes!("data/z3950-present-authorities.ber");

/// No SRU server here.
struct NoTransport;

impl Transport for NoTransport {
    fn get(&self, _: &str, _: Duration) -> Result<HttpAnswer, TransportError> {
        Err(TransportError::NoResponse)
    }
}

/// What each host answers, in order, and the messages each one received.
#[derive(Default)]
struct Recorded {
    answers: HashMap<&'static str, Vec<&'static [u8]>>,
    received: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

struct Replay {
    host: String,
    answers: Cursor<Vec<u8>>,
    received: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl Read for Replay {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.answers.read(buffer)
    }
}

impl Write for Replay {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.received
            .lock()
            .unwrap()
            .entry(self.host.clone())
            .or_default()
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Dialer for Recorded {
    type Stream = Replay;

    fn dial(&self, host: &str, port: u16, _: Instant) -> io::Result<Replay> {
        assert_eq!(port, 210);
        let answers = self
            .answers
            .get(host)
            .ok_or(io::ErrorKind::ConnectionRefused)?;
        Ok(Replay {
            host: host.to_owned(),
            answers: Cursor::new(answers.concat()),
            received: Arc::clone(&self.received),
        })
    }
}

impl Recorded {
    /// The tags of the messages `host` received, in order.
    fn tags(&self, host: &str) -> Vec<u32> {
        let received = self.received.lock().unwrap();
        let mut rest = received.get(host).unwrap().as_slice();
        let mut tags = Vec::new();
        while !rest.is_empty() {
            let (pdu, after) = ber::read(rest).unwrap();
            tags.push(pdu.number);
            rest = after;
        }
        tags
    }
}

fn server(name: &str, host: &str) -> DraftServer {
    DraftServer::Z3950(Z3950Server {
        name: name.to_owned(),
        host: host.to_owned(),
        port: 210,
        databases: vec!["catalogo".to_owned()],
        charset: RecordCharset::Auto,
        timeout: DEFAULT_TIMEOUT,
    })
}

fn sources(
    servers: Vec<DraftServer>,
    answers: Vec<(&'static str, Vec<&'static [u8]>)>,
) -> DraftSources<NoTransport, Recorded> {
    DraftSources::new(
        servers,
        NoTransport,
        Recorded {
            answers: answers.into_iter().collect(),
            ..Recorded::default()
        },
    )
}

fn isbn() -> DraftQuery {
    DraftQuery::new(&TypedQuery {
        isbn: "978-0-00-000000-2",
        ..TypedQuery::default()
    })
    .unwrap()
}

#[test]
fn a_search_brings_the_page_of_each_z3950_server() {
    let asked = sources(
        vec![
            server("Exemplo", "z.example.org"),
            server("Fora", "fora.example.org"),
            server("Recusa", "recusa.example.org"),
            server("Diagnostico", "diag.example.org"),
            server("Fechada", "fechada.example.org"),
        ],
        vec![
            (
                "z.example.org",
                vec![INIT_ACCEPTED, SEARCH_FOUND, PRESENT_RECORDS],
            ),
            ("recusa.example.org", vec![INIT_REFUSED]),
            ("diag.example.org", vec![INIT_ACCEPTED, SEARCH_DIAGNOSTIC]),
            ("fechada.example.org", vec![INIT_ACCEPTED, CLOSE]),
        ],
    );
    let results = asked.search(&isbn(), 1);
    let SourceOutcome::Found {
        total,
        drafts,
        skipped,
    } = &results.first().unwrap().outcome
    else {
        panic!("{:?}", results.first());
    };
    // A record that is not ISO 2709 and one replaced by a diagnostic are counted.
    assert_eq!((*total, drafts.len(), *skipped), (37, 2, 2));
    assert!(drafts.iter().all(|draft| draft.origin == "z3950:Exemplo"));
    let first = &drafts.first().unwrap().summary;
    assert_eq!(first.title, "O rio das águas claras : romance");
    assert_eq!(first.isbns, ["9780000000002"]);
    // MARC-8 by its leader, brought to UTF-8.
    let second = drafts.get(1).unwrap();
    assert_eq!(second.summary.title, "Contos do serta\u{303}o");
    assert_eq!(second.record.leader.get(9), Some(&b'a'));
    // Init, Search, Present and Close.
    assert_eq!(asked_tags(&asked, "z.example.org"), [20, 22, 24, 48]);

    let outcomes: Vec<_> = results.iter().skip(1).map(|r| r.outcome.clone()).collect();
    assert_eq!(
        outcomes,
        [
            SourceOutcome::NoResponse,
            SourceOutcome::Refused("init refused".to_owned()),
            SourceOutcome::Refused("114: 1=7".to_owned()),
            SourceOutcome::Refused("closed by the server (reason 5)".to_owned()),
        ]
    );
    assert_eq!(asked_tags(&asked, "recusa.example.org"), [20, 48]);
}

fn asked_tags(sources: &DraftSources<NoTransport, Recorded>, host: &str) -> Vec<u32> {
    sources.dialer().tags(host)
}

#[test]
fn a_page_past_the_total_asks_for_no_records() {
    let asked = sources(
        vec![server("Exemplo", "z.example.org")],
        vec![("z.example.org", vec![INIT_ACCEPTED, SEARCH_FOUND])],
    );
    let result = asked.search_one(0, &isbn(), 3).unwrap();
    assert_eq!(
        result.outcome,
        SourceOutcome::Found {
            total: 37,
            drafts: Vec::new(),
            skipped: 0
        }
    );
    assert_eq!(asked_tags(&asked, "z.example.org"), [20, 22, 48]);
}

#[test]
fn a_server_that_stops_midway_gave_no_response() {
    let asked = sources(
        vec![server("Exemplo", "z.example.org")],
        vec![("z.example.org", vec![INIT_ACCEPTED, &SEARCH_FOUND[..5]])],
    );
    assert_eq!(
        asked.search_one(0, &isbn(), 1).unwrap().outcome,
        SourceOutcome::NoResponse
    );
}

#[test]
fn an_authority_search_asks_by_the_class_and_reads_the_heading() {
    let asked = sources(
        vec![server("Autoridades", "auth.example.org")],
        vec![(
            "auth.example.org",
            vec![INIT_ACCEPTED, SEARCH_ONE, PRESENT_AUTHORITIES],
        )],
    );
    let query = AuthorityQuery::new("Rio Paraná", SubjectKind::Place).unwrap();
    let result = asked.search_one_authority(0, &query, 1).unwrap();
    let SourceOutcome::Found {
        total,
        drafts,
        skipped,
    } = result.outcome
    else {
        panic!("{:?}", result.outcome);
    };
    assert_eq!((total, drafts.len(), skipped), (1, 1, 0));
    let draft = drafts.first().unwrap();
    assert_eq!(draft.origin, "z3950:Autoridades");
    assert_eq!(draft.heading.kind, SubjectKind::Place);
    // MARC-8 by its leader, brought to UTF-8.
    assert_eq!(draft.heading.heading, "Parana\u{301}, Rio");
    assert_eq!(
        draft.heading.identifier.as_deref(),
        Some("(XX-Ex)sh00000003")
    );
    assert_eq!(draft.heading.variants, ["Rio Parana\u{301}"]);
    // A place is a subject heading: Bib-1 use attribute 21 (Koha `subject`).
    let received = asked.dialer().received.lock().unwrap();
    let sent = received.get("auth.example.org").unwrap();
    let use_21 = [0x30, 0x08, 0x9F, 0x78, 0x01, 0x01, 0x9F, 0x79, 0x01, 0x15];
    assert!(sent.windows(use_21.len()).any(|bytes| bytes == use_21));
    let name = "Rio Paraná".as_bytes();
    assert!(sent.windows(name.len()).any(|bytes| bytes == name));
}
