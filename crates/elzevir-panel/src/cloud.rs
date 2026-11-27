// SPDX-License-Identifier: GPL-3.0-or-later
//! Pure cloud-connection forms and safe provider observations.
/// Destination shown in the confirmation and checked again by the worker.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloudTarget {
    /// Plan identity.
    pub plan: String,
    /// Connection identity.
    pub connection: String,
    /// Relative folder.
    pub folder: String,
}
/// Work against one explicitly selected destination.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum RemoteCommand {
    /// Create a new backup for the selected plan.
    Create,
    /// List a one-based page.
    Browse(u16),
    /// Download the selected encrypted artifact.
    Download(String),
    /// Send an existing local artifact.
    Send(String),
}
/// Bounded result of an explicit remote folder inspection.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RemotePage {
    /// Exact destination inspected.
    pub target: CloudTarget,
    /// Current one-based page.
    pub page: u16,
    /// Number of pages including an empty page.
    pub pages: u16,
    /// Object identity and encrypted size.
    pub files: Vec<(String, u64)>,
    /// Available quota, when reported by the provider.
    pub free: Option<u64>,
}

impl RemotePage {
    pub(crate) fn selected_index(
        &self,
        selection: Option<&(CloudTarget, String)>,
    ) -> Option<usize> {
        let (target, id) = selection?;
        if *target != self.target {
            return None;
        }
        self.files.iter().position(|(file, _)| file == id)
    }
}

/// Confirmed cloud connection intention; sensitive values have no debug representation.
#[derive(Clone, PartialEq, Eq)]
pub enum CloudOperation {
    /// Browse, transfer or retrieve at the reviewed destination.
    Remote(CloudTarget, RemoteCommand),
    /// Send one authenticated snapshot to the explicitly marked destinations.
    SendGroup(Vec<CloudTarget>, String),
    /// Refresh available rclone providers.
    Providers,
    /// Begin a provider-generated configuration session.
    Connect {
        /// Human-readable name.
        name: String,
        /// Rclone provider name.
        provider: String,
    },
    /// Configure an encryption wrapper around a ready managed connection.
    Crypt {
        /// Human-readable name.
        name: String,
        /// Base connection identity.
        parent: String,
        /// Relative encrypted folder.
        folder: String,
    },
    /// Answer the current provider-generated question.
    Answer {
        /// Connection identity.
        id: String,
        /// Expected private-question revision.
        revision: String,
        /// Private answer.
        value: String,
        /// Whether rendering must mask the answer.
        secret: bool,
    },
    /// Send and remove a fictitious test file.
    Test {
        /// Connection identity.
        id: String,
        /// Relative destination folder.
        folder: String,
    },
    /// Create or edit a cloud plan.
    Plan {
        /// Existing ID, empty for a new plan.
        id: String,
        /// Unique plan name.
        name: String,
        /// Ready connection ID.
        connection: String,
        /// Relative remote folder.
        folder: String,
        /// Retention count.
        keep: String,
        /// Optional HH:MM schedule.
        time: String,
        /// IANA zone.
        zone: String,
    },
    /// Run a plan immediately.
    RunPlan {
        /// Plan identity.
        id: String,
    },
    /// Disable a plan's schedule.
    DisablePlan {
        /// Plan identity.
        id: String,
    },
    /// Remove a plan while preserving remote artifacts.
    RemovePlan {
        /// Plan identity.
        id: String,
    },
    /// Remove a connection without deleting backups.
    RemoveConnection {
        /// Connection identity.
        id: String,
    },
}
impl std::fmt::Debug for CloudOperation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("CloudOperation { .. }")
    }
}
impl CloudOperation {
    pub(crate) const fn key(&self) -> &'static str {
        match self {
            Self::SendGroup(..) => "cloud-send-group",
            Self::Remote(_, cmd) => match cmd {
                RemoteCommand::Create => "cloud-run-plan",
                RemoteCommand::Browse(_) => "cloud-browse",
                RemoteCommand::Send(_) => "cloud-send",
                RemoteCommand::Download(_) => "cloud-download",
            },
            Self::Plan { id, .. } if !id.is_empty() => "schedule-edit-plan",
            Self::Plan { .. } => "cloud-plan",
            Self::RunPlan { .. } => "cloud-run-plan",
            Self::DisablePlan { .. } => "cloud-disable-plan",
            Self::RemovePlan { .. } => "cloud-remove-plan",
            Self::RemoveConnection { .. } => "cloud-remove-connection",
            Self::Providers => "cloud-providers",
            Self::Connect { .. } => "cloud-connect",
            Self::Crypt { .. } => "cloud-crypt",
            Self::Answer { .. } => "cloud-answer-action",
            Self::Test { .. } => "cloud-test",
        }
    }
    pub(crate) fn fields(&self) -> (Vec<&'static str>, Vec<String>) {
        match self {
            Self::SendGroup(_, id) => (vec!["cloud-artifact"], vec![id.clone()]),
            Self::Remote(_, cmd) => match cmd {
                RemoteCommand::Create | RemoteCommand::Download(_) => (vec![], vec![]),
                RemoteCommand::Browse(page) => (vec!["cloud-page"], vec![page.to_string()]),
                RemoteCommand::Send(id) => (vec!["cloud-artifact"], vec![id.clone()]),
            },
            Self::Plan {
                id,
                name,
                keep,
                time,
                zone,
                ..
            } if !id.is_empty() => (
                vec!["cloud-plan-name", "op-keep", "op-time", "op-zone"],
                vec![name.clone(), keep.clone(), time.clone(), zone.clone()],
            ),
            Self::Plan {
                id,
                name,
                connection,
                folder,
                keep,
                time,
                zone,
            } => (
                vec![
                    "cloud-plan-id-optional",
                    "cloud-plan-name",
                    "cloud-id",
                    "cloud-folder",
                    "op-keep",
                    "op-time",
                    "op-zone",
                ],
                vec![
                    id.clone(),
                    name.clone(),
                    connection.clone(),
                    folder.clone(),
                    keep.clone(),
                    time.clone(),
                    zone.clone(),
                ],
            ),
            Self::RunPlan { id } | Self::DisablePlan { id } | Self::RemovePlan { id } => {
                (vec!["cloud-plan-id"], vec![id.clone()])
            }
            Self::RemoveConnection { id } => (vec!["cloud-id"], vec![id.clone()]),
            Self::Providers => (vec![], vec![]),
            Self::Connect { name, provider } => (
                vec!["cloud-name", "cloud-provider"],
                vec![name.clone(), provider.clone()],
            ),
            Self::Crypt {
                name,
                parent,
                folder,
            } => (
                vec!["cloud-name", "cloud-parent", "cloud-folder"],
                vec![name.clone(), parent.clone(), folder.clone()],
            ),
            Self::Answer { value, secret, .. } => (
                vec![if *secret {
                    "cloud-secret-answer"
                } else {
                    "cloud-answer"
                }],
                vec![value.clone()],
            ),
            Self::Test { id, folder } => (
                vec!["cloud-id", "cloud-folder"],
                vec![id.clone(), folder.clone()],
            ),
        }
    }
    pub(crate) fn finish(self, values: Vec<String>) -> Option<Self> {
        let mut values = values.into_iter();
        Some(match self {
            Self::SendGroup(targets, _) => Self::SendGroup(targets, values.next()?),
            Self::Remote(target, cmd) => Self::Remote(
                target,
                match cmd {
                    RemoteCommand::Create => RemoteCommand::Create,
                    RemoteCommand::Browse(_) => RemoteCommand::Browse(values.next()?.parse().ok()?),
                    RemoteCommand::Send(_) => RemoteCommand::Send(values.next()?),
                    RemoteCommand::Download(id) => RemoteCommand::Download(id),
                },
            ),
            Self::Plan {
                id,
                connection,
                folder,
                ..
            } if !id.is_empty() => Self::Plan {
                id,
                connection,
                folder,
                name: values.next()?,
                keep: values.next()?,
                time: values.next()?,
                zone: values.next()?,
            },
            Self::Plan { .. } => Self::Plan {
                id: values.next()?,
                name: values.next()?,
                connection: values.next()?,
                folder: values.next()?,
                keep: values.next()?,
                time: values.next()?,
                zone: values.next()?,
            },
            Self::RunPlan { .. } => Self::RunPlan { id: values.next()? },
            Self::DisablePlan { .. } => Self::DisablePlan { id: values.next()? },
            Self::RemovePlan { .. } => Self::RemovePlan { id: values.next()? },
            Self::RemoveConnection { .. } => Self::RemoveConnection { id: values.next()? },
            Self::Providers => Self::Providers,
            Self::Connect { .. } => Self::Connect {
                name: values.next()?,
                provider: values.next()?,
            },
            Self::Crypt { .. } => Self::Crypt {
                name: values.next()?,
                parent: values.next()?,
                folder: values.next()?,
            },
            Self::Answer {
                id,
                revision,
                secret,
                ..
            } => Self::Answer {
                id,
                revision,
                value: values.next()?,
                secret,
            },
            Self::Test { .. } => Self::Test {
                id: values.next()?,
                folder: values.next()?,
            },
        })
    }
}
/// Safe view of a configured or pending connection.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CloudConnection {
    /// Connection identity.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Provider name.
    pub provider: String,
    /// Whether configuration finished.
    pub ready: bool,
    /// Most recent test failed.
    pub test_failed: bool,
    /// Most recent completed fictitious upload test.
    pub tested_at: Option<String>,
}
/// Next provider question, without its opaque state or secret defaults.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CloudQuestion {
    /// Revision used to reject answers to a replaced question.
    pub revision: String,
    /// Connection identity.
    pub id: String,
    /// Provider field name.
    pub name: String,
    /// Static help from the provider catalog.
    pub help: String,
    /// Safe example values and descriptions.
    pub examples: Vec<(String, String)>,
    /// Default value, empty for sensitive fields.
    pub default: String,
    /// Whether the answer is private.
    pub secret: bool,
    /// Whether the preceding answer was refused.
    pub refused: bool,
}
/// Bounded cloud metadata; no credentials or provider state.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct CloudView {
    /// Most recent successful folder listing.
    pub remote: Option<RemotePage>,
    /// Transfers with their exact destination, identity and localized outcome.
    pub transfers: Vec<(CloudTarget, String, String)>,
    /// Private encrypted download available to the restore wizard.
    pub download: Option<(CloudTarget, std::path::PathBuf)>,
    /// Result of the most recent remote read.
    pub notice: Option<(CloudTarget, String)>,

    /// Provider names and static descriptions.
    pub providers: Vec<(String, String)>,
    /// Connection states.
    pub connections: Vec<CloudConnection>,
    /// Current configuration question.
    pub question: Option<CloudQuestion>,
    /// Configured cloud plans.
    pub plans: Vec<CloudPlan>,
}

/// Safe schedule and retention view of a cloud plan.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CloudPlan {
    /// Plan identity.
    pub id: String,
    /// Unique plan name.
    pub name: String,
    /// Connection identity.
    pub connection: String,
    /// Relative remote folder.
    pub folder: String,
    /// Retention count.
    pub keep: u16,
    /// Daily time and IANA zone, absent if disabled.
    pub daily: Option<String>,
    /// Verified copies still awaiting retention cleanup.
    pub retention_pending: bool,
}

impl CloudPlan {
    pub(crate) fn policy(&self) -> Option<CloudOperation> {
        let (time, zone) = match self.daily.as_deref() {
            Some(daily) => daily.split_once(' ')?,
            None => ("", "UTC"),
        };
        Some(CloudOperation::Plan {
            id: self.id.clone(),
            name: self.name.clone(),
            connection: self.connection.clone(),
            folder: self.folder.clone(),
            keep: self.keep.to_string(),
            time: time.into(),
            zone: zone.into(),
        })
    }

    pub(crate) fn target(&self) -> CloudTarget {
        CloudTarget {
            plan: self.id.clone(),
            connection: self.connection.clone(),
            folder: self.folder.clone(),
        }
    }
}

pub(crate) fn shortcut(key: char, view: Option<&CloudView>) -> Option<CloudOperation> {
    Some(match key {
        'g' => CloudOperation::Plan {
            id: String::new(),
            name: String::new(),
            connection: String::new(),
            folder: "Elzevir/backups".to_owned(),
            keep: "7".to_owned(),
            time: String::new(),
            zone: "UTC".to_owned(),
        },
        'u' => CloudOperation::RunPlan { id: String::new() },
        'h' => CloudOperation::DisablePlan { id: String::new() },
        'G' => CloudOperation::RemovePlan { id: String::new() },
        'C' => CloudOperation::RemoveConnection { id: String::new() },
        'p' => CloudOperation::Providers,
        'c' => CloudOperation::Connect {
            name: String::new(),
            provider: String::new(),
        },
        'y' => CloudOperation::Crypt {
            name: String::new(),
            parent: String::new(),
            folder: "Elzevir".to_owned(),
        },
        't' => CloudOperation::Test {
            id: String::new(),
            folder: "Elzevir/tests".to_owned(),
        },
        'a' => {
            let question = view?.question.as_ref()?;
            CloudOperation::Answer {
                id: question.id.clone(),
                revision: question.revision.clone(),
                value: question.default.clone(),
                secret: question.secret,
            }
        }
        _ => return None,
    })
}
