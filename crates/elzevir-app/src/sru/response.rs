// SPDX-License-Identifier: GPL-3.0-or-later
//! The SRU response of a request (docs/specs/opac/sru.md, items 13 to 18).

use std::fmt::Write as _;

use elzevir_marc::{Limits, Record};

use crate::export::{RecordExportError, marc_record};
use crate::field_search::FieldSearch;
use crate::oai::{OaiCatalog, dublin_core, escape, marcxml_element};
use crate::search::SearchScope;

use super::request::{Operation, Packing, RecordSchema, SearchRetrieve, Version, parse};
use super::{CONTEXT_SETS, Diagnostic, INDEXES, SruSettings};

/// The schema of a surrogate diagnostic in place of a record (item 15).
const DIAGNOSTIC_SCHEMA: &str = "info:srw/schema/1/diagnostics-v1.1";

/// The schema of the explain record (item 18).
const ZEEREX: &str = "http://explain.z3950.org/dtd/2.0/";

/// A response and what the log says of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SruAnswer {
    /// The XML document.
    pub xml: String,
    /// `explain` or `searchRetrieve`.
    pub operation: &'static str,
    /// The version answered.
    pub version: Version,
    /// The diagnostic that refused the request, if any.
    pub diagnostic: Option<u16>,
    /// The matches in all.
    pub total: usize,
    /// Records sent.
    pub records: usize,
    /// Instances sent as a surrogate diagnostic.
    pub unavailable: Vec<i64>,
    /// Why storage or the index failed, for the log only.
    pub failure: Option<String>,
}

/// The response to `arguments`, read from `catalog` and `index`.
#[must_use]
pub fn respond(
    settings: &SruSettings,
    catalog: &dyn OaiCatalog,
    index: &dyn FieldSearch,
    arguments: &[(String, String)],
) -> SruAnswer {
    let parsed = parse(arguments, settings);
    let envelope = Envelope::of(parsed.version);
    let mut answer = SruAnswer {
        xml: String::new(),
        operation: if parsed.explain {
            "explain"
        } else {
            "searchRetrieve"
        },
        version: parsed.version,
        diagnostic: None,
        total: 0,
        records: 0,
        unavailable: Vec::new(),
        failure: None,
    };
    let body = match parsed.operation {
        Ok(Operation::Explain) => Ok(explain(settings, envelope)),
        Ok(Operation::SearchRetrieve(asked)) => {
            search(catalog, index, &asked, envelope, &mut answer)
        }
        Err(diagnostic) => Err(diagnostic),
    };
    let (name, inner) = match (parsed.explain, body) {
        (true, Ok(record)) => ("explainResponse", record),
        (true, Err(diagnostic)) => {
            answer.diagnostic = Some(diagnostic.code);
            ("explainResponse", envelope.diagnostics(&diagnostic))
        }
        (false, Ok(records)) => ("searchRetrieveResponse", records),
        (false, Err(diagnostic)) => {
            answer.diagnostic = Some(diagnostic.code);
            let total = if diagnostic.code == 61 {
                answer.total
            } else {
                0
            };
            (
                "searchRetrieveResponse",
                format!(
                    "{}{}",
                    envelope.element("numberOfRecords", &total.to_string()),
                    envelope.diagnostics(&diagnostic)
                ),
            )
        }
    };
    answer.xml = envelope.document(name, &inner);
    answer
}

/// The names of one version's response.
#[derive(Debug, Clone, Copy)]
struct Envelope {
    version: Version,
    prefix: &'static str,
    namespace: &'static str,
    diagnostic_namespace: &'static str,
    packing: &'static str,
}

impl Envelope {
    const fn of(version: Version) -> Self {
        match version {
            Version::V1_1 | Version::V1_2 => Self {
                version,
                prefix: "zs",
                namespace: "http://www.loc.gov/zing/srw/",
                diagnostic_namespace: "http://www.loc.gov/zing/srw/diagnostic/",
                packing: "recordPacking",
            },
            Version::V2_0 => Self {
                version,
                prefix: "sru",
                namespace: "http://docs.oasis-open.org/ns/search-ws/sruResponse",
                diagnostic_namespace: "http://docs.oasis-open.org/ns/search-ws/diagnostic",
                packing: "recordXMLEscaping",
            },
        }
    }

    fn document(self, name: &str, inner: &str) -> String {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<{p}:{name} xmlns:{p}=\"{ns}\">{}{inner}</{p}:{name}>\n",
            self.element("version", self.version.name()),
            p = self.prefix,
            ns = self.namespace,
        )
    }

    fn element(self, name: &str, text: &str) -> String {
        format!("<{p}:{name}>{}</{p}:{name}>", escape(text), p = self.prefix)
    }

    fn diagnostic(self, diagnostic: &Diagnostic) -> String {
        let mut xml = format!(
            "<diag:diagnostic xmlns:diag=\"{}\"><diag:uri>info:srw/diagnostic/1/{}</diag:uri>",
            self.diagnostic_namespace, diagnostic.code
        );
        if let Some(details) = &diagnostic.details {
            let _written = write!(xml, "<diag:details>{}</diag:details>", escape(details));
        }
        let _written = write!(
            xml,
            "<diag:message>{}</diag:message></diag:diagnostic>",
            diagnostic.message()
        );
        xml
    }

    fn diagnostics(self, diagnostic: &Diagnostic) -> String {
        format!(
            "<{p}:diagnostics>{}</{p}:diagnostics>",
            self.diagnostic(diagnostic),
            p = self.prefix
        )
    }

    /// One record; `position` is left out of the explain record.
    fn record(self, schema: &str, packing: Packing, data: &str, position: Option<usize>) -> String {
        let data = match packing {
            Packing::Xml => data.to_owned(),
            Packing::String => escape(data),
        };
        let mut xml = format!(
            "<{p}:record>{}{}<{p}:recordData>{data}</{p}:recordData>",
            self.element("recordSchema", schema),
            self.element(self.packing, packing.name()),
            p = self.prefix
        );
        if let Some(position) = position {
            xml.push_str(&self.element("recordPosition", &position.to_string()));
        }
        let _written = write!(xml, "</{}:record>", self.prefix);
        xml
    }
}

/// The page of records of `asked` (items 13 to 16).
fn search(
    catalog: &dyn OaiCatalog,
    index: &dyn FieldSearch,
    asked: &SearchRetrieve,
    envelope: Envelope,
    answer: &mut SruAnswer,
) -> Result<String, Diagnostic> {
    let offset = asked.start - 1;
    let found = index
        .search_fields(&asked.query, SearchScope::Public, offset, asked.maximum)
        .map_err(|error| failed(answer, error.to_string()))?;
    answer.total = found.total;
    if found.total > 0 && offset >= found.total && asked.maximum > 0 {
        return Err(Diagnostic::bare(61));
    }
    let mut records = String::new();
    for (place, hit) in found.hits.iter().enumerate() {
        let position = asked.start + place;
        let instance = hit.instance;
        let record = match catalog.header(instance) {
            Ok(Some(header)) if !header.hidden => {
                match marc_record(catalog, instance, &Limits::default()) {
                    Ok(record) => record.and_then(|marc| data(&marc, asked.schema)),
                    Err(RecordExportError::Read(error)) => {
                        return Err(failed(answer, error.to_string()));
                    }
                    Err(RecordExportError::Failure(_)) => None,
                }
            }
            Ok(_) => None,
            Err(error) => return Err(failed(answer, error.to_string())),
        };
        let written = if let Some(data) = record {
            envelope.record(asked.schema.name(), asked.packing, &data, Some(position))
        } else {
            answer.unavailable.push(instance.0);
            envelope.record(
                DIAGNOSTIC_SCHEMA,
                asked.packing,
                &envelope.diagnostic(&Diagnostic::bare(64)),
                Some(position),
            )
        };
        records.push_str(&written);
        answer.records += 1;
    }
    let mut xml = envelope.element("numberOfRecords", &found.total.to_string());
    if !records.is_empty() {
        let _written = write!(
            xml,
            "<{p}:records>{records}</{p}:records>",
            p = envelope.prefix
        );
    }
    let next = asked.start + found.hits.len();
    if !found.hits.is_empty() && next <= found.total {
        xml.push_str(&envelope.element("nextRecordPosition", &next.to_string()));
    }
    Ok(xml)
}

fn failed(answer: &mut SruAnswer, cause: String) -> Diagnostic {
    answer.failure = Some(cause);
    Diagnostic::bare(1)
}

/// The record in `schema`; `None` when it cannot be written.
fn data(marc: &Record, schema: RecordSchema) -> Option<String> {
    match schema {
        RecordSchema::MarcXml => marcxml_element(marc),
        RecordSchema::DublinCore => {
            let mut xml = String::from(
                "<srw_dc:dc xmlns:srw_dc=\"info:srw/schema/1/dc-schema\" \
                 xmlns:dc=\"http://purl.org/dc/elements/1.1/\">",
            );
            for (element, value) in dublin_core(marc) {
                let _written = write!(xml, "<dc:{0}>{1}</dc:{0}>", element.name(), escape(&value));
            }
            xml.push_str("</srw_dc:dc>");
            Some(xml)
        }
    }
}

/// The explain record (item 18).
fn explain(settings: &SruSettings, envelope: Envelope) -> String {
    let mut zeerex = format!(
        "<explain xmlns=\"{ZEEREX}\"><serverInfo protocol=\"SRU\" version=\"{}\">\
         <host>{}</host><port>{}</port><database>{}</database></serverInfo>\
         <databaseInfo><title>{}</title></databaseInfo><indexInfo>",
        envelope.version.name(),
        escape(&settings.host),
        settings.port,
        escape(&settings.database),
        escape(&settings.title),
    );
    for (name, identifier) in CONTEXT_SETS {
        let _written = write!(zeerex, "<set name=\"{name}\" identifier=\"{identifier}\"/>");
    }
    for (set, name, _) in INDEXES {
        let _written = write!(
            zeerex,
            "<index><title>{set}.{name}</title><map><name set=\"{set}\">{name}</name></map></index>"
        );
    }
    zeerex.push_str("</indexInfo><schemaInfo>");
    for (schema, title) in [
        (RecordSchema::MarcXml, "MARCXML"),
        (RecordSchema::DublinCore, "Dublin Core"),
    ] {
        let _written = write!(
            zeerex,
            "<schema identifier=\"{}\" name=\"{}\"><title>{title}</title></schema>",
            schema.identifier(),
            schema.name()
        );
    }
    let _written = write!(
        zeerex,
        "</schemaInfo><configInfo><default type=\"numberOfRecords\">{}</default>\
         <setting type=\"maximumRecords\">{}</setting></configInfo></explain>",
        settings.default_records, settings.max_records
    );
    envelope.record(ZEEREX, Packing::Xml, &zeerex, None)
}
