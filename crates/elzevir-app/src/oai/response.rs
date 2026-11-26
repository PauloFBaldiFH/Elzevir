// SPDX-License-Identifier: GPL-3.0-or-later
//! The OAI-PMH response of a request (docs/specs/opac/oai-pmh.md, items 4 and 6 to 18).
//!
//! Koha: `Koha::OAI::Server::Identify`, `ListMetadataFormats`, `ListSets`, `GetRecord`,
//! `ListBase`, `Record` and `DeletedRecord`.

use std::fmt::Write as _;

use elzevir_marc::Record;

use super::dublin_core::dublin_core;
use super::request::{ListState, MetadataFormat, OaiError, OaiErrorCode, Request, parse};
use super::{Datestamp, OaiCatalog, OaiHeader, OaiRecordError, OaiSettings, OaiUnavailable};

/// The earliest datestamp of a repository with no records, as Koha answers.
const NO_RECORDS: &str = "0001-01-01T00:00:00Z";

/// A response and what the log says of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    /// The XML document.
    pub xml: String,
    /// The verb, when it was one.
    pub verb: Option<&'static str>,
    /// The format asked for.
    pub format: Option<&'static str>,
    /// `answered`, or the first error code.
    pub outcome: &'static str,
    /// How many records or headers went out.
    pub records: usize,
    /// Records whose MARC could not be written, sent as deleted.
    pub broken: Vec<i64>,
}

/// The response to `arguments`, at `now`.
///
/// # Errors
///
/// [`OaiUnavailable`] when storage fails.
pub fn respond(
    settings: &OaiSettings,
    catalog: &dyn OaiCatalog,
    arguments: &[(String, String)],
    now: jiff::Timestamp,
) -> Result<Answer, OaiUnavailable> {
    let mut answer = Answer {
        xml: String::new(),
        verb: None,
        format: None,
        outcome: "answered",
        records: 0,
        broken: Vec::new(),
    };
    let request = match parse(arguments) {
        Ok(request) => request,
        Err(errors) => {
            let echo = !errors
                .iter()
                .any(|(code, _)| matches!(code, OaiErrorCode::BadVerb | OaiErrorCode::BadArgument));
            answer.outcome = errors.first().map_or("answered", |(code, _)| code.code());
            answer.xml = document(
                settings,
                now,
                echo.then_some(arguments),
                &errors_xml(&errors),
            );
            return Ok(answer);
        }
    };
    answer.verb = Some(request.verb().name());
    let body = match body(settings, catalog, &request, &mut answer) {
        Ok(body) => body,
        Err(Failure::Unavailable(error)) => return Err(error),
        Err(Failure::Oai(errors)) => {
            answer.outcome = errors.first().map_or("answered", |(code, _)| code.code());
            errors_xml(&errors)
        }
    };
    answer.xml = document(settings, now, Some(arguments), &body);
    Ok(answer)
}

/// Why a request has no verb element.
enum Failure {
    /// OAI errors, answered in the document.
    Oai(Vec<OaiError>),
    /// Storage failed: no document.
    Unavailable(OaiUnavailable),
}

impl From<Vec<OaiError>> for Failure {
    fn from(errors: Vec<OaiError>) -> Self {
        Self::Oai(errors)
    }
}

impl From<OaiUnavailable> for Failure {
    fn from(error: OaiUnavailable) -> Self {
        Self::Unavailable(error)
    }
}

/// The element of the verb, or the errors.
fn body(
    settings: &OaiSettings,
    catalog: &dyn OaiCatalog,
    request: &Request,
    answer: &mut Answer,
) -> Result<String, Failure> {
    Ok(match request {
        Request::Identify => identify(settings, catalog.earliest()?),
        Request::ListMetadataFormats { identifier } => {
            if let Some(identifier) = identifier {
                header_of(settings, catalog, identifier)?;
            }
            list_formats()
        }
        Request::ListSets => return Err(vec![no_sets()].into()),
        Request::GetRecord { identifier, format } => {
            answer.format = Some(format.prefix());
            let header = header_of(settings, catalog, identifier)?;
            answer.records = 1;
            let record = record(settings, catalog, &header, *format, &mut answer.broken)?;
            format!("<GetRecord>{record}</GetRecord>")
        }
        Request::List {
            records,
            state,
            set,
            resumed,
        } => {
            answer.format = Some(state.format.prefix());
            if *set {
                return Err(vec![no_sets()].into());
            }
            list(settings, catalog, *records, state, *resumed, answer)?
        }
    })
}

fn no_sets() -> OaiError {
    (
        OaiErrorCode::NoSetHierarchy,
        "There are no OAI sets defined".to_owned(),
    )
}

/// The header an identifier names, or `idDoesNotExist`.
fn header_of(
    settings: &OaiSettings,
    catalog: &dyn OaiCatalog,
    identifier: &str,
) -> Result<OaiHeader, Failure> {
    let missing = || {
        vec![(
            OaiErrorCode::IdDoesNotExist,
            "There is no record with this identifier".to_owned(),
        )]
    };
    let instance = super::request::instance_of(identifier, &settings.repository_identifier)
        .ok_or_else(missing)?;
    Ok(catalog.header(instance)?.ok_or_else(missing)?)
}

fn identify(settings: &OaiSettings, earliest: Option<Datestamp>) -> String {
    let mut xml = String::from("<Identify>");
    element(&mut xml, "repositoryName", &settings.repository_name);
    element(&mut xml, "baseURL", &settings.base_url);
    element(&mut xml, "protocolVersion", "2.0");
    for email in &settings.admin_emails {
        element(&mut xml, "adminEmail", email);
    }
    let earliest =
        earliest.map_or_else(|| NO_RECORDS.to_owned(), |datestamp| datestamp.to_string());
    element(&mut xml, "earliestDatestamp", &earliest);
    element(&mut xml, "deletedRecord", "persistent");
    element(&mut xml, "granularity", "YYYY-MM-DDThh:mm:ssZ");
    xml.push_str("</Identify>");
    xml
}

fn list_formats() -> String {
    let mut xml = String::from("<ListMetadataFormats>");
    for format in MetadataFormat::ALL {
        xml.push_str("<metadataFormat>");
        element(&mut xml, "metadataPrefix", format.prefix());
        element(&mut xml, "schema", format.schema());
        element(&mut xml, "metadataNamespace", format.namespace());
        xml.push_str("</metadataFormat>");
    }
    xml.push_str("</ListMetadataFormats>");
    xml
}

/// One page of `ListIdentifiers` or `ListRecords`.
fn list(
    settings: &OaiSettings,
    catalog: &dyn OaiCatalog,
    records: bool,
    state: &ListState,
    resumed: bool,
    answer: &mut Answer,
) -> Result<String, Failure> {
    let max = settings.max_count.max(1);
    let mut headers = catalog.headers(
        state.next_id,
        state.from.as_ref(),
        state.until.as_ref(),
        max.saturating_add(1),
    )?;
    let next = (headers.len() > max).then(|| headers.split_off(max));
    if headers.is_empty() {
        return Err(vec![(
            OaiErrorCode::NoRecordsMatch,
            "No records match the request".to_owned(),
        )]
        .into());
    }
    let name = if records {
        "ListRecords"
    } else {
        "ListIdentifiers"
    };
    let mut xml = format!("<{name}>");
    for header in &headers {
        if records {
            xml.push_str(&record(
                settings,
                catalog,
                header,
                state.format,
                &mut answer.broken,
            )?);
        } else {
            xml.push_str(&header_xml(settings, header));
        }
    }
    answer.records = headers.len();
    let given = u64::try_from(headers.len()).unwrap_or(u64::MAX);
    match next.as_ref().and_then(|rest| rest.first()) {
        Some(first) => {
            let token = state.token(given, first.instance.0);
            let _written = write!(
                xml,
                "<resumptionToken cursor=\"{}\">{}</resumptionToken>",
                state.cursor,
                escape(&token)
            );
        }
        None if resumed => {
            let _written = write!(xml, "<resumptionToken cursor=\"{}\"/>", state.cursor);
        }
        None => {}
    }
    let _written = write!(xml, "</{name}>");
    Ok(xml)
}

/// A whole record; hidden or broken, only its deleted header.
fn record(
    settings: &OaiSettings,
    catalog: &dyn OaiCatalog,
    header: &OaiHeader,
    format: MetadataFormat,
    broken: &mut Vec<i64>,
) -> Result<String, Failure> {
    let deleted = || format!("<record>{}</record>", deleted_header(settings, header));
    if header.hidden {
        return Ok(deleted());
    }
    let marc = match super::marc(catalog, header.instance) {
        Ok(Some(marc)) => marc,
        Ok(None) => return Ok(deleted()),
        Err(OaiRecordError::Unavailable(error)) => return Err(error.into()),
        Err(OaiRecordError::Invalid(_)) => {
            broken.push(header.instance.0);
            return Ok(deleted());
        }
    };
    let Some(metadata) = metadata(&marc, format) else {
        broken.push(header.instance.0);
        return Ok(deleted());
    };
    Ok(format!(
        "<record>{}<metadata>{metadata}</metadata></record>",
        header_xml(settings, header)
    ))
}

fn metadata(marc: &Record, format: MetadataFormat) -> Option<String> {
    match format {
        MetadataFormat::OaiDc => Some(dc_xml(marc)),
        MetadataFormat::Marc21 | MetadataFormat::MarcXml => marcxml_element(marc),
    }
}

/// The MARCXML `<record>` element of `marc` alone, as Koha embeds `as_xml_record`;
/// `None` when the record cannot be written.
pub(crate) fn marcxml_element(marc: &Record) -> Option<String> {
    let mut document = String::new();
    marc.write_marcxml_document(&mut document).ok()?;
    document
        .split_once("?>\n")
        .map(|(_, element)| element.trim_end().to_owned())
}

fn dc_xml(marc: &Record) -> String {
    let mut xml = String::from(
        "<oai_dc:dc xmlns:oai_dc=\"http://www.openarchives.org/OAI/2.0/oai_dc/\" \
         xmlns:dc=\"http://purl.org/dc/elements/1.1/\" \
         xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" \
         xsi:schemaLocation=\"http://www.openarchives.org/OAI/2.0/oai_dc/ \
         http://www.openarchives.org/OAI/2.0/oai_dc.xsd\">",
    );
    for (element, value) in dublin_core(marc) {
        let _written = write!(xml, "<dc:{0}>{1}</dc:{0}>", element.name(), escape(&value));
    }
    xml.push_str("</oai_dc:dc>");
    xml
}

fn identifier(settings: &OaiSettings, header: &OaiHeader) -> String {
    format!("{}:{}", settings.repository_identifier, header.instance.0)
}

fn header_xml(settings: &OaiSettings, header: &OaiHeader) -> String {
    if header.hidden {
        return deleted_header(settings, header);
    }
    format!(
        "<header><identifier>{}</identifier><datestamp>{}</datestamp></header>",
        escape(&identifier(settings, header)),
        header.datestamp
    )
}

fn deleted_header(settings: &OaiSettings, header: &OaiHeader) -> String {
    format!(
        "<header status=\"deleted\"><identifier>{}</identifier><datestamp>{}</datestamp></header>",
        escape(&identifier(settings, header)),
        header.datestamp
    )
}

fn errors_xml(errors: &[OaiError]) -> String {
    let mut xml = String::new();
    for (code, message) in errors {
        let _written = write!(
            xml,
            "<error code=\"{}\">{}</error>",
            code.code(),
            escape(message)
        );
    }
    xml
}

/// The whole document around `body`; the request's arguments as attributes when `echo`.
fn document(
    settings: &OaiSettings,
    now: jiff::Timestamp,
    echo: Option<&[(String, String)]>,
    body: &str,
) -> String {
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <OAI-PMH xmlns=\"http://www.openarchives.org/OAI/2.0/\" \
         xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" \
         xsi:schemaLocation=\"http://www.openarchives.org/OAI/2.0/ \
         http://www.openarchives.org/OAI/2.0/OAI-PMH.xsd\">",
    );
    element(&mut xml, "responseDate", Datestamp::of(now).as_str());
    xml.push_str("<request");
    for (name, value) in echo.unwrap_or_default() {
        let _written = write!(xml, " {}=\"{}\"", escape(name), escape(value));
    }
    let _written = write!(xml, ">{}</request>", escape(&settings.base_url));
    xml.push_str(body);
    xml.push_str("</OAI-PMH>\n");
    xml
}

fn element(xml: &mut String, name: &str, value: &str) {
    let _written = write!(xml, "<{name}>{}</{name}>", escape(value));
}

/// Text and attribute values escaped; characters XML 1.0 forbids are dropped.
pub(crate) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' | '\n' | '\r' => out.push(character),
            control if control < ' ' || matches!(control, '\u{FFFE}' | '\u{FFFF}') => {}
            other => out.push(other),
        }
    }
    out
}
