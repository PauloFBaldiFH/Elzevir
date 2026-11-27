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
    /// Test only the public staff endpoint.
    TestStaff,
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
    /// Start, stop or restart the background server of a personal
    /// installation, which has no service manager.
    Server(ServerAction),
    /// Change the institution's name, listen address and language in its
    /// configuration file; the server applies them when it restarts.
    Settings {
        /// Institution name shown in the catalog.
        name: String,
        /// Listen address, host and port.
        listen: String,
        /// Interface language, `en` or `pt-BR`.
        language: String,
    },
    /// Restart the selected managed service.
    Restart {
        /// Managed service name.
        name: String,
    },
    /// Request an operating-system reboot.
    Reboot,
    /// Create an encrypted managed backup.
    BackupNow {
        /// Scope and compression.
        options: crate::BackupOptions,
        /// Optional export directory on the operator's host.
        folder: String,
    },
    /// Recheck this specific catalogue entry.
    VerifyArtifact(String),
    /// Export the selected entry to a directory on the operator host.
    ExportArtifact {
        /// Stable catalogue identity.
        id: String,
        /// Additional destination directory.
        folder: String,
    },
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
    /// Apply the session-bound review after confirmation.
    ApplyRestore(RestoreReview),
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
            Self::TestStaff => "op-staff-test",
            Self::TestTunnel => "op-tunnel-test",
            Self::CloudflareSetup { .. } => "op-cloudflare-setup",
            Self::CloudflareLogin => "op-cloudflare-login",
            Self::InstallCloudflared => "op-install-cloudflared",
            Self::InstallRclone => "op-install-rclone",
            Self::CheckUpdates => "op-update-check",
            Self::RecoverUpdate { .. } => "op-update-recover",
            Self::Update { .. } => "op-update",
            Self::Server(action) => action.key(),
            Self::Settings { .. } => "op-settings",
            Self::Restart { .. } => "op-restart",
            Self::Reboot => "op-reboot",
            Self::BackupNow { .. } => "op-backup-now",
            Self::VerifyArtifact(_) => "bc-verify-selected",
            Self::VerifyLatest => "op-backup-verify",
            Self::ExportArtifact { .. } => "bc-copy-selected",
            Self::ExportLatest { .. } => "op-backup-export",
            Self::ExportKey { .. } => "op-key-export",
            Self::Restore { .. } => "restore-validate",
            Self::ApplyRestore(_) => "op-restore",
            Self::Policy { .. } => "op-policy",
        }
    }
}

/// What to do with a personal installation's server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerAction {
    /// Start it in the background; nothing happens when it already runs.
    Start,
    /// Stop it in order; the catalog and the staff interface go offline.
    Stop,
    /// Stop, then start again.
    Restart,
}

impl ServerAction {
    /// Translation key of the confirmation.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Start => "op-server-start",
            Self::Stop => "op-server-stop",
            Self::Restart => "op-server-restart",
        }
    }
}

/// One safe job observation; arbitrary server messages do not enter the UI.
#[derive(Debug, Clone)]
pub struct Job {
    /// Last durable backup phase, including the phase of failure.
    pub phase: Option<&'static str>,
    /// Random job identity.
    pub id: String,
    /// Known translation key for the action.
    pub action: &'static str,
    /// Known translation key for the outcome.
    pub state: &'static str,
    /// Validated timestamp.
    pub at: String,
}

/// Public metadata of a fully authenticated archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreReview {
    /// One-use handle bound to the operator session.
    pub token: String,
    /// Archive identity.
    pub id: String,
    /// Archive basename.
    pub file: String,
    /// Manifest timestamp.
    pub created: String,
    /// Schema of the snapshot database.
    pub schema: i64,
    /// Authenticated backup scope.
    pub scope: crate::BackupScope,
    /// The artifact belongs to the selected institution's catalog.
    pub managed: bool,
}

/// Local evidence of backups authenticated before publication.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LocalBackup {
    /// Catalog or corresponding artifact could not be confirmed.
    #[default]
    Unknown,
    /// No managed or unclassified artifacts in the backup directory.
    Empty,
    /// The latest catalog entry corresponds to a published regular file.
    Verified,
}

/// A retained catalogue entry, newest entries first.
#[derive(Debug, Clone, Default)]
pub struct BackupArtifact {
    /// Stable identifier assigned by the institution.
    pub id: String,
    /// Byte count of a regular published archive, absent when inaccessible.
    pub bytes: Option<u64>,
    /// Filesystem modification time in UTC.
    pub modified: Option<String>,
    /// Latest attributable export receipt, when the latest result is unambiguous.
    pub export: Option<&'static str>,
    /// Creation time of the most recent attributable export request.
    pub export_at: Option<String>,
    /// Latest explicit recheck observed for this archive in the daemon history.
    pub recheck: Option<&'static str>,
}
/// Operational state from the selected institution.
#[derive(Debug, Clone, Default)]
pub struct Operations {
    /// Completed validation awaiting review.
    pub review: Option<RestoreReview>,
    /// Result of checking the local catalog and the retained file.
    pub local_backup: LocalBackup,
    /// Whether the latest dated export receipt records a failure.
    pub latest_export_failed: Option<bool>,
    /// Identities in the institution's local ownership catalog.
    pub managed_ids: Vec<String>,
    /// Operator-visible default managed backup directory.
    pub backup_folder: Option<std::path::PathBuf>,
    /// Up to 365 recently retained entries, including missing files.
    pub artifacts: Vec<BackupArtifact>,
    /// Identity returned by this session's last manual creation request.
    pub manual_job: Option<String>,
    /// Export helper could not start after the server accepted the manual backup.
    pub manual_export_refused: bool,
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
    /// Current restore path check for each input.
    pub(crate) target_page: usize,
    pub(crate) checks: [crate::restore_paths::Checked; 2],
    /// Localized paste feedback.
    pub(crate) notice: Option<&'static str>,
    /// Translation keys for the fields.
    pub labels: Vec<&'static str>,
}
/// Fields answered by picking one of fixed values, with the translation
/// key of each value's button.
#[must_use]
pub(crate) fn choices(label: &str) -> Option<&'static [(&'static str, &'static str)]> {
    match label {
        "restore-decision" => Some(&[("back", "restore-back"), ("apply", "restore-apply")]),
        "backup-scope" => Some(&[("full", "backup-full"), ("database", "backup-db")]),
        "backup-compression" => Some(&[
            ("fast", "backup-fast"),
            ("balanced", "backup-balanced"),
            ("compact", "backup-compact"),
        ]),
        "settings-language" => Some(&[("en", "language-en"), ("pt-BR", "language-pt-BR")]),
        _ => None,
    }
}

impl Form {
    pub(crate) fn open_broker_settings(&mut self) -> bool {
        if !matches!(
            self.operation,
            Operation::Address(crate::AddressOperation::Enroll { .. })
        ) {
            return false;
        }
        if self.labels.last() != Some(&"broker-url") {
            self.labels.push("broker-url");
            self.values.push(String::new());
        }
        self.field = self.labels.len() - 1;
        true
    }
    /// Creates the fields appropriate for one explicit action.
    #[must_use]
    pub fn new(operation: Operation) -> Self {
        let (labels, values) = match &operation {
            Operation::Address(action) => {
                let (mut labels, mut values) = action.fields();
                if matches!(action,crate::AddressOperation::Enroll { base,.. } if base.is_empty()) {
                    labels.pop();
                    values.pop();
                }
                (labels, values)
            }
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
            Operation::Settings {
                name,
                listen,
                language,
            } => (
                vec!["settings-name", "settings-listen", "settings-language"],
                vec![name.clone(), listen.clone(), language.clone()],
            ),
            Operation::Server(_)
            | Operation::TestStaff
            | Operation::TestTunnel
            | Operation::CloudflareLogin
            | Operation::InstallCloudflared
            | Operation::InstallRclone
            | Operation::CheckUpdates
            | Operation::Reboot
            | Operation::VerifyArtifact(_)
            | Operation::VerifyLatest => (vec![], vec![]),
            Operation::ExportLatest { output } => {
                (vec!["backup-export-target"], vec![output.clone()])
            }
            Operation::ExportKey { output } => (vec!["op-output"], vec![output.clone()]),
            Operation::BackupNow { options, folder } => (
                vec!["backup-scope", "backup-compression", "backup-folder"],
                vec![
                    match options.scope {
                        crate::BackupScope::Full => "full",
                        crate::BackupScope::Database => "database",
                    }
                    .into(),
                    match options.profile {
                        crate::BackupProfile::Fast => "fast",
                        crate::BackupProfile::Balanced => "balanced",
                        crate::BackupProfile::Compact => "compact",
                    }
                    .into(),
                    folder.clone(),
                ],
            ),
            Operation::ExportArtifact { folder, .. } => {
                (vec!["backup-folder"], vec![folder.clone()])
            }
            Operation::ApplyRestore(_) => (vec!["restore-decision"], vec!["back".into()]),
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
            target_page: 0,
            checks: Default::default(),
            notice: None,
            values,
            labels,
        }
    }
    pub(crate) fn paste(&mut self, value: &str) {
        let Some(field) = self.values.get_mut(self.field) else {
            return;
        };
        if matches!(self.operation, Operation::Restore { .. }) {
            if crate::restore_paths::single(value) {
                value.clone_into(field);
                self.notice = None;
            } else {
                field.clear();
                self.notice = Some("restore-single");
            }
        } else if self
            .labels
            .get(self.field)
            .is_some_and(|label| choices(label).is_none())
            && !value.chars().any(char::is_control)
            && field.chars().count() + value.chars().count() <= 512
        {
            field.push_str(value);
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
            Operation::Server(action) => Operation::Server(action),
            Operation::TestStaff => Operation::TestStaff,
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
            Operation::Settings { .. } => Operation::Settings {
                name: values.next()?,
                listen: values.next()?,
                language: values.next()?,
            },
            Operation::Reboot => Operation::Reboot,
            Operation::BackupNow { .. } => Operation::BackupNow {
                options: crate::BackupOptions {
                    scope: match values.next()?.as_str() {
                        "full" => crate::BackupScope::Full,
                        "database" => crate::BackupScope::Database,
                        _ => return None,
                    },
                    profile: match values.next()?.as_str() {
                        "fast" => crate::BackupProfile::Fast,
                        "balanced" => crate::BackupProfile::Balanced,
                        "compact" => crate::BackupProfile::Compact,
                        _ => return None,
                    },
                },
                folder: values.next()?,
            },
            Operation::VerifyArtifact(id) => Operation::VerifyArtifact(id),
            Operation::ExportArtifact { id, .. } => Operation::ExportArtifact {
                id,
                folder: values.next()?,
            },
            Operation::VerifyLatest => Operation::VerifyLatest,
            Operation::ExportLatest { .. } => Operation::ExportLatest {
                output: values.next()?,
            },
            Operation::ExportKey { .. } => Operation::ExportKey {
                output: values.next()?,
            },
            Operation::ApplyRestore(review) => {
                if values.next()? != "apply" {
                    return None;
                }
                Operation::ApplyRestore(review)
            }
            Operation::Restore { .. } => Operation::Restore {
                input: self.checks[0].resolved(&values.next()?)?,
                key: self.checks[1].resolved(&values.next()?)?,
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
