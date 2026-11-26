// SPDX-License-Identifier: GPL-3.0-or-later
//! The Z39.50 server over the real store and index, with the 200 records of the staff
//! export, through a local TCP port (docs/specs/opac/z3950.md): Elzevir's own Z39.50
//! client finds the records the OPAC shows, and nothing it hides goes out.
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "integration test: helper functions fail the test on error"
)]

mod common;

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::num::NonZeroUsize;
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use elzevir_app::catalog::{CatalogCommand, CatalogReader, InstanceChange, InstanceUpdate};
use elzevir_app::commands::WriteCommand;
use elzevir_app::drafts::{DraftQuery, DraftSource, SourceOutcome, TypedQuery};
use elzevir_app::parallelism::Parallelism;
use elzevir_app::search::{IndexOptions, SearchIndex, SearchQuery, SearchScope, index_pending};
use elzevir_domain::catalog::InstanceId;
use elzevir_search::TantivyIndex;
use elzevir_sources::ber::{self, context, context_integer, context_of, sequence};
use elzevir_sources::z3950::{self, Pdu, PresentRecord, USMARC, init_request};
use elzevir_sources::z3950_server::{MARCXML, Z3950Catalog, Z3950Event, serve};
use elzevir_sources::{
    Dialer, DraftServer, DraftSources, HttpAnswer, RecordCharset, Transport, TransportError,
    Z3950Server,
};
use elzevir_store::ElzevirStore;

use common::imported;

/// Bib-1, the attribute set of every term.
const BIB1: [u32; 6] = [1, 2, 840, 10003, 3, 1];

type Events = Arc<Mutex<Vec<Z3950Event>>>;

struct Catalog {
    _dir: tempfile::TempDir,
    store: Arc<ElzevirStore>,
    index: Arc<TantivyIndex>,
}

impl Catalog {
    fn new() -> Self {
        let (dir, store) = imported();
        let catalog = Self {
            _dir: dir,
            store: Arc::new(store),
            index: Arc::new(TantivyIndex::in_memory(Parallelism::Serial).unwrap()),
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

    /// A server on a free local port, answering every connection until the test ends.
    fn listen(&self, max_records: usize) -> (SocketAddr, Events) {
        let server = Z3950Catalog {
            catalog: Arc::clone(&self.store) as _,
            index: Arc::clone(&self.index) as _,
            database: "biblios".to_owned(),
            max_records,
            version: "test".to_owned(),
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let events: Events = Arc::default();
        let sink = Arc::clone(&events);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let (server, sink) = (server.clone(), Arc::clone(&sink));
                std::thread::spawn(move || {
                    let mut stream = stream.unwrap();
                    let log = |event| sink.lock().unwrap().push(event);
                    serve(&mut stream, &server, &log).unwrap();
                });
            }
        });
        (address, events)
    }

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

/// A client's connection, after an accepted Init.
struct Client(TcpStream);

impl Client {
    fn new(address: SocketAddr) -> Self {
        let stream = TcpStream::connect(address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut client = Self(stream);
        let (answer, _) = client.ask(&init_request("test"));
        assert_eq!(answer, Pdu::InitResponse { accepted: true });
        client
    }

    /// The answer to `request`, read and as bytes.
    fn ask(&mut self, request: &[u8]) -> (Pdu, Vec<u8>) {
        self.0.write_all(request).unwrap();
        let bytes = answer(&mut self.0);
        (z3950::parse(&bytes).unwrap(), bytes)
    }

    /// The total of a search of `term` with the use attribute `use_attribute`.
    fn search(&mut self, database: &str, use_attribute: i64, term: &str) -> Pdu {
        self.ask(&search_request(database, use_attribute, term)).0
    }
}

fn answer(stream: &mut TcpStream) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        if let Some(length) = ber::frame_length(&bytes).unwrap()
            && bytes.len() >= length
        {
            return bytes;
        }
        let read = stream.read(&mut chunk).unwrap();
        assert!(read > 0, "closed after {bytes:?}");
        bytes.extend_from_slice(&chunk[..read]);
    }
}

fn search_request(database: &str, use_attribute: i64, term: &str) -> Vec<u8> {
    let attributes = [sequence(&[
        context_integer(120, 1),
        context_integer(121, use_attribute),
    ])];
    let operand = context_of(
        0,
        &[context_of(
            102,
            &[context_of(44, &attributes), context(45, term.as_bytes())],
        )],
    );
    context_of(
        22,
        &[
            context_integer(13, 0),
            context_integer(14, 1),
            context_integer(15, 0),
            context(16, &[0xFF]),
            context(17, b"default"),
            context_of(18, &[context(105, database.as_bytes())]),
            context_of(21, &[context_of(1, &[ber::oid(&BIB1), operand])]),
        ],
    )
}

fn present_request(start: i64, count: i64, syntax: &[u32], elements: &str) -> Vec<u8> {
    context_of(
        24,
        &[
            context(31, b"default"),
            context_integer(30, start),
            context_integer(29, count),
            context_of(19, &[context(0, elements.as_bytes())]),
            context(104, &ber::oid_content(syntax)),
        ],
    )
}

fn total(pdu: &Pdu) -> u64 {
    match pdu {
        Pdu::SearchResponse {
            total,
            succeeded: true,
            ..
        } => *total,
        other => panic!("not a search that succeeded: {other:?}"),
    }
}

fn condition(pdu: &Pdu) -> i64 {
    match pdu {
        Pdu::SearchResponse {
            diagnostic: Some(diagnostic),
            ..
        }
        | Pdu::PresentResponse {
            diagnostic: Some(diagnostic),
            ..
        } => diagnostic.condition,
        other => panic!("no diagnostic: {other:?}"),
    }
}

fn contains(bytes: &[u8], wanted: &[u8]) -> bool {
    bytes.windows(wanted.len()).any(|window| window == wanted)
}

struct NoTransport;

impl Transport for NoTransport {
    fn get(&self, _: &str, _: Duration) -> Result<HttpAnswer, TransportError> {
        Err(TransportError::NoResponse)
    }
}

struct Local;

impl Dialer for Local {
    type Stream = TcpStream;

    fn dial(&self, host: &str, port: u16, _: Instant) -> std::io::Result<TcpStream> {
        let stream = TcpStream::connect((host, port))?;
        stream.set_read_timeout(Some(Duration::from_secs(10)))?;
        Ok(stream)
    }
}

#[test]
fn elzevirs_own_client_finds_the_records() {
    let catalog = Catalog::new();
    let (address, events) = catalog.listen(100);
    let server = |database: &str| {
        DraftServer::Z3950(Z3950Server {
            name: format!("Elzevir {database}"),
            host: address.ip().to_string(),
            port: address.port(),
            databases: vec![database.to_owned()],
            charset: RecordCharset::Auto,
            timeout: Duration::from_secs(10),
        })
    };
    let sources = DraftSources::new(
        vec![server("biblios"), server("authorities")],
        NoTransport,
        Local,
    );
    let query = DraftQuery::new(&TypedQuery {
        title: "anuario estatistico",
        author: "foz do iguacu",
        ..TypedQuery::default()
    })
    .unwrap();
    let results = sources.search(&query, 1);
    let SourceOutcome::Found {
        total,
        drafts,
        skipped,
    } = &results[0].outcome
    else {
        panic!("{:?}", results[0].outcome);
    };
    assert_eq!((*total, *skipped), (1, 0));
    assert_eq!(drafts[0].summary.title, "Perfil - Anuário Estatístico");
    let SourceOutcome::Refused(refusal) = &results[1].outcome else {
        panic!("{:?}", results[1].outcome);
    };
    assert!(refusal.starts_with("235"), "{refusal}");

    let by_isbn = DraftQuery::new(&TypedQuery {
        isbn: "978-85-60342-01-3",
        ..TypedQuery::default()
    })
    .unwrap();
    let SourceOutcome::Found { drafts, .. } = &sources.search(&by_isbn, 1)[0].outcome else {
        panic!("no drafts");
    };
    assert_eq!(drafts[0].summary.isbns.len(), 1);
    let logged = events.lock().unwrap();
    assert!(
        logged
            .iter()
            .any(|event| event.operation == "present" && event.records == 1)
    );
}

#[test]
fn records_come_in_iso2709_or_marcxml_with_the_items_a_page_at_a_time() {
    let catalog = Catalog::new();
    let (address, _) = catalog.listen(3);
    let mut client = Client::new(address);
    assert_eq!(total(&client.search("biblios", 1016, "medici")), 5);

    let (answer, bytes) = client.ask(&present_request(1, 5, &USMARC, "F"));
    let Pdu::PresentResponse {
        records,
        diagnostic: None,
    } = answer
    else {
        panic!("{answer:?}");
    };
    // max_records is 3.
    assert_eq!(records.len(), 3);
    let PresentRecord::Marc(first) = &records[0] else {
        panic!("{:?}", records[0]);
    };
    let record = elzevir_marc::RecordRef::parse(first, &elzevir_marc::Limits::default()).unwrap();
    assert!(
        record
            .fields()
            .any(|field| field.tag().to_string() == "952")
    );
    assert!(contains(&bytes, b"biblios"));

    let (answer, bytes) = client.ask(&present_request(2, 1, &MARCXML, "marcxml"));
    assert!(matches!(
        answer,
        Pdu::PresentResponse {
            diagnostic: None,
            ..
        }
    ));
    assert!(contains(&bytes, b"<record\n    xmlns:xsi="));
    assert!(!contains(&bytes, b"<?xml"));
    assert!(contains(&bytes, b"tag=\"952\""));
}

#[test]
fn bad_requests_get_their_diagnostics() {
    let catalog = Catalog::new();
    let (address, _) = catalog.listen(100);
    let mut client = Client::new(address);
    assert_eq!(condition(&client.search("others", 4, "rio")), 235);
    let (answer, _) = client.ask(&present_request(1, 1, &USMARC, "F"));
    assert_eq!(condition(&answer), 30);
    let found = total(&client.search("biblios", 4, "anuario estatistico"));
    let past = i64::try_from(found).unwrap() + 1;
    let (answer, _) = client.ask(&present_request(past, 1, &USMARC, "F"));
    assert_eq!(condition(&answer), 13);
    let (answer, _) = client.ask(&present_request(1, 1, &[1, 2, 840, 10003, 5, 16], "F"));
    assert_eq!(condition(&answer), 239);
    let (answer, _) = client.ask(&present_request(1, 1, &USMARC, "marcxml"));
    assert_eq!(condition(&answer), 239);
}

#[test]
fn a_hidden_instance_is_never_sent() {
    let catalog = Catalog::new();
    let (address, events) = catalog.listen(100);
    let mut client = Client::new(address);
    let perfil = catalog.first("anuario estatistico");
    let before = total(&client.search("biblios", 4, "anuario estatistico"));
    assert_eq!(total(&client.search("biblios", 1028, "1511")), 1);

    // Hidden but not yet reindexed: found, but sent as diagnostic 14 with nothing of it.
    catalog.hide(perfil);
    assert_eq!(total(&client.search("biblios", 1028, "1511")), 1);
    let (answer, bytes) = client.ask(&present_request(1, 1, &USMARC, "F"));
    assert_eq!(
        answer,
        Pdu::PresentResponse {
            records: vec![PresentRecord::Unusable],
            diagnostic: None
        }
    );
    assert!(!contains(&bytes, b"Anu"));
    assert!(!contains(&bytes, b"1511"));
    assert_eq!(
        events.lock().unwrap().last().unwrap().unavailable,
        [perfil.0]
    );

    catalog.reindex();
    assert_eq!(total(&client.search("biblios", 1028, "1511")), 0);
    assert_eq!(
        total(&client.search("biblios", 4, "anuario estatistico")),
        before - 1
    );
}

#[test]
fn a_request_before_the_init_or_garbage_closes_the_association() {
    let catalog = Catalog::new();
    let (address, _) = catalog.listen(100);
    for request in [
        search_request("biblios", 4, "rio"),
        vec![0x30, 0x80, 0x00, 0x00],
    ] {
        let mut stream = TcpStream::connect(address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        stream.write_all(&request).unwrap();
        let closed = z3950::parse(&answer(&mut stream)).unwrap();
        assert_eq!(closed, Pdu::Close { reason: 6 });
    }
}

/// The running binary, stopped when the test ends.
struct Process(Child);

impl Drop for Process {
    fn drop(&mut self) {
        drop(self.0.kill());
        drop(self.0.wait());
    }
}

#[test]
fn the_server_listens_from_the_configuration() {
    let (dir, store) = imported();
    drop(store);
    std::fs::write(
        dir.path().join("elzevir.toml"),
        "[z3950]\nlisten = \"127.0.0.1:0\"\n",
    )
    .unwrap();
    let log = dir.path().join("output.log");
    let output = std::fs::File::create(&log).unwrap();
    let mut process = Process(
        Command::new(env!("CARGO_BIN_EXE_elzevir"))
            .args(["serve", "--data-dir"])
            .arg(dir.path())
            .args(["--listen", "127.0.0.1:0"])
            .stdout(output.try_clone().unwrap())
            .stderr(output)
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(20);
    let address: SocketAddr = loop {
        let printed = std::fs::read_to_string(&log).unwrap();
        if let Some(address) = printed
            .lines()
            .find_map(|line| line.strip_prefix("Z39.50 on "))
        {
            break address.parse().unwrap();
        }
        assert!(process.0.try_wait().unwrap().is_none(), "{printed}");
        assert!(Instant::now() < deadline, "{printed}");
        std::thread::sleep(Duration::from_millis(20));
    };
    let mut client = Client::new(address);
    // The binary indexes in the background: the search finds the record once it is in.
    let deadline = Instant::now() + Duration::from_secs(20);
    while total(&client.search("biblios", 4, "anuario estatistico")) == 0 {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(50));
    }
    let (answer, _) = client.ask(&z3950::close_request());
    assert_eq!(answer, Pdu::Close { reason: 0 });
}
