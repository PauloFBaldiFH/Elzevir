// SPDX-License-Identifier: GPL-3.0-or-later
//! `elzevir draft-search`: asks the configured draft sources from the command line, so a
//! library can check its servers before cataloguing (docs/specs/catalogo/fontes-de-rascunho.md,
//! item 23), and the HTTP transport and TCP connections the sources talk through.

use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use clap::Args;
use elzevir_app::authority_sources::{AuthorityDraft, AuthorityQuery, AuthoritySource};
use elzevir_app::drafts::{DraftQuery, DraftSource, SourceOutcome, SourceResult, TypedQuery};
use elzevir_domain::catalog::SubjectKind;
use elzevir_sources::{
    Dialer, DraftServer, DraftSources, HttpAnswer, MAX_RESPONSE_BYTES, MAX_TIMEOUT, Transport,
    TransportError,
};

use crate::config;
use crate::http_client::agent;
use crate::logging::{self, Event};

/// `elzevir draft-search`.
#[derive(Args)]
pub(crate) struct DraftSearchArgs {
    /// Institution data directory, whose `elzevir.toml` lists the servers.
    #[arg(long)]
    data_dir: PathBuf,
    /// ISBN.
    #[arg(long, default_value = "")]
    isbn: String,
    /// ISSN.
    #[arg(long, default_value = "")]
    issn: String,
    /// Title words.
    #[arg(long, default_value = "")]
    title: String,
    /// Author words.
    #[arg(long, default_value = "")]
    author: String,
    /// Words in any field.
    #[arg(long, default_value = "")]
    any: String,
    /// Page of 20 records, from 1.
    #[arg(long, default_value_t = 1)]
    page: u32,
}

/// Asks every enabled server and prints what each one answered.
pub(crate) fn run(args: &DraftSearchArgs) -> Result<(), String> {
    let query = DraftQuery::new(&TypedQuery {
        isbn: &args.isbn,
        issn: &args.issn,
        title: &args.title,
        author: &args.author,
        any: &args.any,
    })
    .map_err(|e| e.to_string())?;
    let servers = config::load(&args.data_dir)?.draft_sources;
    if servers.is_empty() {
        return Err("no draft source is configured in elzevir.toml".to_owned());
    }
    let sources = DraftSources::new(servers, HttpTransport::new()?, TcpDialer);
    for result in sources.search(&query, args.page) {
        match result.outcome {
            SourceOutcome::Found {
                total,
                drafts,
                skipped,
            } => {
                println!("{}: {total} found", result.source);
                for draft in drafts {
                    let summary = draft.summary;
                    let parts: Vec<&str> = [
                        summary.author.as_deref(),
                        summary.edition.as_deref(),
                        summary.publication.as_deref(),
                    ]
                    .into_iter()
                    .flatten()
                    .collect();
                    if parts.is_empty() {
                        println!("  {}", summary.title);
                    } else {
                        println!("  {} | {}", summary.title, parts.join(" | "));
                    }
                    if !summary.isbns.is_empty() {
                        println!("    ISBN {}", summary.isbns.join(", "));
                    }
                }
                if skipped > 0 {
                    println!("  not readable: {skipped}");
                }
            }
            SourceOutcome::NoResponse => println!("{}: no response", result.source),
            SourceOutcome::Refused(message) => {
                println!("{}: refused: {message}", result.source);
            }
        }
    }
    Ok(())
}

/// `elzevir authority-search` (fontes-de-autoridade.md, item 15).
#[derive(Args)]
pub(crate) struct AuthoritySearchArgs {
    /// Institution data directory, whose `elzevir.toml` lists the servers.
    #[arg(long)]
    data_dir: PathBuf,
    /// The name or heading searched.
    #[arg(long)]
    name: String,
    /// person, family, organization, meeting, title, topic, place, genre-form or period.
    #[arg(long)]
    class: String,
    /// Page of 20 records, from 1.
    #[arg(long, default_value_t = 1)]
    page: u32,
}

/// The classes of the authority panel by their command-line names.
const CLASSES: [(&str, SubjectKind); 9] = [
    ("person", SubjectKind::Person),
    ("family", SubjectKind::Family),
    ("organization", SubjectKind::Organization),
    ("meeting", SubjectKind::Meeting),
    ("title", SubjectKind::Title),
    ("topic", SubjectKind::Topic),
    ("place", SubjectKind::Place),
    ("genre-form", SubjectKind::GenreForm),
    ("period", SubjectKind::Temporal),
];

/// Asks every enabled authority server and prints what each one answered.
pub(crate) fn run_authorities(args: &AuthoritySearchArgs) -> Result<(), String> {
    let kind = CLASSES
        .iter()
        .find(|(name, _)| *name == args.class)
        .map(|(_, kind)| *kind)
        .ok_or_else(|| format!("unknown class {}", args.class))?;
    let query = AuthorityQuery::new(&args.name, kind).map_err(|e| e.to_string())?;
    let servers = config::load(&args.data_dir)?.authority_sources;
    if servers.is_empty() {
        return Err("no authority source is configured in elzevir.toml".to_owned());
    }
    let sources = DraftSources::new(servers, HttpTransport::new()?, TcpDialer);
    for result in sources.search_authorities(&query, args.page) {
        match result.outcome {
            SourceOutcome::Found {
                total,
                drafts,
                skipped,
            } => {
                println!("{}: {total} found", result.source);
                for draft in drafts {
                    let heading = draft.heading;
                    let class = CLASSES
                        .iter()
                        .find(|(_, kind)| *kind == heading.kind)
                        .map_or("", |(name, _)| name);
                    match heading.identifier {
                        Some(identifier) => {
                            println!("  {} | {class} | {identifier}", heading.heading);
                        }
                        None => println!("  {} | {class}", heading.heading),
                    }
                    if !heading.variants.is_empty() {
                        println!("    see from: {}", heading.variants.join("; "));
                    }
                }
                if skipped > 0 {
                    println!("  not readable: {skipped}");
                }
            }
            SourceOutcome::NoResponse => println!("{}: no response", result.source),
            SourceOutcome::Refused(message) => {
                println!("{}: refused: {message}", result.source);
            }
        }
    }
    Ok(())
}

/// Plain GETs with the system's certificates; SRU servers may still be plain `http`.
pub(crate) struct HttpTransport {
    agent: ureq::Agent,
}

impl HttpTransport {
    /// The transport, or why TLS cannot be set up.
    pub(crate) fn new() -> Result<Self, String> {
        Ok(Self {
            agent: agent(3, MAX_TIMEOUT, false)?,
        })
    }
}

impl Transport for HttpTransport {
    fn get(&self, url: &str, timeout: Duration) -> Result<HttpAnswer, TransportError> {
        let mut response = self
            .agent
            .get(url)
            .config()
            .timeout_global(Some(timeout))
            .build()
            .call()
            .map_err(|_| TransportError::NoResponse)?;
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .with_config()
            .limit(u64::try_from(MAX_RESPONSE_BYTES).unwrap_or(u64::MAX))
            .read_to_vec()
            .map_err(|error| match error {
                ureq::Error::BodyExceedsLimit(_) => TransportError::TooLarge,
                _ => TransportError::NoResponse,
            })?;
        Ok(HttpAnswer { status, body })
    }
}

/// Plain TCP connections to Z39.50 servers, without TLS (cliente-z3950.md, item 24).
pub(crate) struct TcpDialer;

/// A TCP connection whose every read and write must end before the conversation's
/// deadline.
pub(crate) struct DeadlineStream {
    stream: TcpStream,
    deadline: Instant,
}

/// The time left before `deadline`, or a time-out error once it has passed.
fn remaining(deadline: Instant) -> io::Result<Duration> {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        Err(io::ErrorKind::TimedOut.into())
    } else {
        Ok(left)
    }
}

impl Dialer for TcpDialer {
    type Stream = DeadlineStream;

    fn dial(&self, host: &str, port: u16, deadline: Instant) -> io::Result<DeadlineStream> {
        let mut last = io::Error::from(io::ErrorKind::NotFound);
        for address in (host, port).to_socket_addrs()? {
            match TcpStream::connect_timeout(&address, remaining(deadline)?) {
                Ok(stream) => return Ok(DeadlineStream { stream, deadline }),
                Err(error) => last = error,
            }
        }
        Err(last)
    }
}

impl Read for DeadlineStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.stream
            .set_read_timeout(Some(remaining(self.deadline)?))?;
        self.stream.read(buffer)
    }
}

impl Write for DeadlineStream {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.stream
            .set_write_timeout(Some(remaining(self.deadline)?))?;
        self.stream.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

/// The sources the staff screen asks, each answer logged with the source's position,
/// outcome and time, never the query (docs/specs/catalogo/estacao-de-captura.md, item 15,
/// and fontes-de-autoridade.md, item 24).
pub(crate) struct LoggedSources {
    sources: DraftSources<HttpTransport, TcpDialer>,
    instance: String,
}

/// The screen's sources: the bibliographic ones of the capture station and the authority
/// ones of the "+ Authority" form, each `None` without servers, so the screen offers no
/// search (fontes-de-rascunho.md, item 18; fontes-de-autoridade.md, item 16).
pub(crate) struct ScreenSources {
    pub(crate) drafts: Option<LoggedSources>,
    pub(crate) authorities: Option<LoggedSources>,
}

impl ScreenSources {
    /// The sources of `drafts` and `authorities`, logged under the server's `instance`.
    pub(crate) fn new(
        drafts: Vec<DraftServer>,
        authorities: Vec<DraftServer>,
        instance: &str,
    ) -> Result<Self, String> {
        Ok(Self {
            drafts: LoggedSources::new(drafts, instance)?,
            authorities: LoggedSources::new(authorities, instance)?,
        })
    }
}

impl LoggedSources {
    /// The sources of `servers`, logged under the server's `instance`; `None` without
    /// servers.
    pub(crate) fn new(servers: Vec<DraftServer>, instance: &str) -> Result<Option<Self>, String> {
        if servers.is_empty() {
            return Ok(None);
        }
        Ok(Some(Self {
            sources: DraftSources::new(servers, HttpTransport::new()?, TcpDialer),
            instance: instance.to_owned(),
        }))
    }

    /// Logs the answer of source `index`, asked at `started`, as `event`.
    fn log<R>(
        &self,
        index: usize,
        outcome: &SourceOutcome<R>,
        started: Instant,
        event: fn(u64, &'static str, u64) -> Event,
    ) {
        let position = u64::try_from(index).map_or(u64::MAX, |index| index.saturating_add(1));
        let outcome = match outcome {
            SourceOutcome::Found { .. } => "found",
            SourceOutcome::NoResponse => "no_response",
            SourceOutcome::Refused(_) => "refused",
        };
        let millis = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        logging::emit(&self.instance, event(position, outcome, millis));
    }
}

impl DraftSource for LoggedSources {
    fn search(&self, query: &DraftQuery, page: u32) -> Vec<SourceResult> {
        self.sources.search(query, page)
    }

    fn sources(&self) -> Vec<String> {
        self.sources.sources()
    }

    fn search_one(&self, index: usize, query: &DraftQuery, page: u32) -> Option<SourceResult> {
        let started = Instant::now();
        let result = self.sources.search_one(index, query, page)?;
        self.log(
            index,
            &result.outcome,
            started,
            |position, outcome, millis| Event::DraftSourceAnswered {
                position,
                outcome,
                millis,
            },
        );
        Some(result)
    }
}

impl AuthoritySource for LoggedSources {
    fn search_authorities(
        &self,
        query: &AuthorityQuery,
        page: u32,
    ) -> Vec<SourceResult<AuthorityDraft>> {
        self.sources.search_authorities(query, page)
    }

    fn authority_sources(&self) -> Vec<String> {
        self.sources.authority_sources()
    }

    fn search_one_authority(
        &self,
        index: usize,
        query: &AuthorityQuery,
        page: u32,
    ) -> Option<SourceResult<AuthorityDraft>> {
        let started = Instant::now();
        let result = self.sources.search_one_authority(index, query, page)?;
        self.log(
            index,
            &result.outcome,
            started,
            |position, outcome, millis| Event::AuthoritySourceAnswered {
                position,
                outcome,
                millis,
            },
        );
        Some(result)
    }
}
