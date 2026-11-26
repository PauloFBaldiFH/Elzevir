// SPDX-License-Identifier: GPL-3.0-or-later
//! The rules of an SRU request (docs/specs/opac/sru.md, items 4 to 7).

use crate::field_search::FieldQuery;

use super::{Diagnostic, SruSettings, cql};

/// Longest parameter value read (item 4).
const MAX_VALUE_CHARS: usize = 2048;

/// The parameters a request may carry, besides the `x-` extensions (item 4).
const KNOWN: [&str; 13] = [
    "operation",
    "version",
    "query",
    "startRecord",
    "maximumRecords",
    "recordSchema",
    "recordPacking",
    "recordXMLEscaping",
    "queryType",
    "resultSetTTL",
    "stylesheet",
    "httpAccept",
    "sortKeys",
];

/// The SRU version answered (item 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    /// SRU 1.1.
    V1_1,
    /// SRU 1.2, the default.
    V1_2,
    /// SRU 2.0.
    V2_0,
}

impl Version {
    /// As the `version` element writes it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::V1_1 => "1.1",
            Self::V1_2 => "1.2",
            Self::V2_0 => "2.0",
        }
    }
}

/// The schema of the records (item 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordSchema {
    /// MARCXML.
    MarcXml,
    /// Simple Dublin Core.
    DublinCore,
}

impl RecordSchema {
    /// The short name the records carry.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::MarcXml => "marcxml",
            Self::DublinCore => "dc",
        }
    }

    /// The schema's identifier, for `explain`.
    #[must_use]
    pub const fn identifier(self) -> &'static str {
        match self {
            Self::MarcXml => "info:srw/schema/1/marcxml-v1.1",
            Self::DublinCore => "info:srw/schema/1/dc-v1.1",
        }
    }

    fn of(name: &str) -> Option<Self> {
        [Self::MarcXml, Self::DublinCore]
            .into_iter()
            .find(|schema| name == schema.name() || name == schema.identifier())
            .or_else(|| (name == "marc21").then_some(Self::MarcXml))
    }
}

/// How a record goes inside `recordData` (item 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Packing {
    /// As XML.
    Xml,
    /// As escaped text.
    String,
}

impl Packing {
    /// As the response writes it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Xml => "xml",
            Self::String => "string",
        }
    }
}

/// A `searchRetrieve`, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SearchRetrieve {
    pub(super) query: FieldQuery,
    /// 1 for the first record.
    pub(super) start: usize,
    pub(super) maximum: usize,
    pub(super) schema: RecordSchema,
    pub(super) packing: Packing,
}

/// What the response answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Operation {
    Explain,
    SearchRetrieve(SearchRetrieve),
}

/// A request read: the version and envelope of the answer, and the operation or the
/// diagnostic that refuses it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Parsed {
    pub(super) version: Version,
    /// `explainResponse` rather than `searchRetrieveResponse`.
    pub(super) explain: bool,
    pub(super) operation: Result<Operation, Diagnostic>,
}

pub(super) fn parse(arguments: &[(String, String)], settings: &SruSettings) -> Parsed {
    let value = |name: &str| {
        arguments
            .iter()
            .find(|(given, _)| given == name)
            .map(|(_, value)| value.as_str())
    };
    let explain =
        value("operation").map_or(value("query").is_none(), |operation| operation == "explain");
    let version = match value("version") {
        None | Some("1.2") => Ok(Version::V1_2),
        Some("1.1") => Ok(Version::V1_1),
        Some("2.0") => Ok(Version::V2_0),
        Some(_) => Err(Diagnostic::with(5, "1.2")),
    };
    let operation = version
        .clone()
        .and_then(|version| operation(arguments, version, settings, explain));
    Parsed {
        version: version.unwrap_or(Version::V1_2),
        explain,
        operation,
    }
}

fn operation(
    arguments: &[(String, String)],
    version: Version,
    settings: &SruSettings,
    explain: bool,
) -> Result<Operation, Diagnostic> {
    for (index, (name, value)) in arguments.iter().enumerate() {
        if !name.starts_with("x-") && !KNOWN.contains(&name.as_str()) {
            return Err(Diagnostic::with(8, name));
        }
        let repeated = arguments
            .iter()
            .skip(index + 1)
            .any(|(other, _)| other == name);
        if repeated || value.chars().count() > MAX_VALUE_CHARS {
            return Err(Diagnostic::with(6, name));
        }
    }
    let value = |name: &str| {
        arguments
            .iter()
            .find(|(given, _)| given == name)
            .map(|(_, value)| value.as_str())
    };
    match value("operation") {
        None | Some("searchRetrieve" | "explain") => {}
        Some(_) => return Err(Diagnostic::bare(4)),
    }
    if explain {
        return Ok(Operation::Explain);
    }
    let v2 = version == Version::V2_0;
    if !v2 && value("recordXMLEscaping").is_some() {
        return Err(Diagnostic::with(8, "recordXMLEscaping"));
    }
    if !v2 && value("queryType").is_some() {
        return Err(Diagnostic::with(8, "queryType"));
    }
    if value("queryType").is_some_and(|kind| kind != "cql") {
        return Err(Diagnostic::with(6, "queryType"));
    }
    let query = match value("query").map(str::trim) {
        None | Some("") => return Err(Diagnostic::with(7, "query")),
        Some(query) => cql::parse(query)?,
    };
    let start = number(value("startRecord"), "startRecord", 1)?;
    if start == 0 {
        return Err(Diagnostic::with(6, "startRecord"));
    }
    let maximum = number(
        value("maximumRecords"),
        "maximumRecords",
        settings.default_records,
    )?
    .min(settings.max_records);
    let schema = match value("recordSchema") {
        None => RecordSchema::MarcXml,
        Some(name) => RecordSchema::of(name).ok_or_else(|| Diagnostic::with(66, name))?,
    };
    let packing_name = if v2 {
        if !matches!(value("recordPacking"), None | Some("packed" | "unpacked")) {
            return Err(Diagnostic::bare(71));
        }
        value("recordXMLEscaping")
    } else {
        value("recordPacking")
    };
    let packing = match packing_name {
        None | Some("xml") => Packing::Xml,
        Some("string") => Packing::String,
        Some(_) => return Err(Diagnostic::bare(71)),
    };
    Ok(Operation::SearchRetrieve(SearchRetrieve {
        query,
        start,
        maximum,
        schema,
        packing,
    }))
}

fn number(value: Option<&str>, name: &str, default: usize) -> Result<usize, Diagnostic> {
    value.map_or(Ok(default), |text| {
        text.trim().parse().map_err(|_| Diagnostic::with(6, name))
    })
}

#[cfg(test)]
mod tests {
    use crate::field_search::{FieldTerm, SearchField, WordMatch};

    use super::*;

    fn settings() -> SruSettings {
        SruSettings {
            host: "biblioteca.example".to_owned(),
            port: 443,
            database: "sru".to_owned(),
            title: "Biblioteca".to_owned(),
            default_records: 10,
            max_records: 50,
        }
    }

    fn parsed(pairs: &[(&str, &str)]) -> Parsed {
        let arguments: Vec<(String, String)> = pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect();
        parse(&arguments, &settings())
    }

    fn refused(pairs: &[(&str, &str)]) -> (u16, Option<String>) {
        let diagnostic = parsed(pairs).operation.unwrap_err();
        (diagnostic.code, diagnostic.details)
    }

    #[test]
    fn a_search_takes_the_defaults() {
        let request = parsed(&[("query", "rio")]);
        assert_eq!(request.version, Version::V1_2);
        assert!(!request.explain);
        assert_eq!(
            request.operation.unwrap(),
            Operation::SearchRetrieve(SearchRetrieve {
                query: FieldQuery::Term(FieldTerm {
                    field: SearchField::Any,
                    matching: WordMatch::All,
                    text: "rio".to_owned(),
                }),
                start: 1,
                maximum: 10,
                schema: RecordSchema::MarcXml,
                packing: Packing::Xml,
            })
        );
        let Operation::SearchRetrieve(asked) = parsed(&[
            ("operation", "searchRetrieve"),
            ("version", "2.0"),
            ("query", "rio"),
            ("startRecord", "11"),
            ("maximumRecords", "500"),
            ("recordSchema", "info:srw/schema/1/dc-v1.1"),
            ("recordXMLEscaping", "string"),
            ("recordPacking", "packed"),
            ("queryType", "cql"),
            ("x-info-1", "anything"),
        ])
        .operation
        .unwrap() else {
            panic!("not a search");
        };
        assert_eq!(
            (asked.start, asked.maximum, asked.schema, asked.packing),
            (11, 50, RecordSchema::DublinCore, Packing::String)
        );
    }

    #[test]
    fn explain_is_the_default_without_a_query() {
        assert_eq!(parsed(&[]).operation.unwrap(), Operation::Explain);
        let explained = parsed(&[("operation", "explain"), ("version", "1.1")]);
        assert!(explained.explain);
        assert_eq!(explained.version, Version::V1_1);
        assert_eq!(explained.operation.unwrap(), Operation::Explain);
    }

    #[test]
    fn each_refusal_has_its_diagnostic() {
        assert_eq!(
            refused(&[("operation", "scan"), ("scanClause", "x")]),
            (8, Some("scanClause".to_owned()))
        );
        assert_eq!(refused(&[("operation", "scan")]), (4, None));
        assert_eq!(
            refused(&[("version", "3.0"), ("query", "a")]),
            (5, Some("1.2".to_owned()))
        );
        assert_eq!(
            refused(&[("query", "a"), ("query", "b")]),
            (6, Some("query".to_owned()))
        );
        assert_eq!(
            refused(&[("query", &"a".repeat(MAX_VALUE_CHARS + 1))]),
            (6, Some("query".to_owned()))
        );
        assert_eq!(
            refused(&[("operation", "searchRetrieve")]),
            (7, Some("query".to_owned()))
        );
        assert_eq!(
            refused(&[("operation", "searchRetrieve"), ("query", " ")]),
            (7, Some("query".to_owned()))
        );
        assert_eq!(
            refused(&[("query", "a"), ("startRecord", "0")]),
            (6, Some("startRecord".to_owned()))
        );
        assert_eq!(
            refused(&[("query", "a"), ("maximumRecords", "-1")]),
            (6, Some("maximumRecords".to_owned()))
        );
        assert_eq!(
            refused(&[("query", "a"), ("recordSchema", "mods")]),
            (66, Some("mods".to_owned()))
        );
        assert_eq!(
            refused(&[("query", "a"), ("recordPacking", "json")]),
            (71, None)
        );
        assert_eq!(
            refused(&[("query", "a"), ("recordXMLEscaping", "xml")]),
            (8, Some("recordXMLEscaping".to_owned()))
        );
        assert_eq!(
            refused(&[("version", "2.0"), ("query", "a"), ("queryType", "pqf")]),
            (6, Some("queryType".to_owned()))
        );
        assert_eq!(
            refused(&[("query", "dc.shelf=a")]),
            (16, Some("dc.shelf".to_owned()))
        );
    }
}
