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
