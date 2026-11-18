// SPDX-License-Identifier: GPL-3.0-or-later
//! Bounded content evidence, not validation of an entire import source.

pub(crate) mod sql;

/// Maximum prefix inspected; the CLI keeps these bytes in its import buffer.
pub const MAX_DETECTION_BYTES: usize = 8 * 1024;

/// The source families of the universal import roadmap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ImportFormat {
    /// The existing binary MARC path.
    Iso2709,
    /// MARC21 XML records or collection.
    MarcXml,
    /// `MarcEdit` mnemonic records.
    MarcText,
    /// A table with a consistent delimiter.
    Delimited,
    /// Declared Koha catalog or item tables.
    KohaSqlDump,
    /// Catalog, circulation and configuration tables together.
    KohaFullBackup,
    /// Insufficient, ambiguous or unsupported container evidence.
    Unknown,
}

/// Evidence contains no source values, so it can be reported without leaking rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Evidence {
    /// Preserve the existing empty/whitespace-only ISO report.
    EmptyInput,
    /// Numeric positions of an ISO leader, possibly after a damaged record.
    IsoLeader,
    /// A MARC root bound to the MARC21 namespace.
    MarcXmlNamespace,
    /// A `MarcEdit` leader line.
    MarcTextLeader,
    /// Two logical rows with equal column counts for one delimiter.
    DelimitedRows,
    /// More than one delimiter fits the sample.
    AmbiguousDelimiter,
    /// Koha catalog/item table declarations outside comments and values.
    KohaCatalogTables,
    /// Catalog, borrower, circulation and preference declarations.
    KohaSchemaTables,
    /// A ZIP signature; no member has been opened.
    ZipSignature,
    /// A gzip signature; nothing has been decompressed.
    GzipSignature,
    /// The prefix does not establish a supported family.
    Inconclusive,
}

/// A provisional format and the evidence that supports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Detection {
    /// The source family; the reader must still validate the entire source.
    pub format: ImportFormat,
    /// Safe, structured evidence, never a copy of input bytes.
    pub evidence: Evidence,
}

/// Selects a MARC reader while retaining the evidence found in the source.
///
/// Positive evidence of a table, SQL or a container always excludes a MARC reader.
#[must_use]
pub fn select_marc(detected: Detection, selected: Option<ImportFormat>) -> Option<Detection> {
    use ImportFormat::{Iso2709, MarcText, MarcXml};
    if !matches!(detected.format, Iso2709 | MarcXml | MarcText)
        && detected.evidence != Evidence::Inconclusive
    {
        return None;
    }
    let format = selected.unwrap_or(detected.format);
    matches!(format, Iso2709 | MarcXml | MarcText).then_some(Detection {
        format,
        evidence: detected.evidence,
    })
}

/// Selects a reader for staged bibliographic records, including validated Koha SQL.
#[must_use]
pub fn select_prepared(detected: Detection, selected: Option<ImportFormat>) -> Option<Detection> {
    if selected.unwrap_or(detected.format) == ImportFormat::KohaSqlDump
        && (detected.format == ImportFormat::KohaSqlDump
            || matches!(
                detected.evidence,
                Evidence::EmptyInput | Evidence::Inconclusive
            ))
    {
        Some(Detection {
            format: ImportFormat::KohaSqlDump,
            evidence: detected.evidence,
        })
    } else {
        select_marc(detected, selected)
    }
}

/// Inspects at most [`MAX_DETECTION_BYTES`] without I/O or executing source content.
/// SQL containers take precedence over MARC/XML embedded in their values.
#[must_use]
pub fn detect_format(prefix: &[u8]) -> Detection {
    let prefix = prefix.get(..MAX_DETECTION_BYTES).unwrap_or(prefix);
    let found = |format, evidence| Detection { format, evidence };
    if prefix.iter().all(u8::is_ascii_whitespace) {
        return found(ImportFormat::Iso2709, Evidence::EmptyInput);
    }
    if [b"PK\x03\x04", b"PK\x05\x06", b"PK\x07\x08"]
        .iter()
        .any(|signature| prefix.starts_with(*signature))
    {
        return found(ImportFormat::Unknown, Evidence::ZipSignature);
    }
    if prefix.starts_with(b"\x1f\x8b\x08") {
        return found(ImportFormat::Unknown, Evidence::GzipSignature);
    }
    let input = prefix
        .strip_prefix(b"\xef\xbb\xbf")
        .unwrap_or(prefix)
        .trim_ascii_start();
    if let Some(detected) = sql::detect(input) {
        return detected;
    }
    if marcxml_root(input).is_some() {
        return found(ImportFormat::MarcXml, Evidence::MarcXmlNamespace);
    }
    if input.starts_with(b"=LDR ") {
        return found(ImportFormat::MarcText, Evidence::MarcTextLeader);
    }
    if prefix.split(|byte| *byte == 0x1d).any(iso_leader) {
        return found(ImportFormat::Iso2709, Evidence::IsoLeader);
    }
    let candidates = b",;\t"
        .iter()
        .copied()
        .filter(|delimiter| delimited_rows(input, *delimiter).is_some())
        .count();
    match candidates {
        1 => found(ImportFormat::Delimited, Evidence::DelimitedRows),
        2.. => found(ImportFormat::Unknown, Evidence::AmbiguousDelimiter),
        _ => found(ImportFormat::Unknown, Evidence::Inconclusive),
    }
}

fn iso_leader(bytes: &[u8]) -> bool {
    // Do not validate or repair records here: malformed ISO belongs to MarcReader.
    let Some(leader) = bytes.get(..24) else {
        return false;
    };
    leader
        .get(..5)
        .is_some_and(|part| part.iter().all(u8::is_ascii_digit))
        && leader
            .get(12..17)
            .is_some_and(|part| part.iter().all(u8::is_ascii_digit))
        && (leader.get(10..12) == Some(b"22") || bytes.contains(&0x1e))
}

/// Returns the unique tabular separator supported by the detection sample.
#[must_use]
pub fn table_delimiter(prefix: &[u8]) -> Option<crate::import::table::Delimiter> {
    use crate::import::table::Delimiter;
    let prefix = prefix.get(..MAX_DETECTION_BYTES).unwrap_or(prefix);
    let input = prefix
        .strip_prefix(b"\xef\xbb\xbf")
        .unwrap_or(prefix)
        .trim_ascii_start();
    let mut candidates = [Delimiter::Comma, Delimiter::Semicolon, Delimiter::Tab]
        .into_iter()
        .filter(|delimiter| {
            let separator = delimiter.byte();
            delimited_rows(input, separator).is_some()
                || input.contains(&separator)
                    && input
                        .iter()
                        .all(|byte| *byte == separator || byte.is_ascii_whitespace())
                || model_header(input, *delimiter)
        });
    match (candidates.next(), candidates.next()) {
        (Some(separator), None) => Some(separator),
        _ => None,
    }
}

fn model_header(input: &[u8], delimiter: crate::import::table::Delimiter) -> bool {
    if !input.starts_with(b"elzevir-model:") {
        return false;
    }
    let line = input.split(|byte| matches!(byte, b'\r' | b'\n')).next();
    let mut header = Vec::new();
    crate::import::table::write_model_header(&mut header, delimiter).is_ok()
        && line == header.strip_suffix(b"\n")
}

fn marcxml_root(mut input: &[u8]) -> Option<()> {
    loop {
        input = input.trim_ascii_start();
        let (rest, closing): (&[u8], &[u8]) = if let Some(rest) = input.strip_prefix(b"<?") {
            (rest, b"?>")
        } else if let Some(rest) = input.strip_prefix(b"<!--") {
            (rest, b"-->")
        } else {
            break;
        };
        let end = rest
            .windows(closing.len())
            .position(|window| window == closing)?;
        input = rest.get(end + closing.len()..)?;
    }
    let (root, mut attributes) = xml_name(input.strip_prefix(b"<")?)?;
    let mut parts = root.splitn(2, |byte| *byte == b':');
    let first = parts.next()?;
    let (namespace, local) = parts
        .next()
        .map_or((None, first), |local| (Some(first), local));
    if local != b"record" && local != b"collection" {
        return None;
    }
    let mut bound = false;
    loop {
        attributes = attributes.trim_ascii_start();
        if attributes.starts_with(b">") || attributes.starts_with(b"/>") {
            return bound.then_some(());
        }
        let (name, rest) = xml_name(attributes)?;
        let rest = rest
            .trim_ascii_start()
            .strip_prefix(b"=")?
            .trim_ascii_start();
        let (&quote, value) = rest.split_first()?;
        if !matches!(quote, b'\'' | b'"') {
            return None;
        }
        let end = value.iter().position(|byte| *byte == quote)?;
        let matches = namespace.map_or(name == b"xmlns", |prefix| {
            name.strip_prefix(b"xmlns:") == Some(prefix)
        });
        if matches {
            if bound || value.get(..end)? != b"http://www.loc.gov/MARC21/slim" {
                return None;
            }
            bound = true;
        }
        attributes = value.get(end + 1..)?;
    }
}

fn xml_name(input: &[u8]) -> Option<(&[u8], &[u8])> {
    let end = input
        .iter()
        .position(|byte| !(byte.is_ascii_alphanumeric() || b"_:-.".contains(byte)))
        .unwrap_or(input.len());
    if end == 0 {
        return None;
    }
    Some((input.get(..end)?, input.get(end..)?))
}

fn delimited_rows(input: &[u8], delimiter: u8) -> Option<usize> {
    let mut bytes = input.iter().copied().peekable();
    let (mut quoted, mut closed, mut start, mut present) = (false, false, true, false);
    let (mut columns, mut expected) = (1, None);
    while let Some(byte) = bytes.next() {
        if quoted {
            if byte == b'"' {
                if bytes.peek() == Some(&b'"') {
                    bytes.next();
                } else {
                    quoted = false;
                    closed = true;
                }
            }
            continue;
        }
        if byte == delimiter {
            columns += 1;
            (start, closed, present) = (true, false, true);
        } else if matches!(byte, b'\r' | b'\n') {
            if byte == b'\r' && bytes.peek() == Some(&b'\n') {
                bytes.next();
            }
            if present {
                if columns < 2 || expected.is_some_and(|count| count != columns) {
                    return None;
                }
                if expected.is_some() {
                    return Some(columns);
                }
                expected = Some(columns);
            }
            (columns, start, closed, present) = (1, true, false, false);
        } else if byte == b'"' && start {
            (quoted, start, present) = (true, false, true);
        } else if closed || byte == b'"' {
            return None;
        } else {
            (start, present) = (false, true);
        }
    }
    (!quoted && present && expected == Some(columns)).then_some(columns)
}
