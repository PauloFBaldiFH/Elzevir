// SPDX-License-Identifier: GPL-3.0-or-later
//! Read-only public projection of the private broker identity and service observations.
use super::{ConnectorState, DomainPhase, DomainStatus, Path, load, location, observation};

pub(super) fn clear_binding(directory: &Path) -> Result<(), String> {
    // Remove only files owned by this integration; keep the identity until cleanup completes.
    for relative in [
        "secrets/cf-token.txt",
        "secrets/cf-config.json",
        "broker-observation.json",
        "secrets/broker.json",
    ] {
        let path = directory.join(relative);
        match std::fs::symlink_metadata(&path) {
            Ok(_) => {
                crate::backup_archive::regular(&path)?;
                std::fs::remove_file(&path)
                    .map_err(|_| "cannot remove revoked address configuration")?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("cannot inspect revoked address configuration".to_owned()),
        }
    }
    Ok(())
}

pub(super) fn projection(
    directory: &Path,
    connector: ConnectorState,
) -> Result<DomainStatus, String> {
    if !location(directory).exists() {
        return Ok(DomainStatus {
            broker_url: crate::broker_protocol::configured_url(None)?,
            domain: crate::broker_protocol::DOMAIN.to_owned(),
            phase: DomainPhase::NotRegistered,
            catalog: None,
            staff: None,
            failure: None,
            connector_at: jiff::Timestamp::now().to_string(),
            registration_at: None,
            poll_seconds: None,
            connector,
            staff_allow_cidrs: vec![],
            remote_status: None,
        });
    }
    let identity = load(directory)?;
    let expired = identity.library.is_none()
        && identity.device_code.is_some()
        && identity.expires_at <= jiff::Timestamp::now().as_second();
    let mut result = DomainStatus {
        broker_url: identity.base.clone(),
        domain: identity.domain.clone().unwrap_or_default(),
        phase: if expired {
            DomainPhase::Failed
        } else if identity.ready && identity.phase == DomainPhase::NotRegistered {
            DomainPhase::Ready
        } else {
            identity.phase
        },
        catalog: identity.hosts.as_ref().map(|hosts| hosts.opac.clone()),
        staff: identity.hosts.as_ref().map(|hosts| hosts.staff.clone()),
        failure: identity.failure.as_deref().map(|code| match code {
            "denied" => "broker-denied",
            "expired" => "broker-expired",
            "dns_conflict" => "domain-error-dns",
            "quota_exceeded" => "domain-error-quota",
            "domain_unavailable" => "domain-error-unavailable",
            "tunnel_failed" => "domain-error-tunnel",
            _ => "domain-error-provisioning",
        }),
        connector_at: jiff::Timestamp::now().to_string(),
        registration_at: std::fs::metadata(location(directory))
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|time| jiff::Timestamp::try_from(time).ok())
            .map(|time| time.to_string()),
        poll_seconds: identity
            .device_code
            .as_ref()
            .filter(|_| !expired)
            .map(|_| identity.interval),
        connector,
        staff_allow_cidrs: vec![],
        remote_status: None,
    };
    if expired {
        result.failure = Some("broker-expired");
    }
    if let Some(observed) = observation(directory, &identity)? {
        result.staff_allow_cidrs = observed.staff_allow_cidrs;
        result.remote_status = match observed.status.as_deref() {
            Some("active") => Some("domain-active"),
            Some("pending" | "provisioning") => Some("domain-provisioning"),
            Some("suspended" | "disabled" | "revoked") => Some("domain-inactive"),
            _ => None,
        };
    }
    Ok(result)
}

pub(crate) fn status(directory: &Path) -> Result<DomainStatus, String> {
    let connector = if location(directory).exists() {
        let name = crate::service::tunnel_name(directory)?;
        // Manager errors mean unknown state; they must never imply a running connector.
        match crate::service::native_status(&name)
            .ok()
            .as_ref()
            .and_then(|value| value.get("state"))
            .and_then(serde_json::Value::as_str)
        {
            Some("running") => ConnectorState::Running,
            Some("stopped" | "failed") => ConnectorState::Stopped,
            Some("not_installed") => ConnectorState::NotInstalled,
            Some("starting" | "stopping") => ConnectorState::Transitioning,
            _ => ConnectorState::Unavailable,
        }
    } else {
        ConnectorState::Unavailable
    };
    projection(directory, connector)
}

pub(super) fn set_connector(directory: &Path, start: bool) -> Result<(), String> {
    let name = crate::service::tunnel_name(directory)?;
    if start {
        let hosts = super::hosts(directory)?.ok_or("no active address binding")?;
        let expected = crate::access_config::StaffMode::Hostname {
            catalog_host: hosts.opac,
            staff_host: hosts.staff,
        };
        let observed = crate::status::inspect(directory);
        if observed.state != "running" || observed.staff.as_ref() != Some(&expected) {
            return Err(
                "start requires the institution server with its registered host separation".into(),
            );
        }
    }
    let observed = crate::service::native_status(&name)?;
    match (
        start,
        observed.get("state").and_then(serde_json::Value::as_str),
    ) {
        (true, Some("stopped" | "failed")) => crate::service::native_action("start", &name),
        (false, Some("running")) => crate::service::native_action("stop", &name),
        (true, Some("running")) | (false, Some("stopped")) => Ok(()),
        _ => Err("connector not installed or unavailable".into()),
    }
}
