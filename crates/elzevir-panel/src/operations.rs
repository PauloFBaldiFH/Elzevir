// SPDX-License-Identifier: GPL-3.0-or-later
//! Explicit operator intentions and safe projections of background jobs.

/// An operation confirmed by the user, delivered once to the composition boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operation {
    /// One explicit free-address service intention.
    Address(crate::AddressOperation),
    /// Cloud configuration through the limited service worker.
    Cloud(crate::CloudOperation),
    /// Explicitly test the configured public HTTPS endpoints.
    TestTunnel,
    /// Publish separate catalog/staff hostnames through the selected Cloudflare account.
    CloudflareSetup {
        /// Catalog hostname.
        catalog: String,
        /// Staff hostname.
        staff: String,
        /// Optional tunnel name.
        name: String,
        /// Managed library service name.
        service: String,
        /// auto or http2.
        protocol: String,
    },
    /// Sign in to Cloudflare, reusing an existing private certificate when present.
    CloudflareLogin,
    /// Install or update the verified official Cloudflare tunnel binary.
    InstallCloudflared,
    /// Install or update the verified official rclone binary.
    InstallRclone,
    /// Fetch metadata for the latest official release.
    CheckUpdates,
    /// Recover an interrupted update using its private operator journal.
    RecoverUpdate {
        /// Installed executable path; empty selects the current executable.
        executable: String,
    },
    /// Install a pinned release through the managed service.
    Update {
        /// Version tag or latest.
        version: String,
        /// Managed service name.
        name: String,
    },
    /// Restart the selected managed service.
    Restart {
        /// Managed service name.
        name: String,
    },
    /// Request an operating-system reboot.
    Reboot,
    /// Create an encrypted managed backup.
    BackupNow,
    /// Authenticate the newest completed managed backup.
    VerifyLatest,
    /// Export a completed encrypted backup to a new file.
    ExportLatest {
        /// User-selected destination.
        output: String,
    },
    /// Export the recovery key to a new private file.
    ExportKey {
        /// User-selected destination.
        output: String,
    },
    /// Restore an authenticated backup to a stopped institution.
    Restore {
        /// Encrypted archive path.
        input: String,
        /// Private recovery key path.
        key: String,
    },
    /// Replace local retention and optional daily scheduling.
    Policy {
        /// Number of snapshots retained.
        keep: String,
        /// Optional daily time, HH:MM; empty disables it.
        time: String,
        /// Explicit IANA time zone.
        zone: String,
    },
}

impl Operation {
    /// Translation key identifying this action in its confirmation.
    #[must_use]
    pub const fn key(&self) -> &'static str {
        match self {
            Self::Address(action) => action.key(),
            Self::Cloud(action) => action.key(),
            Self::TestTunnel => "op-tunnel-test",
            Self::CloudflareSetup { .. } => "op-cloudflare-setup",
            Self::CloudflareLogin => "op-cloudflare-login",
            Self::InstallCloudflared => "op-install-cloudflared",
            Self::InstallRclone => "op-install-rclone",
            Self::CheckUpdates => "op-update-check",
            Self::RecoverUpdate { .. } => "op-update-recover",
            Self::Update { .. } => "op-update",
            Self::Restart { .. } => "op-restart",
            Self::Reboot => "op-reboot",
            Self::BackupNow => "op-backup-now",
            Self::VerifyLatest => "op-backup-verify",
            Self::ExportLatest { .. } => "op-backup-export",
            Self::ExportKey { .. } => "op-key-export",
            Self::Restore { .. } => "op-restore",
            Self::Policy { .. } => "op-policy",
        }
    }
}

/// One safe job observation; arbitrary server messages do not enter the UI.
#[derive(Debug, Clone)]
pub struct Job {
    /// Random job identity.
    pub id: String,
    /// Known translation key for the action.
    pub action: &'static str,
    /// Known translation key for the outcome.
    pub state: &'static str,
    /// Validated timestamp.
    pub at: String,
}

/// Operational state from the selected institution.
#[derive(Debug, Clone, Default)]
pub struct Operations {
    /// Retention, when the policy could be read.
    pub keep: Option<u16>,
    /// Daily wall-clock time and IANA time zone, when enabled.
    pub daily: Option<String>,
    /// At most 64 recognized job observations.
    pub jobs: Vec<Job>,
    /// Most recent client action result, expressed by a known translation key.
    pub result: Option<&'static str>,
    /// Current server session count; absent when no trusted observation is available.
    pub sessions: Option<usize>,
    /// Latest version, only after a successful explicit metadata check.
    pub latest: Option<String>,
    /// The available release is newer than the installed program.
    pub update_available: bool,
    /// Bounded plain-text official release notes.
    pub release_notes: Option<String>,
    /// Allowlisted sign-in URLs for pending interactive jobs.
    pub authorization_uris: Vec<String>,
    /// Public tunnel candidates and bounded evidence.
    pub tunnel: crate::TunnelState,
    /// Last safe remote address-service observation.
    pub remote_address: Option<crate::RemoteAddress>,
    /// Registration phase, assigned hostnames and the local connector service state.
    pub domain: Option<crate::DomainStatus>,
    /// Safe cloud connection metadata.
    pub cloud: Option<crate::CloudView>,
}

/// Bounded editing and confirmation, without access to operating-system resources.
pub struct Form {
    /// Action currently being prepared.
    pub operation: Operation,
    /// Current field, or confirmation when all fields have been entered.
    pub field: usize,
    /// Values under edit, at most eight fields of 512 Unicode scalar values each.
    pub values: Vec<String>,
    /// Translation keys for the fields.
    pub labels: Vec<&'static str>,
}
impl Form {
    /// Creates the fields appropriate for one explicit action.
    #[must_use]
    pub fn new(operation: Operation) -> Self {
        let (labels, values) = match &operation {
            Operation::Address(action) => action.fields(),
            Operation::Cloud(action) => action.fields(),
            Operation::CloudflareSetup {
                catalog,
                staff,
                name,
                service,
                protocol,
            } => (
                vec![
                    "op-catalog-host",
                    "op-staff-host",
                    "op-tunnel-name",
                    "op-service",
                    "op-transport",
                ],
                vec![
                    catalog.clone(),
                    staff.clone(),
                    name.clone(),
                    service.clone(),
                    protocol.clone(),
                ],
            ),
            Operation::Update { version, name } => (
                vec!["op-version", "op-service"],
                vec![version.clone(), name.clone()],
            ),
            Operation::RecoverUpdate { executable } => {
                (vec!["op-executable"], vec![executable.clone()])
            }
            Operation::Restart { name } => (vec!["op-service"], vec![name.clone()]),
            Operation::TestTunnel
            | Operation::CloudflareLogin
            | Operation::InstallCloudflared
            | Operation::InstallRclone
            | Operation::CheckUpdates
            | Operation::Reboot
            | Operation::BackupNow
            | Operation::VerifyLatest => (vec![], vec![]),
            Operation::ExportLatest { output } | Operation::ExportKey { output } => {
                (vec!["op-output"], vec![output.clone()])
            }
            Operation::Restore { input, key } => {
                (vec!["op-input", "op-key"], vec![input.clone(), key.clone()])
            }
            Operation::Policy { keep, time, zone } => (
                vec!["op-keep", "op-time", "op-zone"],
                vec![keep.clone(), time.clone(), zone.clone()],
            ),
        };
        Self {
            operation,
            field: 0,
            values,
            labels,
        }
    }
    /// Whether all fields are ready for explicit confirmation.
    #[must_use]
    pub fn confirming(&self) -> bool {
        self.field >= self.labels.len()
    }
    /// Commits the typed values into the intended operation.
    #[must_use]
    pub fn finish(self) -> Option<Operation> {
        if !self.confirming() {
            return None;
        }
        if let Operation::Cloud(action) = self.operation {
            return Some(Operation::Cloud(action.finish(self.values)?));
        }
        if let Operation::Address(action) = self.operation {
            return Some(Operation::Address(action.finish(self.values)?));
        }
        let mut values = self.values.into_iter();
        Some(match self.operation {
            Operation::Address(_) | Operation::Cloud(_) => return None,
            Operation::CloudflareSetup { .. } => Operation::CloudflareSetup {
                catalog: values.next()?,
                staff: values.next()?,
                name: values.next()?,
                service: values.next()?,
                protocol: values.next()?,
            },
            Operation::TestTunnel => Operation::TestTunnel,
            Operation::CloudflareLogin => Operation::CloudflareLogin,
            Operation::InstallCloudflared => Operation::InstallCloudflared,
            Operation::InstallRclone => Operation::InstallRclone,
            Operation::CheckUpdates => Operation::CheckUpdates,
            Operation::RecoverUpdate { .. } => Operation::RecoverUpdate {
                executable: values.next()?,
            },
            Operation::Update { .. } => Operation::Update {
                version: values.next()?,
                name: values.next()?,
            },
            Operation::Restart { .. } => Operation::Restart {
                name: values.next()?,
            },
            Operation::Reboot => Operation::Reboot,
            Operation::BackupNow => Operation::BackupNow,
            Operation::VerifyLatest => Operation::VerifyLatest,
            Operation::ExportLatest { .. } => Operation::ExportLatest {
                output: values.next()?,
            },
            Operation::ExportKey { .. } => Operation::ExportKey {
                output: values.next()?,
            },
            Operation::Restore { .. } => Operation::Restore {
                input: values.next()?,
                key: values.next()?,
            },
            Operation::Policy { .. } => Operation::Policy {
                keep: values.next()?,
                time: values.next()?,
                zone: values.next()?,
            },
        })
    }
}

impl std::fmt::Debug for Form {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Form")
            .field("field", &self.field)
            .finish_non_exhaustive()
    }
}
