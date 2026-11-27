// SPDX-License-Identifier: GPL-3.0-or-later
//! The SIP2 listener of `elzevir serve` (docs/specs/circulacao/sip2.md, items 16 and 17):
//! `[sip2]` in `elzevir.toml`, one thread per terminal connection, at most
//! [`MAX_CONNECTIONS`] at a time.

use std::io::{self, ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use elzevir_app::catalog::CatalogReader;
use elzevir_domain::account::DebtSettings;
use elzevir_domain::circulation::RenewalBase;
use elzevir_sip2::{Desk, Encoding, SipEvent, Terminal, Terminator, serve};
use elzevir_store::ElzevirStore;
use jiff::tz::TimeZone;
use serde::Deserialize;

use crate::logging;

/// Most terminal connections at a time; the next one is closed (spec, item 17).
pub(crate) const MAX_CONNECTIONS: usize = 64;
/// Inactivity that closes a connection when the configuration sets none (Koha:
/// `client_timeout`).
const DEFAULT_TIMEOUT: Duration = Duration::from_mins(5);
/// How often a waiting connection or the listener looks at the stop flag.
const POLL: Duration = Duration::from_millis(250);

/// `[sip2]` as written.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Sip2Section {
    listen: String,
    timeout_seconds: Option<u64>,
    currency: Option<String>,
    #[serde(default)]
    terminals: Vec<TerminalSection>,
}

/// One `[[sip2.terminals]]`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TerminalSection {
    login: String,
    unit: String,
    terminator: Option<String>,
    encoding: Option<String>,
}

/// `[sip2]`, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Sip2Settings {
    pub(crate) listen: SocketAddr,
    pub(crate) timeout: Duration,
    pub(crate) currency: Option<String>,
    pub(crate) terminals: Vec<TerminalSettings>,
}

/// A terminal, with its branch still by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TerminalSettings {
    pub(crate) login: String,
    pub(crate) unit: String,
    pub(crate) terminator: Terminator,
    pub(crate) encoding: Encoding,
}

impl Sip2Section {
    /// The settings, or what is wrong with them.
    pub(crate) fn settings(&self) -> Result<Sip2Settings, String> {
        let listen = self
            .listen
            .parse::<SocketAddr>()
            .map_err(|e| format!("listen: {e}"))?;
        let timeout = match self.timeout_seconds {
            None => DEFAULT_TIMEOUT,
            Some(0) => return Err("timeout_seconds must be more than zero".to_owned()),
            Some(seconds) => Duration::from_secs(seconds),
        };
        if let Some(currency) = &self.currency
            && (currency.is_empty() || currency.chars().count() > 3)
        {
            return Err("currency has one to three letters".to_owned());
        }
        let mut terminals: Vec<TerminalSettings> = Vec::with_capacity(self.terminals.len());
        for (index, terminal) in self.terminals.iter().enumerate() {
            let terminator = match terminal.terminator.as_deref() {
                None | Some("crlf") => Terminator::CrLf,
                Some("cr") => Terminator::Cr,
                Some(other) => {
                    return Err(format!(
                        "terminals[{index}]: terminator {other:?} is cr or crlf"
                    ));
                }
            };
            // Koha writes Latin-1 when the account sets no `encoding` (spec, item 16).
            let encoding = match terminal.encoding.as_deref() {
                None | Some("latin-1") => Encoding::Latin1,
                Some("utf-8") => Encoding::Utf8,
                Some(other) => {
                    return Err(format!(
                        "terminals[{index}]: encoding {other:?} is latin-1 or utf-8"
                    ));
                }
            };
            if terminal.login.trim().is_empty() || terminal.unit.trim().is_empty() {
                return Err(format!("terminals[{index}]: login and unit are required"));
            }
            if terminals.iter().any(|seen| seen.login == terminal.login) {
                return Err(format!(
                    "terminals[{index}]: the login {} is repeated",
                    terminal.login
                ));
            }
            terminals.push(TerminalSettings {
                login: terminal.login.clone(),
                unit: terminal.unit.trim().to_owned(),
                terminator,
                encoding,
            });
        }
        Ok(Sip2Settings {
            listen,
            timeout,
            currency: self.currency.clone(),
            terminals,
        })
    }
}

/// A running listener; [`Listener::stop`] ends it and its connections.
pub(crate) struct Listener {
    stop: Arc<AtomicBool>,
    active: Arc<AtomicUsize>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Listener {
    /// Stops accepting, closes the connections and waits for them, so that the store
    /// they hold can shut down.
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            // A panicking listener thread has nothing left to clean up.
            let _joined = thread.join().is_ok();
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.active.load(Ordering::SeqCst) > 0 && Instant::now() < deadline {
            std::thread::sleep(POLL);
        }
    }
}

/// Opens the listener when `[sip2]` is set, and says where.
pub(crate) fn open(
    settings: Option<&Sip2Settings>,
    store: &Arc<ElzevirStore>,
    time_zone: &TimeZone,
    renewal_base: RenewalBase,
    instance: &str,
) -> Result<Option<Listener>, String> {
    let Some(settings) = settings else {
        return Ok(None);
    };
    let (listener, address) = start(settings, store, time_zone.clone(), renewal_base, instance)?;
    println!("SIP2 on {address}");
    Ok(Some(listener))
}

/// Opens the listener of `settings` on `store`.
pub(crate) fn start(
    settings: &Sip2Settings,
    store: &Arc<ElzevirStore>,
    time_zone: TimeZone,
    renewal_base: RenewalBase,
    instance: &str,
) -> Result<(Listener, SocketAddr), String> {
    let branches = store.codes().map_err(|e| e.to_string())?.branches;
    let mut terminals = Vec::with_capacity(settings.terminals.len());
    for terminal in &settings.terminals {
        let branch = branches
            .iter()
            .find(|branch| branch.name == terminal.unit)
            .ok_or_else(|| format!("sip2: no unit is named {}", terminal.unit))?;
        terminals.push(Terminal {
            login: terminal.login.clone(),
            unit: branch.id,
            institution: branch.name.clone(),
            terminator: terminal.terminator,
            encoding: terminal.encoding,
        });
    }
    let instance = instance.to_owned();
    let desk = Desk {
        staff: Arc::clone(store) as _,
        patrons: Arc::clone(store) as _,
        accounts: Arc::clone(store) as _,
        loans: Arc::clone(store) as _,
        self_check: Arc::clone(store) as _,
        debt: DebtSettings::default(),
        renewal_base,
        currency: settings.currency.clone(),
        terminals,
        clock: Arc::new(move || jiff::Timestamp::now().to_zoned(time_zone.clone())),
        log: Arc::new(move |event: SipEvent| {
            logging::sip2_message(
                &instance,
                event.terminal.as_deref().unwrap_or_default(),
                &event.message,
                event.outcome,
            );
        }),
    };
    let listener = TcpListener::bind(settings.listen)
        .map_err(|e| format!("sip2: cannot listen on {}: {e}", settings.listen))?;
    let address = listener.local_addr().map_err(|e| e.to_string())?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let stop = Arc::new(AtomicBool::new(false));
    let active = Arc::new(AtomicUsize::new(0));
    let thread = {
        let (stop, active, timeout) = (Arc::clone(&stop), Arc::clone(&active), settings.timeout);
        std::thread::Builder::new()
            .name("sip2".to_owned())
            .spawn(move || accept(&listener, &desk, &stop, &active, timeout))
            .map_err(|e| e.to_string())?
    };
    Ok((
        Listener {
            stop,
            active,
            thread: Some(thread),
        },
        address,
    ))
}

fn accept(
    listener: &TcpListener,
    desk: &Desk,
    stop: &Arc<AtomicBool>,
    active: &Arc<AtomicUsize>,
    timeout: Duration,
) {
    while !stop.load(Ordering::SeqCst) {
        let stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                std::thread::sleep(POLL);
                continue;
            }
            Err(_) => {
                std::thread::sleep(POLL);
                continue;
            }
        };
        if active.load(Ordering::SeqCst) >= MAX_CONNECTIONS {
            // Dropping the stream closes it without a response (spec, item 17).
            drop(stream);
            continue;
        }
        active.fetch_add(1, Ordering::SeqCst);
        let (desk, stop, active) = (desk.clone(), Arc::clone(stop), Arc::clone(active));
        let spawned = std::thread::Builder::new()
            .name("sip2-terminal".to_owned())
            .spawn({
                let active = Arc::clone(&active);
                move || {
                    if let Ok(mut connection) = Connection::new(stream, stop, timeout) {
                        // A write that fails means the terminal is gone.
                        let _closed = serve(&mut connection, &desk).is_ok();
                    }
                    active.fetch_sub(1, Ordering::SeqCst);
                }
            });
        if spawned.is_err() {
            active.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

/// A terminal's connection: reads wake up often to see the stop flag, and give up
/// after the inactivity timeout.
struct Connection {
    stream: TcpStream,
    stop: Arc<AtomicBool>,
    timeout: Duration,
}

impl Connection {
    fn new(stream: TcpStream, stop: Arc<AtomicBool>, timeout: Duration) -> io::Result<Self> {
        stream.set_nonblocking(false)?;
        stream.set_read_timeout(Some(POLL))?;
        stream.set_write_timeout(Some(timeout))?;
        Ok(Self {
            stream,
            stop,
            timeout,
        })
    }
}

impl Read for Connection {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let deadline = Instant::now() + self.timeout;
        loop {
            if self.stop.load(Ordering::SeqCst) || Instant::now() >= deadline {
                return Err(io::Error::from(ErrorKind::TimedOut));
            }
            match self.stream.read(buffer) {
                Err(error)
                    if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                other => return other,
            }
        }
    }
}

impl Write for Connection {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.stream.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(text: &str) -> Result<Sip2Settings, String> {
        toml::from_str::<Sip2Section>(text)
            .map_err(|e| e.to_string())?
            .settings()
    }

    #[test]
    fn a_section_takes_the_defaults_and_its_terminals() {
        let read = settings(
            "listen = \"127.0.0.1:6001\"\n\
             [[terminals]]\nlogin = \"term1\"\nunit = \" Central \"\n\
             [[terminals]]\nlogin = \"term2\"\nunit = \"Anexo\"\nterminator = \"cr\"\n\
             encoding = \"utf-8\"\n",
        )
        .unwrap();
        assert_eq!(read.timeout, DEFAULT_TIMEOUT);
        assert_eq!(read.currency, None);
        assert_eq!(
            read.terminals,
            [
                TerminalSettings {
                    login: "term1".to_owned(),
                    unit: "Central".to_owned(),
                    terminator: Terminator::CrLf,
                    encoding: Encoding::Latin1,
                },
                TerminalSettings {
                    login: "term2".to_owned(),
                    unit: "Anexo".to_owned(),
                    terminator: Terminator::Cr,
                    encoding: Encoding::Utf8,
                },
            ]
        );
    }

    #[test]
    fn a_wrong_section_says_what_is_wrong() {
        for (text, error) in [
            ("listen = \"nowhere\"", "listen"),
            (
                "listen = \"127.0.0.1:6001\"\ntimeout_seconds = 0",
                "timeout_seconds",
            ),
            (
                "listen = \"127.0.0.1:6001\"\ncurrency = \"REAL\"",
                "currency",
            ),
            ("listen = \"127.0.0.1:6001\"\nport = 6001", "port"),
            (
                "listen = \"127.0.0.1:6001\"\n[[terminals]]\nlogin = \"t\"\nunit = \"C\"\nterminator = \"lf\"",
                "terminator",
            ),
            (
                "listen = \"127.0.0.1:6001\"\n[[terminals]]\nlogin = \"t\"\nunit = \"C\"\nencoding = \"utf8\"",
                "encoding",
            ),
            (
                "listen = \"127.0.0.1:6001\"\n[[terminals]]\nlogin = \"t\"\nunit = \"C\"\n[[terminals]]\nlogin = \"t\"\nunit = \"D\"",
                "repeated",
            ),
            (
                "listen = \"127.0.0.1:6001\"\n[[terminals]]\nlogin = \" \"\nunit = \"C\"",
                "required",
            ),
        ] {
            let refused = settings(text).unwrap_err();
            assert!(refused.contains(error), "{text}: {refused}");
        }
    }
}
