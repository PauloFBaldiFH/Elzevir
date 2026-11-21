// SPDX-License-Identifier: GPL-3.0-or-later
//! Observation values supplied by the application; no operations or I/O here.

use std::net::SocketAddr;

/// Institution selected when opening the panel.
#[derive(Debug, Default)]
pub struct Institution {
    /// Optional configured display name.
    pub name: Option<String>,
    /// Configured listener, if a configuration file was provided.
    pub configured_listen: Option<SocketAddr>,
    /// Staff topology loaded at panel startup, when a file was supplied.
    pub configured_access: Option<crate::StaffAccess>,
    /// Language the server and the panel use.
    pub language: Option<String>,
    /// Run by the system's service manager; otherwise a personal
    /// installation whose server the panel starts and stops itself.
    pub managed: bool,
}

/// Last observed component state.
#[derive(Debug, Clone, Copy)]
pub enum Component {
    /// Component is ready.
    Ok,
    /// Component has not completed its first cycle.
    Starting,
    /// Component is not ready.
    Failed,
}

/// Confirmed process details, never inferred from a stale metadata file.
#[derive(Debug, Clone)]
pub struct Details {
    /// Executable version.
    pub version: String,
    /// Server process id.
    pub pid: u32,
    /// Monotonic process uptime.
    pub uptime_seconds: u64,
    /// Database probe result.
    pub database: Component,
    /// Indexer observation.
    pub indexer: Component,
    /// Bound catalog listener, when advertised by this server version.
    pub listen: Option<SocketAddr>,
}

/// Process state and, only when confirmed, its details.
#[derive(Debug, Clone, Default)]
pub enum ServerStatus {
    /// All required probes are ready.
    Running(Details),
    /// The server answered but is not ready.
    Degraded(Details),
    /// The institution lock was free.
    Stopped,
    /// No trustworthy process observation.
    #[default]
    Unavailable,
}

/// Read-only filesystem observations; missing measurements stay missing.
#[derive(Debug, Clone, Default)]
pub struct Storage {
    /// Size of the regular database file.
    pub database_bytes: Option<u64>,
    /// WAL size, zero only when absent beside an existing database.
    pub wal_bytes: Option<u64>,
    /// Free space on the selected data filesystem.
    pub available_bytes: Option<u64>,
}

/// Machine measurements, not the application's exclusive resource usage.
#[derive(Debug, Clone, Default)]
pub struct Host {
    /// CPU percentage after a sufficient observation interval.
    pub cpu_percent: Option<f32>,
    /// Used and total memory, in bytes.
    pub memory_bytes: Option<(u64, u64)>,
}

/// One complete observation, with no handles back to its source.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    /// Process observation.
    pub status: ServerStatus,
    /// Filesystem measurements.
    pub storage: Storage,
    /// Host measurements.
    pub host: Host,
    /// Lightweight diagnostics and most recent explicit verification.
    pub diagnostics: crate::Diagnostics,
    /// Observed addresses and the last explicit local route tests.
    pub addresses: crate::AddressBook,
    /// Operational policy and background job results.
    pub operations: crate::Operations,
}

/// Pure connected-view state; its age is advanced by the runtime.
#[derive(Debug)]
pub struct LiveState {
    /// Selected configuration context.
    pub institution: Institution,
    /// Last completed observation, including failures.
    pub snapshot: Option<Snapshot>,
    /// Seconds since collection began; a slow read cannot make old data fresh.
    pub age_seconds: u64,
}

/// The demo cannot accidentally use a real source.
#[derive(Debug, Default)]
pub enum Mode {
    /// Standalone demonstration.
    #[default]
    Demo,
    /// Real, read-only observations.
    Live(Box<LiveState>),
}
