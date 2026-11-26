// SPDX-License-Identifier: GPL-3.0-or-later
//! The Z39.50 server of the public catalog (docs/specs/opac/z3950.md): one session per
//! connection, Init, Search, Present and Close, over the search by field and the stored
//! records.
//!
//! Koha: `Koha::Z3950Responder`, `Koha::Z3950Responder::Session` and
//! `Koha::Z3950Responder::GenericSession`.

mod request;
mod response;

use std::io::{self, ErrorKind, Read, Write};
use std::sync::Arc;

use elzevir_app::field_search::{FieldQuery, FieldSearch};
use elzevir_app::oai::{OaiCatalog, marcxml_element, visible_marc};
use elzevir_app::search::SearchScope;
use elzevir_domain::catalog::InstanceId;

use crate::ber;
use crate::z3950::{Diagnostic, USMARC};

use request::{Request, dotted, parse};
use response::{InitAnswer, PresentStatus, RecordOut};

/// The MARCXML record syntax.
pub const MARCXML: [u32; 7] = [1, 2, 840, 10003, 5, 109, 10];
/// Longest request read (item 3).
pub const MAX_REQUEST_BYTES: usize = 64 * 1024;
/// The largest message and record sizes the Init offers (item 5).
const MAX_MESSAGE: i64 = 8 * 1024 * 1024;
/// Result sets a session keeps (item 11).
const MAX_SETS: usize = 16;

// Close reasons (Z39.50 `CloseReason`).
const FINISHED: i64 = 0;
const SHUTDOWN: i64 = 1;
const PROTOCOL_ERROR: i64 = 6;
const LACK_OF_ACTIVITY: i64 = 7;

/// The server's catalog: the records, the index and its settings.
#[derive(Clone)]
pub struct Z3950Catalog {
    /// The stored records.
    pub catalog: Arc<dyn OaiCatalog>,
    /// The search by field.
    pub index: Arc<dyn FieldSearch>,
    /// The database name clients ask for.
    pub database: String,
    /// Most records per Present.
    pub max_records: usize,
    /// The program's version, for the Init.
    pub version: String,
}

/// What the log says of one Search or Present (item 19).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Z3950Event {
    /// `search` or `present`.
    pub operation: &'static str,
    /// The Bib-1 condition that refused it, if any.
    pub diagnostic: Option<i64>,
    /// The matches in all.
    pub total: usize,
    /// Records sent.
    pub records: usize,
    /// Instances sent as a surrogate diagnostic.
    pub unavailable: Vec<i64>,
    /// Why storage or the index failed, for the log only.
    pub failure: Option<String>,
}

struct ResultSet {
    name: String,
    query: FieldQuery,
    total: usize,
}

/// What to do after a request.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    /// Send and wait for the next request.
    Reply(Vec<u8>),
    /// Send and close the connection.
    Last(Vec<u8>),
}

/// One connection's state: whether the Init came, and its result sets.
struct Session<'a> {
    server: &'a Z3950Catalog,
    initialized: bool,
    sets: Vec<ResultSet>,
}

/// Serves one connection until the client closes it, sends something it must not, or
/// `stream` gives up: [`ErrorKind::TimedOut`] closes for inactivity and
/// [`ErrorKind::ConnectionAborted`] for the server's shutdown (item 18).
///
/// # Errors
///
/// When writing to the client fails.
pub fn serve(
    stream: &mut (impl Read + Write),
    server: &Z3950Catalog,
    log: &dyn Fn(Z3950Event),
) -> io::Result<()> {
    let mut session = Session {
        server,
        initialized: false,
        sets: Vec::new(),
    };
    loop {
        let message = match read_request(stream) {
            Ok(Some(message)) => message,
            Ok(None) => return Ok(()),
            Err(Stop::Io(error)) => {
                let reason = match error.kind() {
                    ErrorKind::TimedOut | ErrorKind::WouldBlock => LACK_OF_ACTIVITY,
                    ErrorKind::ConnectionAborted => SHUTDOWN,
                    _ => return Ok(()),
                };
                return stream.write_all(&response::close(None, reason));
            }
            Err(Stop::Invalid) => {
                return stream.write_all(&response::close(None, PROTOCOL_ERROR));
            }
        };
        let (step, event) = session.answer(&message);
        if let Some(event) = event {
            log(event);
        }
        match step {
            Step::Reply(bytes) => {
                stream.write_all(&bytes)?;
                stream.flush()?;
            }
            Step::Last(bytes) => {
                stream.write_all(&bytes)?;
                return stream.flush();
            }
        }
    }
}

/// Whether `bytes` are one request the server reads (items 3 to 17); for the fuzzing
/// target, since [`serve`] reads requests itself.
#[must_use]
pub fn reads_request(bytes: &[u8]) -> bool {
    parse(bytes).is_ok()
}

enum Stop {
    Io(io::Error),
    Invalid,
}

/// One whole request; `None` when the client closed between requests.
fn read_request(stream: &mut impl Read) -> Result<Option<Vec<u8>>, Stop> {
    let mut message = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        match ber::frame_length(&message) {
            Ok(Some(wanted)) if wanted > MAX_REQUEST_BYTES => return Err(Stop::Invalid),
            Ok(Some(wanted)) if message.len() >= wanted => {
                // One request at a time: bytes past it would be a pipelined request,
                // which a Z39.50 client does not send before the answer.
                message.truncate(wanted);
                return Ok(Some(message));
            }
            Ok(_) if message.len() > MAX_REQUEST_BYTES => return Err(Stop::Invalid),
            Ok(_) => {}
            Err(_) => return Err(Stop::Invalid),
        }
        let read = stream.read(&mut chunk).map_err(Stop::Io)?;
        if read == 0 {
            return if message.is_empty() {
                Ok(None)
            } else {
                Err(Stop::Invalid)
            };
        }
        message.extend_from_slice(chunk.get(..read).unwrap_or_default());
    }
}

impl Session<'_> {
    fn answer(&mut self, message: &[u8]) -> (Step, Option<Z3950Event>) {
        let Ok(request) = parse(message) else {
            return (Step::Last(response::close(None, PROTOCOL_ERROR)), None);
        };
        match request {
            Request::Init {
                reference,
                versions,
                options,
                message_size,
                record_size,
            } => {
                self.initialized = true;
                (
                    Step::Reply(init(
                        reference.as_deref(),
                        &versions,
                        &options,
                        (message_size, record_size),
                        &self.server.version,
                    )),
                    None,
                )
            }
            _ if !self.initialized => (Step::Last(response::close(None, PROTOCOL_ERROR)), None),
            Request::Search {
                reference,
                set,
                databases,
                query,
            } => {
                let (outcome, event) = self.search(set, &databases, query);
                (
                    Step::Reply(response::search_response(
                        reference.as_deref(),
                        outcome.as_ref().map(|total| *total),
                    )),
                    Some(event),
                )
            }
            Request::Present {
                reference,
                set,
                start,
                count,
                elements,
                syntax,
            } => {
                let asked = Asked {
                    set: &set,
                    start,
                    count,
                    elements: elements.as_deref(),
                    syntax: syntax.as_deref(),
                };
                let (bytes, event) = self.present(reference.as_deref(), &asked);
                (Step::Reply(bytes), Some(event))
            }
            Request::Close { reference } => (
                Step::Last(response::close(reference.as_deref(), FINISHED)),
                None,
            ),
            Request::Other(_) => (Step::Last(response::close(None, PROTOCOL_ERROR)), None),
        }
    }

    /// Items 6 to 13.
    fn search(
        &mut self,
        name: String,
        databases: &[String],
        query: Result<FieldQuery, Diagnostic>,
    ) -> (Result<usize, Diagnostic>, Z3950Event) {
        let mut event = Z3950Event {
            operation: "search",
            diagnostic: None,
            total: 0,
            records: 0,
            unavailable: Vec::new(),
            failure: None,
        };
        let outcome = self.check_database(databases).and(query).and_then(|query| {
            let found = self
                .server
                .index
                .search_fields(&query, SearchScope::Public, 0, 0)
                .map_err(|error| {
                    event.failure = Some(error.to_string());
                    Diagnostic::bare(2)
                })?;
            self.sets.retain(|kept| kept.name != name);
            if self.sets.len() >= MAX_SETS {
                self.sets.remove(0);
            }
            self.sets.push(ResultSet {
                name,
                query,
                total: found.total,
            });
            Ok(found.total)
        });
        match &outcome {
            Ok(total) => event.total = *total,
            Err(diagnostic) => event.diagnostic = Some(diagnostic.condition),
        }
        (outcome, event)
    }

    fn check_database(&self, databases: &[String]) -> Result<(), Diagnostic> {
        match databases {
            [one] if one.eq_ignore_ascii_case(&self.server.database) => Ok(()),
            [one] => Err(Diagnostic::with(235, one)),
            [] => Err(Diagnostic::with(235, "")),
            _ => Err(Diagnostic::with(111, "1")),
        }
    }

    /// Items 14 to 17.
    fn present(&self, reference: Option<&[u8]>, asked: &Asked<'_>) -> (Vec<u8>, Z3950Event) {
        let mut event = Z3950Event {
            operation: "present",
            diagnostic: None,
            total: 0,
            records: 0,
            unavailable: Vec::new(),
            failure: None,
        };
        let database = self.server.database.as_str();
        let outcome = self.records(asked, &mut event);
        let bytes = match &outcome {
            Ok((records, next, status)) => {
                event.records = records.len();
                response::present_response(reference, database, Ok((records, *next, *status)))
            }
            Err(diagnostic) => {
                event.diagnostic = Some(diagnostic.condition);
                response::present_response(reference, database, Err(diagnostic))
            }
        };
        (bytes, event)
    }

    fn records(
        &self,
        asked: &Asked<'_>,
        event: &mut Z3950Event,
    ) -> Result<(Vec<RecordOut>, usize, PresentStatus), Diagnostic> {
        let set = self
            .sets
            .iter()
            .find(|set| set.name == asked.set)
            .ok_or_else(|| Diagnostic::with(30, asked.set))?;
        event.total = set.total;
        let total = i64::try_from(set.total).unwrap_or(i64::MAX);
        if asked.start < 1 || asked.count < 0 || asked.start - 1 + asked.count > total {
            return Err(Diagnostic::bare(13));
        }
        let xml = syntax_of(asked)?;
        let start = usize::try_from(asked.start).unwrap_or(1);
        let wanted = usize::try_from(asked.count).unwrap_or(0);
        let count = wanted.min(self.server.max_records);
        if count == 0 {
            return Ok((Vec::new(), start, PresentStatus::Success));
        }
        let found = self
            .server
            .index
            .search_fields(&set.query, SearchScope::Public, start - 1, count)
            .map_err(|error| {
                event.failure = Some(error.to_string());
                Diagnostic::bare(2)
            })?;
        let mut records = Vec::with_capacity(found.hits.len());
        for hit in &found.hits {
            let record = self.record(hit.instance, xml).map_err(|failure| {
                event.failure = Some(failure);
                Diagnostic::bare(2)
            })?;
            records.push(record.unwrap_or_else(|| {
                event.unavailable.push(hit.instance.0);
                RecordOut::Surrogate(Diagnostic::bare(14))
            }));
        }
        let after = start + records.len();
        let next = if after > set.total { 0 } else { after };
        let status = if records.len() < wanted {
            PresentStatus::Partial
        } else {
            PresentStatus::Success
        };
        Ok((records, next, status))
    }

    /// The record in ISO 2709 or MARCXML when the OPAC shows it now; `None` otherwise.
    fn record(&self, instance: InstanceId, xml: bool) -> Result<Option<RecordOut>, String> {
        let Some(marc) = visible_marc(self.server.catalog.as_ref(), instance)
            .map_err(|error| error.to_string())?
        else {
            return Ok(None);
        };
        let written = if xml {
            marcxml_element(&marc).map(|element| (MARCXML.to_vec(), element.into_bytes()))
        } else {
            let mut bytes = Vec::new();
            marc.write_iso2709(&mut bytes)
                .ok()
                .map(|()| (USMARC.to_vec(), bytes))
        };
        Ok(written.map(|(syntax, bytes)| RecordOut::Record { syntax, bytes }))
    }
}

/// A Present's request.
struct Asked<'a> {
    set: &'a str,
    start: i64,
    count: i64,
    elements: Option<&'a str>,
    syntax: Option<&'a [u32]>,
}

/// Whether the records go as MARCXML, or the diagnostic 239 (item 15).
fn syntax_of(asked: &Asked<'_>) -> Result<bool, Diagnostic> {
    let xml = match asked.syntax {
        None => false,
        Some(syntax) if syntax == USMARC => false,
        Some(syntax) if syntax == MARCXML => true,
        Some(other) => return Err(Diagnostic::with(239, &dotted(other))),
    };
    let elements_fit = matches!(
        (xml, asked.elements),
        (_, None | Some("F")) | (false, Some("B")) | (true, Some("marcxml"))
    );
    if elements_fit {
        Ok(xml)
    } else {
        Err(Diagnostic::with(239, asked.elements.unwrap_or_default()))
    }
}

/// The Init response (item 5): the versions and options both sides name, the smaller
/// sizes, and acceptance when version 2 or 3 is among them.
fn init(
    reference: Option<&[u8]>,
    versions: &[u8],
    options: &[u8],
    (message_size, record_size): (Option<i64>, Option<i64>),
    version: &str,
) -> Vec<u8> {
    // Versions 1 to 3 are bits 0 to 2 of the first byte after the unused-bits count.
    let asked_versions = versions.get(1).copied().unwrap_or(0) & 0xE0;
    // search (0) and present (1), and namedResultSets (14).
    let options_first = options.get(1).copied().unwrap_or(0) & 0xC0;
    let options_second = options.get(2).copied().unwrap_or(0) & 0x02;
    let size = |asked: Option<i64>| {
        asked
            .filter(|size| *size > 0)
            .map_or(MAX_MESSAGE, |size| size.min(MAX_MESSAGE))
    };
    response::init_response(&InitAnswer {
        reference,
        versions: vec![0x05, asked_versions],
        options: vec![0x01, options_first, options_second],
        message_size: size(message_size),
        record_size: size(record_size),
        accepted: asked_versions & 0x60 != 0,
        version,
    })
}

#[cfg(test)]
mod tests;
