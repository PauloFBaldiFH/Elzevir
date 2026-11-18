// SPDX-License-Identifier: GPL-3.0-or-later
//! Streaming `MarcEdit` mnemonic records. Diagnostics report structural errors and positions.

use std::io::{BufRead, Read};

use crate::{Field, LEADER_LEN, Record, Subfield, Tag};

/// Maximum raw bytes of one physical line, including its terminator.
pub const MAX_MRK_LINE_BYTES: usize = 1024 * 1024;
/// Maximum raw bytes of the leader and field lines of one record.
pub const MAX_MRK_RECORD_BYTES: usize = 1024 * 1024;
/// Maximum physical lines in one record, including its leader.
pub const MAX_MRK_LINES_PER_RECORD: usize = 20_000;
/// Maximum raw bytes of a `MarcEdit` input.
pub const MAX_MRK_INPUT_BYTES: usize = 512 * 1024 * 1024;
/// Maximum records in a `MarcEdit` input.
pub const MAX_MRK_RECORDS: usize = 100_000;

const BOM: &[u8] = b"\xef\xbb\xbf";
const SEPARATOR_BYTES: usize = b"\r\n".len();
const ESCAPES: &[(&[u8], u8)] = &[
    (b"dollar}", b'$'),
    (b"bsol}", b'\\'),
    (b"lcub}", b'{'),
    (b"rcub}", b'}'),
];

/// Bounds checked before reading beyond the current line or record budget.
#[derive(Debug, Clone, Copy)]
pub struct MrkLimits {
    /// Maximum bytes of one line.
    pub max_line_bytes: usize,
    /// Maximum bytes of one record.
    pub max_record_bytes: usize,
    /// Maximum lines of one record.
    pub max_lines_per_record: usize,
    /// Maximum bytes of the entire input.
    pub max_input_bytes: usize,
    /// Maximum number of records.
    pub max_records: usize,
}

impl Default for MrkLimits {
    fn default() -> Self {
        Self {
            max_line_bytes: MAX_MRK_LINE_BYTES,
            max_record_bytes: MAX_MRK_RECORD_BYTES,
            max_lines_per_record: MAX_MRK_LINES_PER_RECORD,
            max_input_bytes: MAX_MRK_INPUT_BYTES,
            max_records: MAX_MRK_RECORDS,
        }
    }
}

/// Safe failure categories with fixed labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum MrkProblem {
    /// The source does not form a supported UTF-8 bibliographic record.
    #[error("invalid MarcEdit input: {0}")]
    Invalid(&'static str),
    /// A named reading limit was exceeded.
    #[error("MarcEdit limit exceeded: {0}")]
    Limit(&'static str),
    /// I/O failed. Its message may contain source data or a private path.
    #[error("cannot read MarcEdit input")]
    Input,
}

/// A terminal `MarcEdit` error, located without retaining the source line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("record {index}, line {line} at byte {offset}: {problem}")]
pub struct MarcTextError {
    /// Zero-based record index.
    pub index: u64,
    /// One-based physical line number.
    pub line: u64,
    /// Zero-based byte offset of the physical line.
    pub offset: u64,
    /// A safe diagnostic category.
    pub problem: MrkProblem,
}

/// Reads one owned bibliographic record at a time from a UTF-8 `MarcEdit` file.
pub struct MarcTextReader<R> {
    input: R,
    limits: MrkLimits,
    line: Vec<u8>,
    offset: usize,
    line_number: u64,
    record_start: usize,
    index: usize,
    done: bool,
}

impl<R: BufRead> MarcTextReader<R> {
    /// Wraps a source of physical lines separated by LF or CRLF.
    pub const fn new(input: R, limits: MrkLimits) -> Self {
        Self {
            input,
            limits,
            line: Vec::new(),
            offset: 0,
            line_number: 0,
            record_start: 0,
            index: 0,
            done: false,
        }
    }

    /// Raw byte offset of the leader line of the last record read.
    #[must_use]
    pub const fn record_offset(&self) -> u64 {
        self.record_start as u64
    }

    fn read_line(&mut self, bytes_left: usize, lines: usize) -> Result<usize, MrkProblem> {
        // A record at its limit can still be followed by an empty separator line.
        let separator = SEPARATOR_BYTES + if self.offset == 0 { BOM.len() } else { 0 };
        let (budget, label) = [
            (self.limits.max_line_bytes, "line bytes"),
            (
                self.limits.max_input_bytes.saturating_sub(self.offset),
                "input bytes",
            ),
            (bytes_left.max(separator), "record bytes"),
            (
                if lines >= self.limits.max_lines_per_record {
                    separator
                } else {
                    usize::MAX
                },
                "lines per record",
            ),
            (
                if self.index >= self.limits.max_records {
                    separator
                } else {
                    usize::MAX
                },
                "record count",
            ),
        ]
        .into_iter()
        .min_by_key(|(remaining, _)| *remaining)
        .unwrap_or((0, "line bytes"));
        self.line.clear();
        // Allow one extra byte to distinguish an exact limit from a truncated line.
        let length = self
            .input
            .by_ref()
            .take((budget as u64).saturating_add(1))
            .read_until(b'\n', &mut self.line)
            .map_err(|_| MrkProblem::Input)?;
        if length > budget {
            return Err(MrkProblem::Limit(label));
        }
        self.offset += length;
        Ok(length)
    }

    fn read_record(&mut self) -> Result<Option<Record>, MarcTextError> {
        let mut record: Option<Record> = None;
        let mut lines = 0;
        loop {
            let offset = self.offset;
            let index = self.index as u64;
            let line_number = self.line_number + 1;
            let fail = |problem| MarcTextError {
                index,
                line: line_number,
                offset: offset as u64,
                problem,
            };
            let used = if record.is_some() {
                offset - self.record_start
            } else {
                0
            };
            let bytes_left = self.limits.max_record_bytes.saturating_sub(used);
            let length = self.read_line(bytes_left, lines).map_err(fail)?;
            if length == 0 {
                return Ok(record);
            }
            self.line_number = line_number;
            let content = line_content(&self.line, offset == 0);
            if content.is_empty() {
                if record.is_some() {
                    return Ok(record);
                }
                continue;
            }
            if length > bytes_left {
                return Err(fail(MrkProblem::Limit("record bytes")));
            }
            if lines >= self.limits.max_lines_per_record {
                return Err(fail(MrkProblem::Limit("lines per record")));
            }
            if self.index >= self.limits.max_records {
                return Err(fail(MrkProblem::Limit("record count")));
            }
            if std::str::from_utf8(content).is_err()
                || content
                    .iter()
                    .any(|byte| byte.is_ascii_control() && *byte != b'\t')
            {
                return Err(fail(MrkProblem::Invalid("UTF-8 text")));
            }
            if let Some(current) = &mut record {
                current.fields.push(field(content).map_err(fail)?);
            } else {
                let leader = leader(content).map_err(fail)?;
                self.record_start = offset;
                record = Some(Record {
                    leader,
                    fields: Vec::new(),
                });
            }
            lines += 1;
        }
    }
}

impl<R: BufRead> Iterator for MarcTextReader<R> {
    type Item = Result<Record, MarcTextError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        match self.read_record() {
            Ok(Some(record)) => {
                self.index += 1;
                Some(Ok(record))
            }
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

fn line_content(line: &[u8], first: bool) -> &[u8] {
    let content = if first {
        line.strip_prefix(BOM).unwrap_or(line)
    } else {
        line
    };
    content
        .strip_suffix(b"\n")
        .map_or(content, |line| line.strip_suffix(b"\r").unwrap_or(line))
}

const fn blank(byte: u8) -> u8 {
    if byte == b'\\' { b' ' } else { byte }
}

fn leader(line: &[u8]) -> Result<[u8; LEADER_LEN], MrkProblem> {
    let value = line
        .strip_prefix(b"=LDR  ")
        .ok_or(MrkProblem::Invalid("missing leader"))?;
    let raw: [u8; LEADER_LEN] = value
        .try_into()
        .map_err(|_| MrkProblem::Invalid("leader length"))?;
    let mut leader = raw.map(blank);
    if !leader
        .iter()
        .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
        || !leader
            .get(..5)
            .is_some_and(|part| part.iter().all(u8::is_ascii_digit))
        || !leader
            .get(12..17)
            .is_some_and(|part| part.iter().all(u8::is_ascii_digit))
    {
        return Err(MrkProblem::Invalid("leader"));
    }
    if leader.get(6) == Some(&b'z') {
        return Err(MrkProblem::Invalid("authority leader"));
    }
    *leader
        .get_mut(9)
        .ok_or(MrkProblem::Invalid("leader encoding"))? = b'a';
    Ok(leader)
}

fn field(line: &[u8]) -> Result<Field, MrkProblem> {
    let [b'=', a, b, c, b' ', b' ', content @ ..] = line else {
        return Err(MrkProblem::Invalid("field prefix"));
    };
    let bytes = [*a, *b, *c];
    if bytes == *b"000" || !bytes.iter().all(u8::is_ascii_digit) {
        return Err(MrkProblem::Invalid("field tag"));
    }
    let tag = Tag::new(bytes);
    if tag.is_control() {
        return Ok(Field::Control {
            tag,
            value: value(content, true)?,
        });
    }
    let [first, second, data @ ..] = content else {
        return Err(MrkProblem::Invalid("indicators"));
    };
    let indicators = [blank(*first), blank(*second)];
    if !indicators
        .iter()
        .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
    {
        return Err(MrkProblem::Invalid("indicators"));
    }
    let subfields = if data.is_empty() {
        Vec::new()
    } else {
        data.strip_prefix(b"$")
            .ok_or(MrkProblem::Invalid("missing subfield delimiter"))?
            .split(|byte| *byte == b'$')
            .map(subfield)
            .collect::<Result<_, _>>()?
    };
    Ok(Field::Data {
        tag,
        indicators,
        subfields,
    })
}

fn subfield(data: &[u8]) -> Result<Subfield, MrkProblem> {
    let Some((&code, text)) = data
        .split_first()
        .filter(|(code, _)| code.is_ascii_alphanumeric())
    else {
        return Err(MrkProblem::Invalid("subfield code"));
    };
    Ok(Subfield {
        code,
        value: value(text, false)?,
    })
}

fn value(mut text: &[u8], fixed: bool) -> Result<Vec<u8>, MrkProblem> {
    let mut decoded = Vec::with_capacity(text.len());
    while let Some((&byte, rest)) = text.split_first() {
        text = rest;
        match byte {
            b'{' => {
                let (escaped, tail) = ESCAPES
                    .iter()
                    .find_map(|(escape, byte)| rest.strip_prefix(*escape).map(|tail| (*byte, tail)))
                    .ok_or(MrkProblem::Invalid("unknown or incomplete escape"))?;
                decoded.push(escaped);
                text = tail;
            }
            b'}' => return Err(MrkProblem::Invalid("unescaped closing brace")),
            _ => decoded.push(if fixed { blank(byte) } else { byte }),
        }
    }
    Ok(decoded)
}
