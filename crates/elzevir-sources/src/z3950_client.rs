// SPDX-License-Identifier: GPL-3.0-or-later
//! The conversation with a Z39.50 server (docs/specs/catalogo/cliente-z3950.md, items 15
//! to 24): connect, Init, Search, Present and Close, over a connection the binary opens.

use std::io::{self, Read, Write};
use std::time::{Duration, Instant};

use elzevir_app::drafts::{DraftField, SourceOutcome};
use elzevir_marc::{Field, Limits, Record, RecordRef, marc8_to_utf8};

use crate::ber::{self, BerError};
use crate::z3950::{self, Pdu, PresentRecord};

/// The port of a server that names none.
pub const DEFAULT_Z3950_PORT: u16 = 210;

/// Bytes asked from the connection at a time.
const READ_CHUNK: usize = 64 * 1024;

/// The character set the records of a server are read in (item 21).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RecordCharset {
    /// UTF-8 when leader position 09 is `a`, MARC-8 otherwise.
    #[default]
    Auto,
    /// Always UTF-8.
    Utf8,
    /// Always MARC-8.
    Marc8,
}

/// One Z39.50 server of the institution, from `[[draft_sources]]` in `elzevir.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Z3950Server {
    /// Name shown in the origin, `z3950:<name>`.
    pub name: String,
    /// Host name or address, without scheme or port.
    pub host: String,
    /// TCP port.
    pub port: u16,
    /// The databases searched, at least one.
    pub databases: Vec<String>,
    /// How the records' characters are read.
    pub charset: RecordCharset,
    /// How long the whole conversation may take.
    pub timeout: Duration,
}

/// Opens byte streams to Z39.50 servers (item 23).
pub trait Dialer: Send + Sync {
    /// The connection.
    type Stream: Read + Write;

    /// Connects to `host` on `port`. Every read and write of the stream fails once
    /// `deadline` has passed.
    ///
    /// # Errors
    ///
    /// When the name does not resolve, the connection is refused or time runs out.
    fn dial(&self, host: &str, port: u16, deadline: Instant) -> io::Result<Self::Stream>;
}

/// Why a conversation stopped before its answer.
enum Stop {
    /// The connection failed, closed early or ran out of time.
    Silent,
    /// The server answered something this client cannot take.
    Refused(String),
}

impl From<io::Error> for Stop {
    fn from(_: io::Error) -> Self {
        Self::Silent
    }
}

impl From<BerError> for Stop {
    fn from(error: BerError) -> Self {
        Self::Refused(format!("unreadable Z39.50 message: {error}"))
    }
}

/// What `server` answers for page `page` of the filled `fields`, each record read by
/// `read` (items 18 to 22).
pub(crate) fn ask<R>(
    dialer: &impl Dialer,
    server: &Z3950Server,
    fields: &[(DraftField, &str)],
    page: u32,
    read: &dyn Fn(Record, &str) -> Option<R>,
) -> SourceOutcome<R> {
    let started = Instant::now();
    let deadline = started.checked_add(server.timeout).unwrap_or(started);
    let Ok(mut stream) = dialer.dial(&server.host, server.port, deadline) else {
        return SourceOutcome::NoResponse;
    };
    let answer = converse(&mut stream, server, fields, page, read);
    if !matches!(answer, Err(Stop::Silent)) {
        // The Close expects no answer (item 11); a server already gone changes nothing.
        stream.write_all(&z3950::close_request()).ok();
    }
    match answer {
        Ok(outcome) => outcome,
        Err(Stop::Silent) => SourceOutcome::NoResponse,
        Err(Stop::Refused(message)) => SourceOutcome::Refused(message),
    }
}

/// Init, Search and, when the page holds records, Present.
fn converse<R>(
    stream: &mut (impl Read + Write),
    server: &Z3950Server,
    fields: &[(DraftField, &str)],
    page: u32,
    read: &dyn Fn(Record, &str) -> Option<R>,
) -> Result<SourceOutcome<R>, Stop> {
    stream.write_all(&z3950::init_request(env!("CARGO_PKG_VERSION")))?;
    match read_pdu(stream)? {
        Pdu::InitResponse { accepted: true } => {}
        Pdu::InitResponse { accepted: false } => {
            return Err(Stop::Refused("init refused".to_owned()));
        }
        other => return Err(unexpected(&other)),
    }
    stream.write_all(&z3950::search_request_of(&server.databases, fields))?;
    let total = match read_pdu(stream)? {
        Pdu::SearchResponse {
            diagnostic: Some(diagnostic),
            ..
        } => return Err(Stop::Refused(diagnostic.message())),
        Pdu::SearchResponse {
            succeeded: false, ..
        } => return Err(Stop::Refused("search failed".to_owned())),
        Pdu::SearchResponse { total, .. } => total,
        other => return Err(unexpected(&other)),
    };
    // A page past the total asks for nothing more (item 20).
    let Some(present) = z3950::present_request(page, total) else {
        return Ok(crate::found(&origin(server), total, Vec::new(), 0, read));
    };
    stream.write_all(&present)?;
    let records = match read_pdu(stream)? {
        Pdu::PresentResponse {
            records,
            diagnostic: Some(diagnostic),
        } if records.is_empty() => return Err(Stop::Refused(diagnostic.message())),
        Pdu::PresentResponse { records, .. } => records,
        other => return Err(unexpected(&other)),
    };
    let mut skipped = 0_u32;
    let readable = records
        .into_iter()
        .filter_map(|record| {
            let read = match record {
                PresentRecord::Marc(bytes) => marc_record(&bytes, server.charset),
                PresentRecord::Unusable => None,
            };
            if read.is_none() {
                skipped = skipped.saturating_add(1);
            }
            read
        })
        .collect();
    Ok(crate::found(
        &origin(server),
        total,
        readable,
        skipped,
        read,
    ))
}

fn origin(server: &Z3950Server) -> String {
    format!("z3950:{}", server.name)
}

/// The refusal for a message that does not answer the step (item 12).
fn unexpected(pdu: &Pdu) -> Stop {
    Stop::Refused(match pdu {
        Pdu::Close { reason } => format!("closed by the server (reason {reason})"),
        Pdu::Other(tag) => format!("unexpected Z39.50 message {tag}"),
        Pdu::InitResponse { .. } | Pdu::SearchResponse { .. } | Pdu::PresentResponse { .. } => {
            "unexpected Z39.50 message".to_owned()
        }
    })
}

/// Reads one whole message: the BER header says how many bytes it has.
fn read_pdu(stream: &mut impl Read) -> Result<Pdu, Stop> {
    let mut message = Vec::new();
    let mut chunk = vec![0_u8; READ_CHUNK];
    loop {
        let wanted = match ber::frame_length(&message)? {
            Some(length) if message.len() >= length => return Ok(z3950::parse(&message)?),
            // Only up to the end of the message: nothing of a next one is taken.
            Some(length) => (length - message.len()).min(READ_CHUNK),
            // The header is a few bytes; read them one at a time.
            None => 1,
        };
        let buffer = chunk.get_mut(..wanted).ok_or(Stop::Silent)?;
        let read = stream.read(buffer)?;
        if read == 0 {
            return Err(Stop::Silent);
        }
        message.extend_from_slice(buffer.get(..read).ok_or(Stop::Silent)?);
    }
}

/// The ISO 2709 `bytes` as a UTF-8 record, or `None` when they are not valid MARC or do
/// not convert (items 21 and 22).
fn marc_record(bytes: &[u8], charset: RecordCharset) -> Option<Record> {
    let read = RecordRef::parse(bytes, &Limits::default()).ok()?;
    let mut record = Record::from_ref(&read);
    to_utf8(&mut record, charset).then_some(record)
}

/// Brings `record` to UTF-8 from `charset` and sets leader position 09 to `a`; `false`,
/// with the record untouched, when a value does not convert.
///
/// Koha: `C4::Charset::MarcToUTF8Record` with the server's encoding, for MARC21; `Auto`
/// trusts leader position 09, which Koha reads only from the server's setting.
pub fn to_utf8(record: &mut Record, charset: RecordCharset) -> bool {
    let marc8 = match charset {
        RecordCharset::Auto => record.leader.get(9) != Some(&b'a'),
        RecordCharset::Utf8 => false,
        RecordCharset::Marc8 => true,
    };
    let mut converted = record.clone();
    let mut fine = true;
    for field in &mut converted.fields {
        match field {
            Field::Control { value, .. } => fine &= std::str::from_utf8(value).is_ok(),
            Field::Data { subfields, .. } => {
                for subfield in subfields {
                    if marc8 {
                        let decoded = marc8_to_utf8(&subfield.value);
                        fine &= decoded.unmapped.is_empty();
                        subfield.value = decoded.text.into_owned().into_bytes();
                    } else {
                        fine &= std::str::from_utf8(&subfield.value).is_ok();
                    }
                }
            }
        }
        if !fine {
            return false;
        }
    }
    if let Some(position) = converted.leader.get_mut(9) {
        *position = b'a';
    }
    *record = converted;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(leader09: u8, value: &[u8]) -> Record {
        let mut leader = *b"00000nam a2200000 a 4500";
        if let Some(position) = leader.get_mut(9) {
            *position = leader09;
        }
        Record {
            leader,
            fields: vec![Field::Data {
                tag: elzevir_marc::Tag::new(*b"245"),
                indicators: *b"00",
                subfields: vec![elzevir_marc::Subfield {
                    code: b'a',
                    value: value.to_vec(),
                }],
            }],
        }
    }

    fn title(record: &Record) -> Vec<u8> {
        match record.fields.first().unwrap() {
            Field::Data { subfields, .. } => subfields.first().unwrap().value.clone(),
            Field::Control { value, .. } => value.clone(),
        }
    }

    #[test]
    fn the_leader_chooses_the_charset_unless_the_server_forces_one() {
        // MARC-8: acute accent (0xE2) before the letter.
        let mut marc8 = record(b' ', b"Jos\xE2e");
        assert!(to_utf8(&mut marc8, RecordCharset::Auto));
        // Combining marks stay decomposed, as MARC::Charset leaves them.
        assert_eq!(title(&marc8), "Jose\u{301}".as_bytes());
        assert_eq!(marc8.leader.get(9), Some(&b'a'));

        let mut utf8 = record(b'a', "José".as_bytes());
        assert!(to_utf8(&mut utf8, RecordCharset::Auto));
        assert_eq!(title(&utf8), "José".as_bytes());

        // The server says UTF-8 though the leader does not.
        let mut forced = record(b' ', "José".as_bytes());
        assert!(to_utf8(&mut forced, RecordCharset::Utf8));
        assert_eq!(title(&forced), "José".as_bytes());

        // Not UTF-8 after all: left untouched and refused.
        let mut broken = record(b'a', b"Jos\xE9");
        assert!(!to_utf8(&mut broken, RecordCharset::Auto));
        assert_eq!(broken.leader.get(9), Some(&b'a'));
        assert_eq!(title(&broken), b"Jos\xE9");

        let mut forced_marc8 = record(b'a', b"Jos\xE2e");
        assert!(to_utf8(&mut forced_marc8, RecordCharset::Marc8));
        assert_eq!(title(&forced_marc8), "Jose\u{301}".as_bytes());
    }
}
