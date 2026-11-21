// SPDX-License-Identifier: GPL-3.0-or-later
//! Reading a searchRetrieve response: the total, the MARC21 records and the diagnostics
//! (spec items 13 and 14). Names are matched by local name, so SRU 1.2 and 2.0 read alike;
//! only a MARC record must carry the MARC21 namespace.

use elzevir_app::drafts::DRAFTS_PER_PAGE;
use elzevir_marc::{MarcXmlReader, Record, XmlLimits};
use quick_xml::events::Event;
use quick_xml::name::ResolveResult;
use quick_xml::reader::NsReader;

/// Largest response body read.
pub const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
/// Most characters of a diagnostic kept.
const MAX_DIAGNOSTIC_CHARS: usize = 300;
const MARC21: &[u8] = b"http://www.loc.gov/MARC21/slim";

/// What a response says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedResponse {
    /// The server's total, the records read and how many were left out.
    Records {
        /// `numberOfRecords`.
        total: u64,
        /// Readable MARC21 records, at most one page.
        records: Vec<Record>,
        /// Records that were not valid MARC21, or beyond one page.
        skipped: u32,
    },
    /// The server's diagnostic message.
    Diagnostic(String),
}

/// Why a response cannot be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ResponseError {
    /// Longer than [`MAX_RESPONSE_BYTES`].
    #[error("response too large")]
    TooLarge,
    /// Not well-formed XML, or with a DTD.
    #[error("unreadable SRU response")]
    Unreadable,
    /// No `numberOfRecords` and no diagnostic.
    #[error("not an SRU searchRetrieve response")]
    NotSru,
}

#[derive(Default)]
struct Reading {
    total: Option<u64>,
    records: Vec<Record>,
    skipped: u32,
    diagnostic: Vec<String>,
    text: String,
}

/// Reads a searchRetrieve response.
///
/// # Errors
///
/// When the body is too large, not XML, or not an SRU response.
pub fn parse(body: &[u8]) -> Result<ParsedResponse, ResponseError> {
    if body.len() > MAX_RESPONSE_BYTES {
        return Err(ResponseError::TooLarge);
    }
    let mut reader = NsReader::from_reader(body);
    let mut path: Vec<Vec<u8>> = Vec::new();
    let mut reading = Reading::default();
    loop {
        let start = position(&reader);
        let (namespace, event) = reader
            .read_resolved_event()
            .map_err(|_| ResponseError::Unreadable)?;
        match event {
            Event::Start(element) => {
                let local = element.local_name().as_ref().to_vec();
                let marc = matches!(namespace, ResolveResult::Bound(ns) if ns.as_ref() == MARC21);
                if marc && local == b"record" && path.iter().any(|name| name == b"recordData") {
                    reader
                        .read_to_end(element.name())
                        .map_err(|_| ResponseError::Unreadable)?;
                    let prefix = element
                        .name()
                        .prefix()
                        .map(|prefix| prefix.as_ref().to_vec());
                    let end = position(&reader);
                    let slice = body.get(start..end).ok_or(ResponseError::Unreadable)?;
                    reading.record(slice, prefix.as_deref());
                    continue;
                }
                reading.text.clear();
                path.push(local);
            }
            Event::End(_) => {
                if let Some(name) = path.pop() {
                    reading.close(&name, &path);
                }
            }
            Event::Text(text) => {
                let text = text.decode().map_err(|_| ResponseError::Unreadable)?;
                reading.text.push_str(&text);
            }
            Event::GeneralRef(reference) => {
                if let Ok(Some(character)) = reference.resolve_char_ref() {
                    reading.text.push(character);
                } else if let Some(value) = reference
                    .decode()
                    .ok()
                    .and_then(|name| quick_xml::escape::resolve_xml_entity(&name))
                {
                    reading.text.push_str(value);
                }
            }
            Event::DocType(_) => return Err(ResponseError::Unreadable),
            Event::Eof => break,
            _ => {}
        }
    }
    if !reading.diagnostic.is_empty() && reading.records.is_empty() {
        let message: String = reading
            .diagnostic
            .join(": ")
            .chars()
            .take(MAX_DIAGNOSTIC_CHARS)
            .collect();
        return Ok(ParsedResponse::Diagnostic(message));
    }
    let total = reading.total.ok_or(ResponseError::NotSru)?;
    Ok(ParsedResponse::Records {
        total,
        records: reading.records,
        skipped: reading.skipped,
    })
}

fn position(reader: &NsReader<&[u8]>) -> usize {
    usize::try_from(reader.buffer_position()).unwrap_or(usize::MAX)
}

impl Reading {
    fn close(&mut self, name: &[u8], parents: &[Vec<u8>]) {
        let text = self.text.trim();
        match name {
            b"numberOfRecords" => self.total = text.parse().ok(),
            b"message" | b"details"
                if parents.iter().any(|parent| parent == b"diagnostic") && !text.is_empty() =>
            {
                self.diagnostic.push(text.to_owned());
            }
            _ => {}
        }
        self.text.clear();
    }

    /// One MARC record, cut out of the envelope and read by the MARCXML reader with its
    /// limits. The namespace may have been declared on an ancestor, so it is declared again
    /// around the record.
    fn record(&mut self, slice: &[u8], prefix: Option<&[u8]>) {
        if self.records.len() >= usize::try_from(DRAFTS_PER_PAGE).unwrap_or(usize::MAX) {
            self.skipped = self.skipped.saturating_add(1);
            return;
        }
        let mut xml = b"<collection xmlns=\"http://www.loc.gov/MARC21/slim\"".to_vec();
        if let Some(prefix) = prefix {
            xml.extend_from_slice(b" xmlns:");
            xml.extend_from_slice(prefix);
            xml.extend_from_slice(b"=\"http://www.loc.gov/MARC21/slim\"");
        }
        xml.push(b'>');
        xml.extend_from_slice(slice);
        xml.extend_from_slice(b"</collection>");
        let limits = XmlLimits {
            max_input_bytes: MAX_RESPONSE_BYTES,
            max_records: 1,
            ..XmlLimits::default()
        };
        match MarcXmlReader::new(xml.as_slice(), limits).next() {
            Some(Ok(record)) => self.records.push(record),
            _ => self.skipped = self.skipped.saturating_add(1),
        }
    }
}
