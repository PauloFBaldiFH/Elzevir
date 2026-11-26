// SPDX-License-Identifier: GPL-3.0-or-later
//! Draft sources at the edge: the SRU client behind the application's
//! [`DraftSource`] port (docs/specs/catalogo/fontes-de-rascunho.md). The HTTP transport
//! is a port of its own, so this crate holds the protocol and nothing that talks to the
//! network; the binary gives it the TLS client.

pub mod ber;
mod cql;
mod response;
pub mod z3950;

use std::collections::HashMap;
use std::time::Duration;

use elzevir_app::drafts::{
    DRAFTS_PER_PAGE, Draft, DraftField, DraftQuery, DraftSource, DraftSummary, MAX_DRAFT_PAGE,
    SourceOutcome, SourceResult,
};

pub use cql::cql;
pub use response::{MAX_RESPONSE_BYTES, ParsedResponse, parse};

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

/// The institution's SRU servers, asked together.
pub struct SruSources<T> {
    servers: Vec<SruServer>,
    transport: T,
}

impl<T: Transport> SruSources<T> {
    /// The sources of `servers`, in priority order, over `transport`.
    #[must_use]
    pub const fn new(servers: Vec<SruServer>, transport: T) -> Self {
        Self { servers, transport }
    }

    /// The transport the sources talk through.
    #[must_use]
    pub const fn transport(&self) -> &T {
        &self.transport
    }

    /// The searchRetrieve URL of page `page` of `query` on `server` (spec item 11).
    #[must_use]
    pub fn url(server: &SruServer, query: &DraftQuery, page: u32) -> String {
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
            cql::encode(&cql(query, &server.fields)),
            cql::encode(&server.record_schema),
        )
    }

    fn ask(&self, server: &SruServer, query: &DraftQuery, page: u32) -> SourceOutcome {
        let answer = match self
            .transport
            .get(&Self::url(server, query, page), server.timeout)
        {
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
            }) => {
                let origin = format!("sru:{}", server.name);
                let mut skipped = skipped;
                let drafts = records
                    .into_iter()
                    .filter_map(|record| {
                        let summary = DraftSummary::of(&record);
                        if summary.is_none() {
                            skipped = skipped.saturating_add(1);
                        }
                        summary.map(|summary| Draft {
                            origin: origin.clone(),
                            record,
                            summary,
                        })
                    })
                    .collect();
                SourceOutcome::Found {
                    total,
                    drafts,
                    skipped,
                }
            }
            Err(problem) => SourceOutcome::Refused(problem.to_string()),
        }
    }
}

impl<T: Transport> DraftSource for SruSources<T> {
    fn search(&self, query: &DraftQuery, page: u32) -> Vec<SourceResult> {
        // One thread per server: a slow one never holds the others (spec item 4).
        std::thread::scope(|scope| {
            let asked: Vec<_> = self
                .servers
                .iter()
                .map(|server| (server, scope.spawn(move || self.ask(server, query, page))))
                .collect();
            asked
                .into_iter()
                .map(|(server, handle)| SourceResult {
                    source: server.name.clone(),
                    // A panicking parser would be a bug; the server then counts as silent.
                    outcome: handle.join().unwrap_or(SourceOutcome::NoResponse),
                })
                .collect()
        })
    }

    fn sources(&self) -> Vec<String> {
        self.servers
            .iter()
            .map(|server| server.name.clone())
            .collect()
    }

    fn search_one(&self, index: usize, query: &DraftQuery, page: u32) -> Option<SourceResult> {
        let server = self.servers.get(index)?;
        Some(SourceResult {
            source: server.name.clone(),
            outcome: self.ask(server, query, page),
        })
    }
}
