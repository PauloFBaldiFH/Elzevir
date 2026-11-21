// SPDX-License-Identifier: GPL-3.0-or-later
//! `elzevir draft-search`: asks the configured draft sources from the command line, so a
//! library can check its servers before cataloguing (docs/specs/catalogo/fontes-de-rascunho.md,
//! item 23), and the HTTP transport the sources talk through.

use std::path::PathBuf;
use std::time::Duration;

use clap::Args;
use elzevir_app::drafts::{DraftQuery, DraftSource, SourceOutcome, TypedQuery};
use elzevir_sources::{
    HttpAnswer, MAX_RESPONSE_BYTES, MAX_TIMEOUT, SruSources, Transport, TransportError,
};

use crate::config;
use crate::http_client::agent;

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
    let sources = SruSources::new(servers, HttpTransport::new()?);
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
