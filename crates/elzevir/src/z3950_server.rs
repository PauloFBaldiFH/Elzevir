// SPDX-License-Identifier: GPL-3.0-or-later
//! The Z39.50 listener of `elzevir serve` (docs/specs/opac/z3950.md, items 1 to 3):
//! `[z3950]` in `elzevir.toml`, one thread per connection, at most [`MAX_CONNECTIONS`]
//! at a time.

use std::io::{self, ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use elzevir_search::TantivyIndex;
use elzevir_sources::z3950_server::{Z3950Catalog, Z3950Event, serve};
use elzevir_store::ElzevirStore;
use serde::Deserialize;

use crate::logging;

/// Most connections at a time; the next one is closed (item 3).
const MAX_CONNECTIONS: usize = 64;
/// How often a waiting connection or the listener looks at the stop flag.
const POLL: Duration = Duration::from_millis(250);

/// `[z3950]` as written.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Z3950Section {
    listen: String,
    database: Option<String>,
    timeout_seconds: Option<u64>,
    max_records: Option<usize>,
}

/// `[z3950]`, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Z3950Settings {
    pub(crate) listen: SocketAddr,
    pub(crate) database: String,
    pub(crate) timeout: Duration,
    pub(crate) max_records: usize,
}

impl Z3950Section {
    /// The settings, or what is wrong with them (item 1).
    pub(crate) fn settings(&self) -> Result<Z3950Settings, String> {
        let listen = self
            .listen
            .parse::<SocketAddr>()
            .map_err(|e| format!("listen: {e}"))?;
        let database = self.database.as_deref().unwrap_or("biblios").trim();
        if database.is_empty()
            || database.chars().count() > 64
            || !database.chars().all(|c| c.is_ascii_graphic())
        {
            return Err("database is a name of 1 to 64 characters without spaces".to_owned());
        }
        let timeout_seconds = self.timeout_seconds.unwrap_or(300);
        if !(1..=3600).contains(&timeout_seconds) {
            return Err("timeout_seconds goes from 1 to 3600".to_owned());
        }
        let max_records = self.max_records.unwrap_or(100);
        if !(1..=1000).contains(&max_records) {
            return Err("max_records goes from 1 to 1000".to_owned());
        }
        Ok(Z3950Settings {
            listen,
            database: database.to_owned(),
            timeout: Duration::from_secs(timeout_seconds),
            max_records,
        })
    }
}

/// A running listener; dropping it stops accepting, closes the connections and waits
/// for them, so that the store they hold can shut down.
pub(crate) struct Listener {
    stop: Arc<AtomicBool>,
    active: Arc<AtomicUsize>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Listener {
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

/// Opens the listener when `[z3950]` is set, and says where.
pub(crate) fn open(
    settings: Option<&Z3950Settings>,
    store: &Arc<ElzevirStore>,
    index: &Arc<TantivyIndex>,
    instance: &str,
) -> Result<Option<Listener>, String> {
    let Some(settings) = settings else {
        return Ok(None);
    };
    let (listener, address) = start(settings, store, index, instance)?;
    println!("Z39.50 on {address}");
    Ok(Some(listener))
}

/// Opens the listener of `settings` on `store` and `index`.
pub(crate) fn start(
    settings: &Z3950Settings,
    store: &Arc<ElzevirStore>,
    index: &Arc<TantivyIndex>,
    instance: &str,
) -> Result<(Listener, SocketAddr), String> {
    let catalog = Z3950Catalog {
        catalog: Arc::clone(store) as _,
        index: Arc::clone(index) as _,
        database: settings.database.clone(),
        max_records: settings.max_records,
        version: env!("CARGO_PKG_VERSION").to_owned(),
    };
    let listener = TcpListener::bind(settings.listen)
        .map_err(|e| format!("z3950: cannot listen on {}: {e}", settings.listen))?;
    let address = listener.local_addr().map_err(|e| e.to_string())?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let stop = Arc::new(AtomicBool::new(false));
    let active = Arc::new(AtomicUsize::new(0));
    let thread = {
        let (stop, active, timeout) = (Arc::clone(&stop), Arc::clone(&active), settings.timeout);
        let instance = instance.to_owned();
        std::thread::Builder::new()
            .name("z3950".to_owned())
            .spawn(move || accept(&listener, &catalog, &instance, (&stop, &active), timeout))
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
    catalog: &Z3950Catalog,
    instance: &str,
    (stop, active): (&Arc<AtomicBool>, &Arc<AtomicUsize>),
    timeout: Duration,
) {
    while !stop.load(Ordering::SeqCst) {
        let Ok((stream, _)) = listener.accept() else {
            // Nothing waiting, or a failed accept: look again after a pause.
            std::thread::sleep(POLL);
            continue;
        };
        if active.load(Ordering::SeqCst) >= MAX_CONNECTIONS {
            // Dropping the stream closes it without a response (item 3).
            drop(stream);
            continue;
        }
        active.fetch_add(1, Ordering::SeqCst);
        let (catalog, stop, instance) = (catalog.clone(), Arc::clone(stop), instance.to_owned());
        let spawned = std::thread::Builder::new()
            .name("z3950-client".to_owned())
            .spawn({
                let active = Arc::clone(active);
                move || {
                    if let Ok(mut connection) = Connection::new(stream, stop, timeout) {
                        let log = |event: Z3950Event| logging::z3950_request(&instance, &event);
                        // A write that fails means the client is gone.
                        let _closed = serve(&mut connection, &catalog, &log).is_ok();
                    }
                    active.fetch_sub(1, Ordering::SeqCst);
                }
            });
        if spawned.is_err() {
            active.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

/// A client's connection: reads wake up often to see the stop flag, which ends them as
/// [`ErrorKind::ConnectionAborted`], and give up as [`ErrorKind::TimedOut`] after the
/// inactivity timeout.
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
            if self.stop.load(Ordering::SeqCst) {
                return Err(io::Error::from(ErrorKind::ConnectionAborted));
            }
            if Instant::now() >= deadline {
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

    fn settings(text: &str) -> Result<Z3950Settings, String> {
        toml::from_str::<Z3950Section>(text)
            .map_err(|e| e.to_string())?
            .settings()
    }

    #[test]
    fn a_section_takes_the_defaults() {
        assert_eq!(
            settings("listen = \"127.0.0.1:2100\"").unwrap(),
            Z3950Settings {
                listen: "127.0.0.1:2100".parse().unwrap(),
                database: "biblios".to_owned(),
                timeout: Duration::from_mins(5),
                max_records: 100,
            }
        );
    }

    #[test]
    fn a_wrong_section_says_what_is_wrong() {
        for (text, error) in [
            ("listen = \"nowhere\"", "listen"),
            (
                "listen = \"127.0.0.1:2100\"\ndatabase = \"a b\"",
                "database",
            ),
            (
                "listen = \"127.0.0.1:2100\"\ntimeout_seconds = 0",
                "timeout_seconds",
            ),
            (
                "listen = \"127.0.0.1:2100\"\ntimeout_seconds = 3601",
                "timeout_seconds",
            ),
            (
                "listen = \"127.0.0.1:2100\"\nmax_records = 1001",
                "max_records",
            ),
            ("listen = \"127.0.0.1:2100\"\nport = 2100", "port"),
        ] {
            let refused = settings(text).unwrap_err();
            assert!(refused.contains(error), "{text}: {refused}");
        }
    }
}
