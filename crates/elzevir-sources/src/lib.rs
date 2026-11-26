// SPDX-License-Identifier: GPL-3.0-or-later
//! Draft sources at the edge: the SRU and Z39.50 clients behind the application's
//! [`DraftSource`] port (docs/specs/catalogo/fontes-de-rascunho.md and cliente-z3950.md).
//! The HTTP transport and the TCP connection are ports of their own, so this crate holds
//! the protocols and nothing that talks to the network; the binary gives it the clients.

pub mod ber;
mod cql;
mod response;
pub mod z3950;
mod z3950_client;

use std::collections::HashMap;
use std::time::Duration;

use elzevir_app::authority_sources::{AuthorityDraft, AuthorityQuery, AuthoritySource};
use elzevir_app::drafts::{
    DRAFTS_PER_PAGE, Draft, DraftField, DraftQuery, DraftSource, DraftSummary, MAX_DRAFT_PAGE,
    SourceOutcome, SourceResult,
};
use elzevir_marc::Record;

pub use cql::cql;
pub use response::{MAX_RESPONSE_BYTES, ParsedResponse, parse};
pub use z3950_client::{DEFAULT_Z3950_PORT, Dialer, RecordCharset, Z3950Server, to_utf8};

/// Most characters of a server's name.
pub const MAX_NAME_CHARS: usize = 80;
/// Longest time limit a server may have.
pub const MAX_TIMEOUT: Duration = Duration::from_mins(1);
/// Time limit of a server that sets none.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// The SRU versions a server may speak.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SruVersion {
    /// SRU 1.2, the default.
    #[default]
    V1_2,
    /// SRU 2.0.
    V2_0,
}

/// One SRU server of the institution, from `[[draft_sources]]` in `elzevir.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SruServer {
    /// Name shown in the origin, `sru:<name>`.
    pub name: String,
    /// Base URL, `http` or `https`, without a query string.
    pub url: String,
    /// SRU version.
    pub version: SruVersion,
    /// Record schema asked for, `marcxml` unless set.
    pub record_schema: String,
    /// How long the server has to answer.
    pub timeout: Duration,
    /// The server's CQL index of each query field (Koha `sru_fields`).
    pub fields: HashMap<DraftField, String>,
}

/// One draft server of the institution, SRU or Z39.50, in the configured order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DraftServer {
    /// An SRU server.
    Sru(SruServer),
    /// A Z39.50 server.
    Z3950(Z3950Server),
}

impl DraftServer {
    /// The server's name.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Sru(server) => &server.name,
            Self::Z3950(server) => &server.name,
        }
    }
}

/// What the transport got back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpAnswer {
    /// HTTP status.
    pub status: u16,
    /// Body, at most [`MAX_RESPONSE_BYTES`].
    pub body: Vec<u8>,
}

/// Why the transport got nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TransportError {
    /// The time limit ran out, or the connection failed.
    #[error("no response")]
    NoResponse,
    /// The body is longer than [`MAX_RESPONSE_BYTES`].
    #[error("response too large")]
    TooLarge,
}

/// A plain HTTP GET with a time limit and a body limit.
pub trait Transport: Send + Sync {
    /// Fetches `url`.
    ///
    /// # Errors
    ///
    /// When nothing usable came back in time.
    fn get(&self, url: &str, timeout: Duration) -> Result<HttpAnswer, TransportError>;
}

/// The institution's draft servers, SRU and Z39.50, asked together.
pub struct DraftSources<T, D> {
    servers: Vec<DraftServer>,
    transport: T,
    dialer: D,
}

impl<T: Transport, D: Dialer> DraftSources<T, D> {
    /// The sources of `servers`, in priority order: SRU over `transport`, Z39.50 over
    /// connections from `dialer`.
    #[must_use]
    pub const fn new(servers: Vec<DraftServer>, transport: T, dialer: D) -> Self {
        Self {
            servers,
            transport,
            dialer,
        }
    }

    /// The transport the SRU sources talk through.
    #[must_use]
    pub const fn transport(&self) -> &T {
        &self.transport
    }

    /// The dialer the Z39.50 sources connect through.
    #[must_use]
    pub const fn dialer(&self) -> &D {
        &self.dialer
    }

    /// What `server` answers for page `page` of the filled `fields`, each record read by
    /// `read` with its origin.
    fn ask<R>(
        &self,
        server: &DraftServer,
        fields: &[(DraftField, &str)],
        page: u32,
        read: &(dyn Fn(Record, &str) -> Option<R> + Sync),
    ) -> SourceOutcome<R> {
        match server {
            DraftServer::Sru(server) => ask_sru(&self.transport, server, fields, page, read),
            DraftServer::Z3950(server) => {
                z3950_client::ask(&self.dialer, server, fields, page, read)
            }
        }
    }

    /// Every server asked at once, one thread each: a slow one never holds the others
    /// (spec item 4).
    fn ask_all<R: Send>(
        &self,
        fields: &[(DraftField, &str)],
        page: u32,
        read: &(dyn Fn(Record, &str) -> Option<R> + Sync),
    ) -> Vec<SourceResult<R>> {
        std::thread::scope(|scope| {
            let asked: Vec<_> = self
                .servers
                .iter()
                .map(|server| {
                    (
                        server,
                        scope.spawn(move || self.ask(server, fields, page, read)),
                    )
                })
                .collect();
            asked
                .into_iter()
                .map(|(server, handle)| SourceResult {
                    source: server.name().to_owned(),
                    // A panicking parser would be a bug; the server then counts as silent.
                    outcome: handle.join().unwrap_or(SourceOutcome::NoResponse),
                })
                .collect()
        })
    }

    /// Server `index` asked alone.
    fn ask_one<R>(
        &self,
        index: usize,
        fields: &[(DraftField, &str)],
        page: u32,
        read: &(dyn Fn(Record, &str) -> Option<R> + Sync),
    ) -> Option<SourceResult<R>> {
        let server = self.servers.get(index)?;
        Some(SourceResult {
            source: server.name().to_owned(),
            outcome: self.ask(server, fields, page, read),
        })
    }

    fn names(&self) -> Vec<String> {
        self.servers
            .iter()
            .map(|server| server.name().to_owned())
            .collect()
    }
}

/// The searchRetrieve URL of page `page` of `query` on `server` (spec item 11).
#[must_use]
pub fn sru_url(server: &SruServer, query: &DraftQuery, page: u32) -> String {
    sru_url_of(server, &query.fields().collect::<Vec<_>>(), page)
}

/// The searchRetrieve URL of page `page` of the filled `fields` on `server`.
fn sru_url_of(server: &SruServer, fields: &[(DraftField, &str)], page: u32) -> String {
    let page = page.clamp(1, MAX_DRAFT_PAGE);
    let start = (page - 1) * DRAFTS_PER_PAGE + 1;
    let (version, packing) = match server.version {
        SruVersion::V1_2 => ("1.2", "recordPacking=xml"),
        SruVersion::V2_0 => ("2.0", "recordXMLEscaping=xml"),
    };
    format!(
        "{}?operation=searchRetrieve&version={version}&query={}&startRecord={start}\
         &maximumRecords={DRAFTS_PER_PAGE}&recordSchema={}&{packing}",
        server.url,
        cql::encode(&cql::cql_of(fields.iter().copied(), &server.fields)),
        cql::encode(&server.record_schema),
    )
}

/// What the SRU `server` answers.
fn ask_sru<R>(
    transport: &impl Transport,
    server: &SruServer,
    fields: &[(DraftField, &str)],
    page: u32,
    read: &dyn Fn(Record, &str) -> Option<R>,
) -> SourceOutcome<R> {
    let answer = match transport.get(&sru_url_of(server, fields, page), server.timeout) {
        Ok(answer) => answer,
        Err(TransportError::NoResponse) => return SourceOutcome::NoResponse,
        Err(TransportError::TooLarge) => {
            return SourceOutcome::Refused("response too large".to_owned());
        }
    };
    if !(200..300).contains(&answer.status) {
        return SourceOutcome::Refused(format!("HTTP {}", answer.status));
    }
    match parse(&answer.body) {
        Ok(ParsedResponse::Diagnostic(message)) => SourceOutcome::Refused(message),
        Ok(ParsedResponse::Records {
            total,
            records,
            skipped,
        }) => found(
            &format!("sru:{}", server.name),
            total,
            records,
            skipped,
            read,
        ),
        Err(problem) => SourceOutcome::Refused(problem.to_string()),
    }
}

/// The drafts `read` makes of `records`, each with `origin`; a record it cannot read is
/// counted in `skipped`.
fn found<R>(
    origin: &str,
    total: u64,
    records: Vec<Record>,
    skipped: u32,
    read: &dyn Fn(Record, &str) -> Option<R>,
) -> SourceOutcome<R> {
    let mut skipped = skipped;
    let drafts = records
        .into_iter()
        .filter_map(|record| {
            let draft = read(record, origin);
            if draft.is_none() {
                skipped = skipped.saturating_add(1);
            }
            draft
        })
        .collect();
    SourceOutcome::Found {
        total,
        drafts,
        skipped,
    }
}

/// A bibliographic draft of `record`, when the list can summarize it.
fn bibliographic(record: Record, origin: &str) -> Option<Draft> {
    DraftSummary::of(&record).map(|summary| Draft {
        origin: origin.to_owned(),
        record,
        summary,
    })
}

/// An authority draft of `record`, when it is an authority record with a heading.
#[expect(
    clippy::needless_pass_by_value,
    reason = "the same reader type as `bibliographic`, which keeps the record"
)]
fn authority(record: Record, origin: &str) -> Option<AuthorityDraft> {
    AuthorityDraft::of(&record, origin)
}

impl<T: Transport, D: Dialer> DraftSource for DraftSources<T, D> {
    fn search(&self, query: &DraftQuery, page: u32) -> Vec<SourceResult> {
        self.ask_all(&query.fields().collect::<Vec<_>>(), page, &bibliographic)
    }

    fn sources(&self) -> Vec<String> {
        self.names()
    }

    fn search_one(&self, index: usize, query: &DraftQuery, page: u32) -> Option<SourceResult> {
        self.ask_one(
            index,
            &query.fields().collect::<Vec<_>>(),
            page,
            &bibliographic,
        )
    }
}

impl<T: Transport, D: Dialer> AuthoritySource for DraftSources<T, D> {
    fn search_authorities(
        &self,
        query: &AuthorityQuery,
        page: u32,
    ) -> Vec<SourceResult<AuthorityDraft>> {
        self.ask_all(&[(query.field(), query.name())], page, &authority)
    }

    fn authority_sources(&self) -> Vec<String> {
        self.names()
    }

    fn search_one_authority(
        &self,
        index: usize,
        query: &AuthorityQuery,
        page: u32,
    ) -> Option<SourceResult<AuthorityDraft>> {
        self.ask_one(index, &[(query.field(), query.name())], page, &authority)
    }
}
