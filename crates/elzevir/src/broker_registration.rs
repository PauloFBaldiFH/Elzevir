// SPDX-License-Identifier: GPL-3.0-or-later
//! Resumable device flow. Only the approved public key leaves the private identity.
use super::*;

#[derive(Deserialize)]
struct Authorization {
    device_code: String,
    #[serde(default)]
    verification_uri: Option<String>,
    domain: String,
    status: String,
    #[serde(default)]
    hostnames: Option<Hosts>,
    expires_in: u32,
    interval: u32,
}
#[derive(Deserialize)]
struct Poll {
    status: String,
    #[serde(default)]
    interval: Option<u32>,
}
#[derive(Deserialize)]
struct Enrolled {
    library_id: String,
    job_id: String,
    domain: String,
    hostnames: Hosts,
}

fn new_identity(base: String, origin: String) -> Result<Identity, String> {
    use ring::rand::{SecureRandom as _, SystemRandom};
    let mut seed = zeroize::Zeroizing::new([0_u8; 32]);
    SystemRandom::new()
        .fill(seed.as_mut())
        .map_err(|_| "cannot generate address signing key")?;
    Ok(Identity {
        schema: 1,
        base,
        seed: base64::engine::general_purpose::STANDARD.encode(*seed),
        origin,
        device_code: None,
        verification_uri: None,
        expires_at: 0,
        library: None,
        job: None,
        hosts: None,
        ready: false,
        domain: Some(crate::broker_protocol::DOMAIN.to_owned()),
        approved: false,
        interval: default_interval(),
        phase: DomainPhase::Pending,
        failure: None,
    })
}

pub(super) fn begin(
    directory: &Path,
    setup: &Setup,
    settings: &crate::server_config::Settings,
    transport: &impl Transport,
    clock: &impl FlowClock,
) -> Result<Identity, String> {
    let Action::Enroll {
        base,
        institution,
        email,
        name,
    } = &setup.action
    else {
        return Err("address enrollment action required".to_owned());
    };
    validate_input(institution, email, name)?;
    let base = crate::broker_protocol::origin(base)?;
    let origin = origin(settings)?;
    let mut identity = if location(directory).exists() {
        let current = load(directory)?;
        if current.base != base || current.origin != origin {
            return Err(
                "existing address identity belongs to a different broker or origin".to_owned(),
            );
        }
        current
    } else {
        new_identity(base, origin)?
    };
    if identity.library.is_some()
        || (identity.device_code.is_some() && identity.expires_at > clock.now())
    {
        return Ok(identity);
    }
    identity.domain = Some(crate::broker_protocol::DOMAIN.to_owned());
    identity.approved = false;
    identity.hosts = None;
    identity.failure = None;
    identity.phase = DomainPhase::Pending;
    // Persist the key before the request: a lost response must never replace it.
    save(directory, &identity)?;
    let mut body = serde_json::json!({"institution_name":institution,"contact_email":email,
        "domain":crate::broker_protocol::DOMAIN});
    if !name.is_empty() {
        body.as_object_mut()
            .ok_or("invalid device request")?
            .insert(
                "requested_name".to_owned(),
                serde_json::Value::String(name.clone()),
            );
    }
    let bytes = serde_json::to_vec(&body).map_err(|_| "cannot encode device request")?;
    let mut authorization: Authorization = transport
        .request(&identity.base, "POST", "/v1/device/start", &[], &bytes)?
        .json()?;
    if authorization.domain != crate::broker_protocol::DOMAIN
        || !matches!(authorization.status.as_str(), "approved" | "pending")
        || !(1..=300).contains(&authorization.interval)
        || !(1..=86_400).contains(&authorization.expires_in)
        || authorization.device_code.is_empty()
        || authorization.device_code.len() > 64
    {
        return Err("invalid broker device authorization".to_owned());
    }
    if let Some(uri) = &authorization.verification_uri {
        verification_url(&identity.base, uri)?;
    }
    if let Some(hosts) = &mut authorization.hostnames {
        hosts.validate_for(Some(&authorization.domain))?;
    }
    identity.device_code = Some(authorization.device_code);
    identity.verification_uri = authorization.verification_uri;
    identity.expires_at = clock
        .now()
        .saturating_add(i64::from(authorization.expires_in));
    identity.approved = authorization.status == "approved";
    identity.interval = authorization.interval;
    identity.hosts = authorization.hostnames;
    identity.phase = if identity.approved {
        DomainPhase::Enrolling
    } else {
        DomainPhase::Pending
    };
    save(directory, &identity)?;
    Ok(identity)
}
fn validate_input(institution: &str, email: &str, name: &str) -> Result<(), String> {
    if !(1..=200).contains(&institution.chars().count())
        || institution.chars().any(char::is_control)
        || !(3..=254).contains(&email.len())
        || !email.contains('@')
        || email.chars().any(char::is_whitespace)
        || (!name.is_empty() && !valid_slug(name))
    {
        return Err("invalid institution, contact email or requested slug".to_owned());
    }
    Ok(())
}

pub(super) fn enroll(
    directory: &Path,
    identity: &mut Identity,
    transport: &impl Transport,
    progress: Option<&crate::process_runner::Progress>,
    clock: &impl FlowClock,
) -> Result<(), String> {
    if identity.library.is_some() {
        return Ok(());
    }
    if !identity.approved {
        if let (Some(progress), Some(uri)) = (progress, &identity.verification_uri) {
            progress(uri.as_bytes())?;
        }
        approve(directory, identity, transport, clock)?;
    }
    identity.phase = DomainPhase::Enrolling;
    save(directory, identity)?;
    let body = serde_json::to_vec(&serde_json::json!({
        "device_code":identity.device_code.as_deref().ok_or("device authorization required")?,
        "public_key":identity.key()?.public(),
    }))
    .map_err(|_| "cannot encode address enrollment")?;
    let mut enrolled: Enrolled = transport
        .request(&identity.base, "POST", "/v1/enroll", &[], &body)?
        .json()?;
    if enrolled.domain != crate::broker_protocol::DOMAIN
        || !identifier(&enrolled.library_id)
        || !identifier(&enrolled.job_id)
    {
        return Err("invalid address enrollment result".to_owned());
    }
    enrolled.hostnames.validate_for(Some(&enrolled.domain))?;
    identity.library = Some(enrolled.library_id);
    identity.job = Some(enrolled.job_id);
    identity.hosts = Some(enrolled.hostnames);
    identity.device_code = None;
    identity.verification_uri = None;
    identity.approved = false;
    identity.phase = DomainPhase::Provisioning;
    save(directory, identity)
}
fn approve(
    directory: &Path,
    identity: &mut Identity,
    transport: &impl Transport,
    clock: &impl FlowClock,
) -> Result<(), String> {
    let deadline = identity.expires_at.min(clock.now().saturating_add(900));
    while clock.now() < deadline {
        if clock.now().saturating_add(i64::from(identity.interval)) >= deadline {
            break;
        }
        clock.wait(identity.interval);
        let body = serde_json::to_vec(&serde_json::json!({"device_code":identity.device_code.as_deref().ok_or("device authorization required")?}))
            .map_err(|_| "cannot encode device poll")?;
        let poll: Poll = transport
            .request(&identity.base, "POST", "/v1/device/poll", &[], &body)?
            .json()?;
        match poll.status.as_str() {
            "approved" | "consumed" => {
                identity.approved = true;
                return Ok(());
            }
            "pending" => {
                if let Some(interval) = poll.interval {
                    if !(1..=300).contains(&interval) {
                        return Err("invalid device poll interval".to_owned());
                    }
                    identity.interval = interval;
                    save(directory, identity)?;
                }
            }
            "denied" | "expired" => {
                identity.device_code = None;
                identity.verification_uri = None;
                identity.phase = DomainPhase::Failed;
                save(directory, identity)?;
                return Err(
                    "device authorization denied or expired; register again to retry".to_owned(),
                );
            }
            _ => return Err("invalid device authorization status".to_owned()),
        }
    }
    Err("device authorization still pending or expired; retry to resume".to_owned())
}
