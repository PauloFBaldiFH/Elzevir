// SPDX-License-Identifier: GPL-3.0-or-later
//! Bounded, streaming bibliographic MARCXML. Diagnostics never echo source values.

mod state;

use std::io::{self, BufRead, Read};

use quick_xml::events::{BytesDecl, Event};
use quick_xml::name::ResolveResult;
use quick_xml::reader::NsReader;

use crate::Record;
use state::State;

/// Maximum XML nesting.
pub const MAX_XML_DEPTH: usize = 32;
/// Maximum raw event and accumulated scalar bytes.
pub const MAX_XML_ELEMENT_BYTES: usize = 1024 * 1024;
/// Maximum raw bytes of one record.
pub const MAX_XML_RECORD_BYTES: usize = 1024 * 1024;
/// Maximum elements of one record.
pub const MAX_XML_ELEMENTS_PER_RECORD: usize = 20_000;
/// Maximum bytes of a MARCXML input; ISO input keeps its existing limits.
pub const MAX_XML_INPUT_BYTES: usize = 512 * 1024 * 1024;
/// Maximum records of a MARCXML input.
pub const MAX_XML_RECORDS: usize = 100_000;

/// XML limits checked while reading, before buffering an unbounded event.
#[derive(Debug, Clone, Copy)]
pub struct XmlLimits {
    /// Maximum nesting, including collection and record.
    pub max_depth: usize,
    /// Maximum event and scalar bytes.
    pub max_element_bytes: usize,
    /// Maximum raw record bytes.
    pub max_record_bytes: usize,
    /// Maximum elements in one record.
    pub max_elements_per_record: usize,
    /// Maximum raw input bytes.
    pub max_input_bytes: usize,
    /// Maximum records.
    pub max_records: usize,
    /// Whether authority records (leader 06 `z`) are read; the catalogue import refuses
    /// them, the authority sources take them (docs/specs/catalogo/fontes-de-autoridade.md).
    pub authorities: bool,
}

impl Default for XmlLimits {
    fn default() -> Self {
        Self {
            max_depth: MAX_XML_DEPTH,
            max_element_bytes: MAX_XML_ELEMENT_BYTES,
            max_record_bytes: MAX_XML_RECORD_BYTES,
            max_elements_per_record: MAX_XML_ELEMENTS_PER_RECORD,
            max_input_bytes: MAX_XML_INPUT_BYTES,
            max_records: MAX_XML_RECORDS,
            authorities: false,
        }
    }
}

/// Safe failure categories; labels are fixed by the parser, never read from XML.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum XmlProblem {
    /// The input cannot form a bibliographic MARCXML record.
    #[error("invalid MARCXML: {0}")]
    Invalid(&'static str),
    /// DTD, non-UTF-8 encoding or a non-bibliographic record.
    #[error("unsupported MARCXML feature: {0}")]
    Forbidden(&'static str),
    /// A named reading limit was exceeded.
    #[error("MARCXML limit exceeded: {0}")]
    Limit(&'static str),
    /// Underlying I/O failed; its payload is deliberately not copied into diagnostics.
    #[error("cannot read MARCXML input")]
    Input,
}

/// A terminal XML error, located without retaining source contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("record {index} at byte {offset}: {problem}")]
pub struct MarcXmlError {
    /// Zero-based record index.
    pub index: u64,
    /// Byte position in the XML source.
    pub offset: u64,
    /// A safe diagnostic category.
    pub problem: XmlProblem,
}

/// Reads one owned record at a time. A structural error ends iteration.
pub struct MarcXmlReader<R> {
    reader: NsReader<Bounded<R>>,
    buffer: Vec<u8>,
    original: Vec<u8>,
    declares_utf8: bool,
    bom_bytes: u64,
    state: State,
    limits: XmlLimits,
    index: u64,
    record_start: u64,
    done: bool,
}

impl<R: BufRead> MarcXmlReader<R> {
    /// Wraps a UTF-8 XML input; no external entity or schema is fetched.
    pub fn new(input: R, limits: XmlLimits) -> Self {
        let mut reader = NsReader::from_reader(Bounded {
            inner: input,
            left: 0,
            raw: Vec::new(),
            start: 0,
        });
        reader.config_mut().expand_empty_elements = true;
        reader.config_mut().check_comments = true;
        reader.config_mut().check_end_names = true;
        Self {
            reader,
            buffer: Vec::new(),
            original: Vec::new(),
            declares_utf8: false,
            bom_bytes: 0,
            state: State::default(),
            limits,
            index: 0,
            record_start: 0,
            done: false,
        }
    }

    /// Start of the most recently begun record, for per-record import failures.
    #[must_use]
    pub const fn record_offset(&self) -> u64 {
        self.record_start + self.bom_bytes
    }

    /// Exact XML bytes from the opening record element through its closing tag.
    pub fn take_record_bytes(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.original)
    }

    /// Whether the XML declaration explicitly specifies UTF-8.
    #[must_use]
    pub const fn declares_utf8(&self) -> bool {
        self.declares_utf8
    }

    fn budget(&self, offset: u64) -> (usize, &'static str) {
        let position = usize::try_from(offset.saturating_add(self.bom_bytes)).unwrap_or(usize::MAX);
        let mut bound = (
            self.limits
                .max_element_bytes
                .min(self.limits.max_record_bytes),
            "element bytes",
        );
        let remaining = self.limits.max_input_bytes.saturating_sub(position);
        if remaining < bound.0 {
            bound = (remaining, "input bytes");
        }
        if self.state.record.is_some() {
            let used =
                usize::try_from(offset.saturating_sub(self.record_start)).unwrap_or(usize::MAX);
            let remaining = self.limits.max_record_bytes.saturating_sub(used);
            if remaining < bound.0 {
                bound = (remaining, "record bytes");
            }
        }
        bound
    }

    fn read_record(&mut self) -> Result<Option<Record>, MarcXmlError> {
        loop {
            self.buffer.clear();
            let offset = self.reader.buffer_position();
            let bound = self.budget(offset);
            if self.state.record.is_none() {
                self.reader.get_mut().discard_before(offset);
            }
            let fail = |problem| MarcXmlError {
                index: self.index,
                offset,
                problem,
            };
            self.reader.get_mut().left = bound.0.saturating_add(1);
            let (namespace, event) = self
                .reader
                .read_resolved_event_into(&mut self.buffer)
                .map_err(|error| fail(xml_error(error, bound.1)))?;
            let valid_namespace = matches!(namespace, ResolveResult::Bound(ns) if ns.as_ref() == b"http://www.loc.gov/MARC21/slim");
            let end = self.reader.buffer_position();
            if offset == 0 && self.bom_bytes == 0 {
                // quick-xml counts positions after the BOM; the capture saw physical bytes.
                self.bom_bytes = self.reader.get_mut().strip_bom();
            }
            if end.saturating_sub(offset) > u64::try_from(bound.0).unwrap_or(u64::MAX) {
                return Err(fail(XmlProblem::Limit(bound.1)));
            }
            // Comments and processing instructions must also be legal XML 1.0 UTF-8.
            if !std::str::from_utf8(&event).is_ok_and(state::legal_text) {
                return Err(fail(XmlProblem::Invalid("XML character")));
            }
            match event {
                Event::Start(start) => {
                    if !valid_namespace {
                        return Err(fail(XmlProblem::Invalid("MARC21 namespace")));
                    }
                    if start.local_name().as_ref() == b"record" {
                        if self.index >= u64::try_from(self.limits.max_records).unwrap_or(u64::MAX)
                        {
                            return Err(fail(XmlProblem::Limit("record count")));
                        }
                        self.record_start = offset;
                    }
                    self.state.begin(&start, &self.limits).map_err(fail)?;
                }
                Event::End(_) => {
                    if let Some(record) = self.state.end(&self.limits).map_err(fail)? {
                        let input = self.reader.get_mut();
                        let len = usize::try_from(end.saturating_sub(self.record_start))
                            .map_err(|_| fail(XmlProblem::Limit("record bytes")))?;
                        self.original = input
                            .raw
                            .get(..len)
                            .ok_or_else(|| fail(XmlProblem::Input))?
                            .to_vec();
                        self.index = self.index.saturating_add(1);
                        return Ok(Some(record));
                    }
                }
                // Do not trim or normalize field text: converted source must round-trip.
                Event::Text(text) => self
                    .state
                    .append(
                        &text
                            .decode()
                            .map_err(|_| fail(XmlProblem::Invalid("UTF-8 text")))?,
                        &self.limits,
                    )
                    .map_err(fail)?,
                Event::CData(text) => self
                    .state
                    .append(
                        &text
                            .decode()
                            .map_err(|_| fail(XmlProblem::Invalid("UTF-8 text")))?,
                        &self.limits,
                    )
                    .map_err(fail)?,
                Event::GeneralRef(reference) => {
                    let value = reference_text(&reference).map_err(fail)?;
                    self.state.append(&value, &self.limits).map_err(fail)?;
                }
                Event::Decl(declaration) => {
                    validate_declaration(&declaration, offset).map_err(fail)?;
                    self.declares_utf8 = declaration.encoding().is_some();
                }
                Event::DocType(_) => return Err(fail(XmlProblem::Forbidden("DTD"))),
                Event::Comment(_) | Event::PI(_) => {}
                Event::Eof if self.state.closed => return Ok(None),
                Event::Eof | Event::Empty(_) => {
                    return Err(fail(XmlProblem::Invalid("incomplete document")));
                }
            }
        }
    }
}

fn reference_text(reference: &quick_xml::events::BytesRef<'_>) -> Result<String, XmlProblem> {
    if let Some(character) = reference
        .resolve_char_ref()
        .map_err(|_| XmlProblem::Invalid("character reference"))?
    {
        Ok(character.to_string())
    } else {
        quick_xml::escape::resolve_xml_entity(
            &reference
                .decode()
                .map_err(|_| XmlProblem::Invalid("UTF-8 reference"))?,
        )
        .map(str::to_owned)
        .ok_or(XmlProblem::Forbidden("entity"))
    }
}

fn validate_declaration(declaration: &BytesDecl<'_>, offset: u64) -> Result<(), XmlProblem> {
    if offset != 0
        || declaration
            .version()
            .map_err(|_| XmlProblem::Invalid("declaration"))?
            .as_ref()
            != b"1.0"
    {
        return Err(XmlProblem::Forbidden("XML declaration"));
    }
    if let Some(encoding) = declaration.encoding()
        && !encoding
            .map_err(|_| XmlProblem::Invalid("encoding"))?
            .eq_ignore_ascii_case(b"UTF-8")
    {
        return Err(XmlProblem::Forbidden("encoding"));
    }
    Ok(())
}

// Parser diagnostics can contain user data; report only the category.
fn xml_error(error: quick_xml::Error, bound: &'static str) -> XmlProblem {
    match error {
        quick_xml::Error::Io(error)
            if error
                .get_ref()
                .is_some_and(<dyn std::error::Error + Send + Sync>::is::<BudgetExceeded>) =>
        {
            XmlProblem::Limit(bound)
        }
        quick_xml::Error::Io(_) => XmlProblem::Input,
        _ => XmlProblem::Invalid("XML syntax"),
    }
}

impl<R: BufRead> Iterator for MarcXmlReader<R> {
    type Item = Result<Record, MarcXmlError>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        match self.read_record() {
            Ok(Some(record)) => Some(Ok(record)),
            Ok(None) => {
                self.done = true;
                None
            }
            Err(error) => {
                self.done = true;
                Some(Err(error))
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("XML event reading budget exhausted")]
struct BudgetExceeded;

struct Bounded<R> {
    inner: R,
    left: usize,
    // quick-xml can consume '<' from the next event; retain that lookahead.
    raw: Vec<u8>,
    start: u64,
}

impl<R> Bounded<R> {
    fn strip_bom(&mut self) -> u64 {
        if self.raw.starts_with(b"\xef\xbb\xbf") {
            self.raw.drain(..3);
            3
        } else {
            0
        }
    }

    fn discard_before(&mut self, offset: u64) {
        let discard = usize::try_from(offset.saturating_sub(self.start))
            .unwrap_or(usize::MAX)
            .min(self.raw.len());
        self.raw.drain(..discard);
        self.start += discard as u64;
    }
}

impl<R: BufRead> Read for Bounded<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        let count = self.fill_buf()?.read(out)?;
        self.consume(count);
        Ok(count)
    }
}

impl<R: BufRead> BufRead for Bounded<R> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.left == 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, BudgetExceeded));
        }
        let bytes = self.inner.fill_buf()?;
        Ok(bytes.get(..self.left).unwrap_or(bytes))
    }
    fn consume(&mut self, amount: usize) {
        let count = amount.min(self.left);
        if let Ok(bytes) = self.inner.fill_buf()
            && let Some(consumed) = bytes.get(..count)
        {
            self.raw.extend_from_slice(consumed);
        }
        self.inner.consume(count);
        self.left -= count;
    }
}
