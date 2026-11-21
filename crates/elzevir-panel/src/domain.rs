// SPDX-License-Identifier: GPL-3.0-or-later
//! Safe domain and connector observations, with no network or persistence.
use serde::{Deserialize, Serialize};
/// Last locally recorded stage of a domain registration.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DomainPhase {
    /// No enrollment has been recorded.
    #[default]
    NotRegistered,
    /// Waiting for approval by the broker.
    Pending,
    /// Sending the approved public signing key.
    Enrolling,
    /// Waiting for the remote provisioning job.
    Provisioning,
    /// Applying host separation and starting the connector.
    Connecting,
    /// Provisioning and the last connector action completed.
    Ready,
    /// The last operation failed; the identity is retained for retry.
    Failed,
}
impl DomainPhase {
    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::NotRegistered => "domain-not-registered",
            Self::Pending => "domain-pending",
            Self::Enrolling => "domain-enrolling",
            Self::Provisioning => "domain-provisioning",
            Self::Connecting => "domain-connecting",
            Self::Ready => "domain-ready",
            Self::Failed => "domain-failed",
        }
    }
}
/// Last observation from the local service manager, independent of public HTTPS tests.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorState {
    /// Manager confirmed execution.
    Running,
    /// Manager confirmed a stopped service.
    Stopped,
    /// Manager reports no service registration.
    NotInstalled,
    /// Manager reports a transition.
    Transitioning,
    /// No trustworthy service observation.
    #[default]
    Unavailable,
}
impl ConnectorState {
    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::Running => "domain-connector-running",
            Self::Stopped => "domain-connector-stopped",
            Self::NotInstalled => "domain-connector-absent",
            Self::Transitioning => "domain-connector-transitioning",
            Self::Unavailable => "panel-unknown",
        }
    }
}
/// Public projection of registration progress; contains no key, token or device code.
#[derive(Debug, Clone, Serialize)]
pub struct DomainStatus {
    /// Validated broker origin selected for this registration.
    pub broker_url: String,
    /// Requested public suffix.
    pub domain: String,
    /// Last registration stage.
    pub phase: DomainPhase,
    /// Assigned public hostname, not proof of availability.
    pub catalog: Option<String>,
    /// Assigned remote staff hostname.
    pub staff: Option<String>,
    /// Classified Fluent error key; never a remote error payload.
    pub failure: Option<&'static str>,
    /// Time when the local service state was sampled.
    pub connector_at: String,
    /// Broker-selected device polling interval while authorization is outstanding.
    pub poll_seconds: Option<u32>,
    /// Time the persisted registration stage was last saved.
    pub registration_at: Option<String>,
    /// Local connector service observation.
    pub connector: ConnectorState,
    /// Last remotely observed staff restrictions.
    pub staff_allow_cidrs: Vec<String>,
    /// Classified translation key for the latest remote status, when recognized.
    pub remote_status: Option<&'static str>,
}
