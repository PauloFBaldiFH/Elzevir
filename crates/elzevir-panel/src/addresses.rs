// SPDX-License-Identifier: GPL-3.0-or-later
//! Observed HTTP addresses, independent of network and web implementation.

use crate::CheckResult;
use std::net::SocketAddr;

/// Staff topology as configured or confirmed for a running process.
#[derive(Debug, Clone, Default)]
pub enum StaffAccess {
    /// Catalog and staff share one HTTP address, separated by path.
    #[default]
    SameAddress,
    /// Distinct DNS names on the main listener.
    Hostname {
        /// Catalog DNS name.
        catalog_host: String,
        /// Staff DNS name.
        staff_host: String,
    },
    /// Dedicated staff listener, optionally restricted by source IP.
    SeparateListener {
        /// Staff binding, configured or actually observed.
        listen: SocketAddr,
        /// Allowed TCP peer IPs/CIDRs; empty means unrestricted by IP.
        allow_ips: Vec<String>,
    },
}

/// Addresses are candidates until an explicit local HTTP test is requested.
#[derive(Debug, Clone, Default)]
pub struct AddressBook {
    /// Topology advertised by the confirmed running process, never inferred.
    pub observed_mode: Option<StaffAccess>,
    /// Direct catalog URL for this computer.
    pub catalog: Option<String>,
    /// Direct staff URL for this computer.
    pub staff: Option<String>,
    /// At most eight catalog candidates on locally observed interfaces.
    pub catalog_lan: Vec<String>,
    /// At most eight staff candidates on locally observed interfaces.
    pub staff_lan: Vec<String>,
    /// Last explicit catalog HTTP test from this machine.
    pub catalog_test: CheckResult,
    /// Last explicit login-form HTTP test from this machine.
    pub staff_test: CheckResult,
    /// UTC start time of the explicit local tests.
    pub tested_at: Option<String>,
}

/// Evidence from an explicit public HTTPS request.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PublicCheck {
    /// No explicit request yet.
    #[default]
    NotRun,
    /// The endpoint responded successfully or redirected to sign-in.
    Ok,
    /// A staff edge gate refused the unauthenticated request.
    Protected,
    /// The endpoint responded with an error.
    Failed,
    /// The request could not establish a trustworthy response.
    Unknown,
}
/// Allowlisted connector events; raw tool messages never enter this model.
#[derive(Debug, Clone, Default)]
pub enum TunnelLog {
    /// No safe log source could be read.
    #[default]
    Unavailable,
    /// A bounded private log window was projected.
    Ready {
        /// Known Fluent keys, newest events first.
        events: Vec<&'static str>,
        /// Complete unrecognized/invalid records omitted from the window.
        omitted: u64,
    },
}
/// Configured public candidates and the most recent explicit checks.
#[derive(Debug, Clone)]
pub struct Tunnel {
    /// Configured catalog URL; this alone does not prove reachability.
    pub catalog: String,
    /// Configured staff URL; this alone does not prove reachability.
    pub staff: String,
    /// UTC time of the last explicit test.
    pub tested_at: Option<String>,
    /// UTC time of the most recent staff request.
    pub staff_tested_at: Option<String>,
    /// Most recent catalog response.
    pub catalog_check: PublicCheck,
    /// Most recent staff response.
    pub staff_check: PublicCheck,
    /// Bounded, redacted connector events.
    pub logs: TunnelLog,
}
/// Missing configuration is distinct from failed observation.
#[derive(Debug, Clone, Default)]
pub enum TunnelState {
    /// No configured managed public connector.
    NotConfigured,
    /// Managed candidates with explicit evidence.
    Configured(Tunnel),
    /// Configuration could not be safely observed.
    #[default]
    Unavailable,
}
/// Selected detail tab within Addresses.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TunnelTab {
    /// Local and public address overview.
    #[default]
    Addresses,
    /// Enrollment and connector management.
    Manage,
    /// Explicit public test results.
    Test,
    /// Redacted connector log projection.
    Log,
    /// Public catalog QR code.
    Share,
}

impl PublicCheck {
    /// Stable value for command-line reports, independent of translation.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::NotRun => "not_run",
            Self::Ok => "ok",
            Self::Protected => "protected",
            Self::Failed => "failed",
            Self::Unknown => "unknown",
        }
    }
}

/// Safe last observation from the free-address service.
#[derive(Debug, Clone)]
pub struct RemoteAddress {
    /// When the remote settings were observed.
    pub at: String,
    /// Whether the public directory includes the catalog.
    pub listed: bool,
    /// Whether remote staff edge authentication is enabled.
    pub staff_remote: bool,
}
