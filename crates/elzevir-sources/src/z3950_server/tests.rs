// SPDX-License-Identifier: GPL-3.0-or-later
//! The session over fake ports: the index finds Instances 1 to `total`, and the
//! catalog has no record, so every record goes out as the surrogate diagnostic 14.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::num::NonZeroUsize;

use elzevir_app::catalog::CatalogReadError;
use elzevir_app::drafts::DraftField;
use elzevir_app::export::{RecordSource, StoredRecord};
use elzevir_app::field_search::{FieldTerm, SearchField, Truncation, WordMatch};
use elzevir_app::material::Material;
use elzevir_app::oai::{Datestamp, OaiHeader, OaiUnavailable};
use elzevir_app::rdf::HeldItem;
use elzevir_app::search::{SearchError, SearchHit, SearchResults};
use elzevir_domain::catalog::WorkId;

use super::*;
use crate::ber::{context, context_integer, context_of};
use crate::z3950::{self, Pdu, PresentRecord, close_request, init_request, present_request};

struct Found(usize);

impl FieldSearch for Found {
    fn search_fields(
        &self,
        _query: &FieldQuery,
        _scope: SearchScope,
        offset: usize,
        limit: usize,
    ) -> Result<SearchResults, SearchError> {
        let hits = (offset..self.0.min(offset + limit))
            .map(|index| {
                let id = i64::try_from(index + 1).unwrap();
                SearchHit {
                    instance: InstanceId(id),
                    work: WorkId(id),
                    hub: None,
                    score: 1.0,
                    title: "Rio".to_owned(),
                    main_contributor: None,
                    date: None,
                    material: Material {
                        content: None,
                        carrier: None,
                    },
                    call_number: None,
                }
            })
            .collect();
        Ok(SearchResults {
            total: self.0,
            hits,
        })
    }
}

struct Empty;

impl RecordSource for Empty {
    fn records_after(
        &self,
        _after: Option<InstanceId>,
        _limit: NonZeroUsize,
    ) -> Result<Vec<StoredRecord>, CatalogReadError> {
        Ok(Vec::new())
    }

    fn held_items(&self, _instance: InstanceId) -> Result<Vec<HeldItem>, CatalogReadError> {
        Ok(Vec::new())
    }
}

impl OaiCatalog for Empty {
    fn earliest(&self) -> Result<Option<Datestamp>, OaiUnavailable> {
        Ok(None)
    }

    fn header(&self, _instance: InstanceId) -> Result<Option<OaiHeader>, OaiUnavailable> {
        Ok(None)
    }

    fn headers(
        &self,
        _from_id: i64,
        _from: Option<&Datestamp>,
        _until: Option<&Datestamp>,
        _limit: usize,
    ) -> Result<Vec<OaiHeader>, OaiUnavailable> {
        Ok(Vec::new())
    }
}

fn server(total: usize) -> Z3950Catalog {
    Z3950Catalog {
        catalog: Arc::new(Empty),
        index: Arc::new(Found(total)),
        database: "biblios".to_owned(),
        max_records: 5,
        version: "0.1.0".to_owned(),
    }
}

/// The client's requests, one per read, then the end of the stream or `error`.
struct Wire {
    requests: VecDeque<Vec<u8>>,
    error: Option<ErrorKind>,
    sent: Vec<u8>,
}

impl Read for Wire {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match self.requests.pop_front() {
            Some(request) => {
                let length = request.len().min(buffer.len());
                buffer[..length].copy_from_slice(&request[..length]);
                if length < request.len() {
                    self.requests.push_front(request[length..].to_vec());
                }
                Ok(length)
            }
            None => self.error.map_or(Ok(0), |kind| Err(kind.into())),
        }
    }
}

impl Write for Wire {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.sent.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The answers to `requests`, and the events logged.
fn exchange(
    total: usize,
    requests: Vec<Vec<u8>>,
    error: Option<ErrorKind>,
) -> (Vec<Pdu>, Vec<Z3950Event>) {
    let mut wire = Wire {
        requests: requests.into(),
        error,
        sent: Vec::new(),
    };
    let events = RefCell::new(Vec::new());
    serve(&mut wire, &server(total), &|event| {
        events.borrow_mut().push(event);
    })
    .unwrap();
    let mut answers = Vec::new();
    let mut rest = wire.sent.as_slice();
    while !rest.is_empty() {
        let length = ber::frame_length(rest).unwrap().unwrap();
        answers.push(z3950::parse(&rest[..length]).unwrap());
        rest = &rest[length..];
    }
    (answers, events.into_inner())
}

fn search(database: &str) -> Vec<u8> {
    z3950::search_request_of(&[database.to_owned()], &[(DraftField::Title, "rio")])
}

fn present(start: i64, count: i64, syntax: &[u32]) -> Vec<u8> {
    context_of(
        24,
        &[
            context(31, b"default"),
            context_integer(30, start),
            context_integer(29, count),
            context(104, &ber::oid_content(syntax)),
        ],
    )
}

fn diagnostic(pdu: &Pdu) -> Option<i64> {
    match pdu {
        Pdu::SearchResponse { diagnostic, .. } | Pdu::PresentResponse { diagnostic, .. } => {
            diagnostic.as_ref().map(|diagnostic| diagnostic.condition)
        }
        _ => None,
    }
}

#[test]
fn a_session_searches_presents_and_closes() {
    let (answers, events) = exchange(
        12,
        vec![
            init_request("1.0"),
            search("Biblios"),
            present_request(1, 12).unwrap(),
            close_request(),
        ],
        None,
    );
    assert_eq!(answers[0], Pdu::InitResponse { accepted: true });
    assert_eq!(
        answers[1],
        Pdu::SearchResponse {
            total: 12,
            succeeded: true,
            diagnostic: None
        }
    );
    // Asked for 12, sent max_records (5), each a surrogate since the catalog has none.
    let Pdu::PresentResponse {
        records,
        diagnostic,
    } = &answers[2]
    else {
        panic!("not a present response: {:?}", answers[2]);
    };
    assert_eq!(records, &vec![PresentRecord::Unusable; 5]);
    assert_eq!(diagnostic, &None);
    assert_eq!(answers[3], Pdu::Close { reason: FINISHED });
    assert_eq!(events.len(), 2);
    assert_eq!((events[0].operation, events[0].total), ("search", 12));
    assert_eq!(
        (
            events[1].operation,
            events[1].records,
            &events[1].unavailable
        ),
        ("present", 5, &vec![1, 2, 3, 4, 5])
    );
}

#[test]
fn each_refusal_has_its_diagnostic() {
    let (answers, _) = exchange(
        3,
        vec![
            init_request("1.0"),
            search("other"),
            present(1, 1, &USMARC),
            search("biblios"),
            present(3, 2, &USMARC),
            present(0, 1, &USMARC),
            present(1, 1, &[1, 2, 840, 10003, 5, 16]),
            present(1, 1, &MARCXML),
        ],
        None,
    );
    let conditions: Vec<Option<i64>> = answers.iter().skip(1).map(diagnostic).collect();
    assert_eq!(
        conditions,
        vec![
            Some(235),
            Some(30),
            None,
            Some(13),
            Some(13),
            Some(239),
            None
        ]
    );
}

#[test]
fn many_databases_are_refused() {
    let request = z3950::search_request_of(
        &["biblios".to_owned(), "authorities".to_owned()],
        &[(DraftField::Title, "rio")],
    );
    let (answers, _) = exchange(1, vec![init_request("1.0"), request], None);
    assert_eq!(diagnostic(&answers[1]), Some(111));
}

#[test]
fn a_request_before_the_init_or_garbage_closes_with_protocol_error() {
    let (answers, _) = exchange(1, vec![search("biblios")], None);
    assert_eq!(
        answers,
        vec![Pdu::Close {
            reason: PROTOCOL_ERROR
        }]
    );
    let (answers, _) = exchange(1, vec![init_request("1.0"), vec![0x30, 0x03, 1]], None);
    assert_eq!(
        answers[1],
        Pdu::Close {
            reason: PROTOCOL_ERROR
        }
    );
    let mut oversized = vec![0xB6, 0x83];
    oversized.extend_from_slice(&[0x01, 0x00, 0x01]);
    let (answers, _) = exchange(1, vec![oversized], None);
    assert_eq!(
        answers,
        vec![Pdu::Close {
            reason: PROTOCOL_ERROR
        }]
    );
}

#[test]
fn idleness_and_shutdown_close_with_their_reasons() {
    let (answers, _) = exchange(1, vec![init_request("1.0")], Some(ErrorKind::TimedOut));
    assert_eq!(
        answers[1],
        Pdu::Close {
            reason: LACK_OF_ACTIVITY
        }
    );
    let (answers, _) = exchange(1, Vec::new(), Some(ErrorKind::ConnectionAborted));
    assert_eq!(answers, vec![Pdu::Close { reason: SHUTDOWN }]);
    let (answers, _) = exchange(1, Vec::new(), None);
    assert!(answers.is_empty());
}

#[test]
fn the_init_keeps_what_both_sides_name() {
    let refused = context_of(
        20,
        &[
            // Version 1 only.
            context(3, &[0x05, 0x80]),
            context(4, &[0x06, 0xC0]),
        ],
    );
    let (answers, _) = exchange(1, vec![refused], None);
    assert_eq!(answers[0], Pdu::InitResponse { accepted: false });
}

#[test]
fn a_search_replaces_its_set_by_name_and_keeps_sixteen() {
    let mut session = Session {
        server: &server(2),
        initialized: true,
        sets: Vec::new(),
    };
    let query = || {
        Ok(FieldQuery::Term(FieldTerm {
            field: SearchField::Any,
            matching: WordMatch::All,
            text: "rio".to_owned(),
            truncation: Truncation::None,
        }))
    };
    for number in 0..20 {
        let (outcome, _) = session.search(format!("s{number}"), &["biblios".to_owned()], query());
        assert_eq!(outcome, Ok(2));
    }
    let (outcome, _) = session.search("s19".to_owned(), &["biblios".to_owned()], query());
    assert_eq!(outcome, Ok(2));
    assert_eq!(session.sets.len(), MAX_SETS);
    assert_eq!(
        session.sets.first().map(|set| set.name.as_str()),
        Some("s4")
    );
    assert_eq!(
        session.sets.last().map(|set| set.name.as_str()),
        Some("s19")
    );
}
