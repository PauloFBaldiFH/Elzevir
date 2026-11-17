// SPDX-License-Identifier: GPL-3.0-or-later
//! Explicit operator intentions and safe projections of background jobs.

/// An operation confirmed by the user, delivered once to the composition boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operation {
    /// Fetch metadata for the latest official release.
    CheckUpdates,
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
            Self::CheckUpdates => "op-update-check",
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
    /// Bounded plain-text official release notes.
    pub release_notes: Option<String>,
}

/// Bounded editing and confirmation, without access to operating-system resources.
#[derive(Debug)]
pub struct Form {
    /// Action currently being prepared.
    pub operation: Operation,
    /// Current field, or confirmation when all fields have been entered.
    pub field: usize,
    /// Values under edit, at most three fields of 512 Unicode scalar values each.
    pub values: Vec<String>,
    /// Translation keys for the fields.
    pub labels: Vec<&'static str>,
}
impl Form {
    /// Creates the fields appropriate for one explicit action.
    #[must_use]
    pub fn new(operation: Operation) -> Self {
        let (labels, values) = match &operation {
            Operation::Update { version, name } => (
                vec!["op-version", "op-service"],
                vec![version.clone(), name.clone()],
            ),
            Operation::Restart { name } => (vec!["op-service"], vec![name.clone()]),
            Operation::CheckUpdates
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
        let mut values = self.values.into_iter();
        Some(match self.operation {
            Operation::CheckUpdates => Operation::CheckUpdates,
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
