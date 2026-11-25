// SPDX-License-Identifier: GPL-3.0-or-later
//! Read-only diagnostic values and explicit observation requests.

/// Masked credential carried only to the local administrative client.
#[derive(Clone, PartialEq, Eq)]
pub struct AccountPassword(pub(crate) zeroize::Zeroizing<String>);
impl AccountPassword {
    /// Borrow for submission over the private institution channel.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}
impl std::fmt::Debug for AccountPassword {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[redacted]")
    }
}

/// Work requested from the single background observer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ObservationRequest {
    /// Refresh lightweight observations and the bounded log window.
    #[default]
    Poll,
    /// Test local catalog and staff HTTP routes, without database diagnostics.
    TestAddresses,
    /// Run the explicit integrity and HTTP checks.
    Verify,
    /// Verify, then create a new redacted support archive.
    Export,
    /// A confirmed operation, never coalesced with other requests.
    Operate(crate::Operation),
    /// Create an administrator in the selected institution.
    Administrator(String, AccountPassword),
}

/// Coalesced UI action state, separate from lightweight refresh.
#[derive(Debug, Default)]
pub struct DiagnosticAction {
    /// Open the local account form.
    pub account_requested: bool,
    /// Next explicit work request, if any.
    pub requested: Option<ObservationRequest>,
    /// Whether an explicit action is waiting for completion.
    pub pending: bool,
}

/// A check never conflates missing evidence with success.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CheckResult {
    /// Explicit check has not been requested.
    #[default]
    NotRun,
    /// The check succeeded.
    Ok,
    /// The check failed.
    Failed,
    /// The check could not be completed or is unsupported.
    Unknown,
}

impl CheckResult {
    /// Classifies observed space without asserting capacity for a future backup.
    #[must_use]
    pub const fn available_space(bytes: Option<u64>) -> Self {
        match bytes {
            None => Self::Unknown,
            Some(0) => Self::Failed,
            Some(_) => Self::Ok,
        }
    }

    /// Stable, nonlocalized report value.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::NotRun => "not_run",
            Self::Ok => "ok",
            Self::Failed => "failed",
            Self::Unknown => "unknown",
        }
    }
}

/// Known operational event; raw messages never enter the UI model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogEvent {
    /// Server started.
    ServerStarted,
    /// Server stopped normally.
    ServerStopped,
    /// Server failed.
    ServerFailed,
    /// Index batch committed.
    IndexerCommitted,
    /// Indexer will retry after a failure.
    IndexerFailed,
    /// Indexer worker panicked.
    IndexerPanicked,
    /// An administrative handler failed.
    AdminHandlerFailed,
    /// Runtime metadata cleanup failed.
    RuntimeCleanupFailed,
}

impl LogEvent {
    /// Stable allowlisted event identifier.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::ServerStarted => "server.started",
            Self::ServerStopped => "server.stopped",
            Self::ServerFailed => "server.failed",
            Self::IndexerCommitted => "indexer.committed",
            Self::IndexerFailed => "indexer.failed",
            Self::IndexerPanicked => "indexer.panicked",
            Self::AdminHandlerFailed => "admin.handler_failed",
            Self::RuntimeCleanupFailed => "runtime.cleanup_failed",
        }
    }

    /// Fixed severity for this operational event.
    #[must_use]
    pub const fn level(self) -> &'static str {
        match self {
            Self::ServerStarted | Self::ServerStopped | Self::IndexerCommitted => "INFO",
            Self::ServerFailed => "ERROR",
            _ => "WARN",
        }
    }
}

/// Safe log projection, without arbitrary payloads.
#[derive(Debug, Clone)]
pub struct LogEntry {
    /// Validated UTC timestamp.
    pub timestamp: String,
    /// Known event, including its fixed severity.
    pub event: LogEvent,
}

/// Availability of the selected institution's log window.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LogState {
    /// No log directory or matching file yet.
    #[default]
    Missing,
    /// Read successfully, possibly empty.
    Ready,
    /// Could not safely read the log files.
    Unavailable,
}

/// A bounded recent window, newest entries first.
#[derive(Debug, Clone, Default)]
pub struct LogFeed {
    /// Source availability.
    pub state: LogState,
    /// At most forty safe events.
    pub entries: Vec<LogEntry>,
    /// The byte, file, directory or event window was limited.
    pub limited: bool,
    /// Invalid or unknown complete lines omitted from the inspected window.
    pub omitted: u64,
}

/// Export outcome, replaced only after an explicit request.
#[derive(Debug, Clone, Default)]
pub enum ExportState {
    /// No export has been requested.
    #[default]
    NotRequested,
    /// A new archive was saved at this path.
    Saved(String),
    /// The archive could not be published.
    Failed,
}

/// Diagnostics accompanying one lightweight observation.
#[derive(Debug, Clone, Default)]
pub struct Diagnostics {
    /// SQLite integrity check performed by the existing server.
    pub quick_check: CheckResult,
    /// Explicit local catalog HTTP check.
    pub catalog_http: CheckResult,
    /// UTC time when the explicit checks started.
    pub verified_at: Option<String>,
    /// Recognized local data volume.
    pub local_volume: CheckResult,
    /// Basic directory access, without writing test files.
    pub directory_access: CheckResult,
    /// Bounded live log projection.
    pub logs: LogFeed,
    /// Most recent explicit export outcome.
    pub export: ExportState,
}

impl ObservationRequest {
    pub(crate) fn merge(self, next: Self) -> Option<Self> {
        if matches!(self, Self::Operate(_) | Self::Administrator(..))
            || matches!(next, Self::Operate(_) | Self::Administrator(..))
        {
            return if self == Self::Poll {
                Some(next)
            } else if next == Self::Poll {
                Some(self)
            } else {
                None
            };
        }
        let rank = |request: &Self| match request {
            Self::Poll => 0,
            Self::TestAddresses => 1,
            Self::Verify => 2,
            Self::Export => 3,
            Self::Operate(_) | Self::Administrator(..) => 4,
        };
        Some(if rank(&self) >= rank(&next) {
            self
        } else {
            next
        })
    }
}
