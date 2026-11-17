// SPDX-License-Identifier: GPL-3.0-or-later
//! Device enrollment and signed address management; credentials never enter argv.
use crate::broker_protocol::{Http, Key, Response, Transport};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Identity {
    schema: u8,
    base: String,
    seed: String,
    origin: String,
    device_code: Option<String>,
    verification_uri: Option<String>,
    expires_at: i64,
    library: Option<String>,
    job: Option<String>,
    hosts: Option<Hosts>,
    ready: bool,
}
impl Drop for Identity {
    fn drop(&mut self) {
        use zeroize::Zeroize as _;
        self.seed.zeroize();
        self.device_code.zeroize();
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Hosts {
    pub opac: String,
    pub staff: String,
}
impl Hosts {
    fn validate(&mut self) -> Result<(), String> {
        self.opac = elzevir_web::normalize_hostname(&self.opac)
            .map_err(|_| "invalid broker catalog hostname")?;
        self.staff = elzevir_web::normalize_hostname(&self.staff)
            .map_err(|_| "invalid broker staff hostname")?;
        if self.opac == self.staff {
            return Err("broker must isolate staff by hostname".to_owned());
        }
        Ok(())
    }
}
fn read(path: &Path) -> Result<zeroize::Zeroizing<Vec<u8>>, String> {
    if crate::backup_archive::regular(path)? > 16_384 {
        return Err("address identity exceeds limit".to_owned());
    }
    let mut bytes = zeroize::Zeroizing::new(Vec::new());
    std::fs::File::open(path)
        .and_then(|file| file.take(16_385).read_to_end(&mut bytes))
        .map_err(|_| "address identity unavailable")?;
    if bytes.len() > 16_384 {
        return Err("address identity exceeds limit".to_owned());
    }
    Ok(bytes)
}
fn location(directory: &Path) -> PathBuf {
    directory.join("secrets/broker.json")
}
fn load(directory: &Path) -> Result<Identity, String> {
    let identity: Identity = serde_json::from_slice(&read(&location(directory))?)
        .map_err(|_| "invalid address identity")?;
    identity.validate()?;
    Ok(identity)
}
impl Identity {
    fn validate(&self) -> Result<(), String> {
        if self.schema != 1
            || crate::broker_protocol::origin(&self.base)? != self.base
            || self
                .device_code
                .as_ref()
                .is_some_and(|value| value.is_empty() || value.len() > 64)
            || self
                .library
                .as_ref()
                .is_some_and(|value| !identifier(value))
            || self.job.as_ref().is_some_and(|value| !identifier(value))
        {
            return Err("invalid address identity".to_owned());
        }
        self.key()?;
        if let Some(hosts) = &self.hosts {
            hosts.clone().validate()?;
        }
        if self.ready && (self.library.is_none() || self.hosts.is_none()) {
            return Err("incomplete address identity".to_owned());
        }
        if let Some(uri) = &self.verification_uri {
            verification_url(&self.base, uri)?;
        }
        Ok(())
    }
    fn key(&self) -> Result<Key, String> {
        let seed = zeroize::Zeroizing::new(
            base64::engine::general_purpose::STANDARD
                .decode(&self.seed)
                .map_err(|_| "invalid address signing key")?,
        );
        Key::from_seed(
            seed.as_slice()
                .try_into()
                .map_err(|_| "invalid address signing key")?,
        )
    }
    fn signed(
        &self,
        transport: &impl Transport,
        method: &str,
        path: &str,
        body: &[u8],
    ) -> Result<Response, String> {
        let headers = self.key()?.headers(
            self.library
                .as_deref()
                .ok_or("address enrollment incomplete")?,
            method,
            path,
            body,
            jiff::Timestamp::now().as_second(),
            &crate::backup_archive::identifier()?,
        )?;
        transport.request(&self.base, method, path, &headers, body)
    }
}
fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}
fn save(directory: &Path, identity: &Identity) -> Result<(), String> {
    identity.validate()?;
    let secrets = directory.join("secrets");
    crate::backup_cli::make_directory(&secrets)?;
    let bytes = zeroize::Zeroizing::new(
        serde_json::to_vec(identity).map_err(|_| "cannot encode address identity")?,
    );
    publish(directory, &location(directory), &bytes)
}
pub(crate) fn publish(directory: &Path, path: &Path, bytes: &[u8]) -> Result<(), String> {
    if path.exists() {
        crate::backup_archive::regular(path)?;
    }
    let mut file =
        tempfile::NamedTempFile::new_in(path.parent().ok_or("private file needs parent")?)
            .map_err(|_| "cannot prepare private configuration")?;
    file.write_all(bytes)
        .and_then(|()| file.as_file().sync_all())
        .map_err(|_| "cannot save private configuration")?;
    crate::file_owner::private_tree(file.path(), directory)?;
    file.persist(path)
        .map_err(|_| "cannot publish private configuration")?;
    Ok(())
}
fn verification_url(base: &str, uri: &str) -> Result<(), String> {
    let code = uri
        .strip_prefix(base)
        .and_then(|value| value.strip_prefix("/join?c="))
        .ok_or("invalid broker verification URL")?;
    if !(4..=32).contains(&code.len())
        || !code
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte == b'-')
    {
        return Err("invalid broker verification URL".to_owned());
    }
    Ok(())
}
pub(crate) fn authorization_url(directory: &Path, bytes: &[u8]) -> Option<String> {
    let uri = std::str::from_utf8(bytes).ok()?;
    let identity = load(directory).ok()?;
    verification_url(&identity.base, uri).ok()?;
    Some(uri.to_owned())
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Action {
    Enroll {
        base: String,
        institution: String,
        email: String,
        name: String,
    },
    Refresh,
    Listing {
        public: bool,
    },
    Staff {
        username: String,
        password: String,
        cidrs: Vec<String>,
    },
    DisableStaff,
    Renew,
    Withdraw,
    Export {
        output: PathBuf,
    },
    Recover {
        input: PathBuf,
    },
}
impl std::fmt::Debug for Action {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("BrokerAction { .. }")
    }
}
impl Drop for Action {
    fn drop(&mut self) {
        if let Self::Staff { password, .. } = self {
            use zeroize::Zeroize as _;
            password.zeroize();
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Setup {
    pub config: PathBuf,
    pub service: String,
    pub action: Action,
}

fn check_capabilities(transport: &impl Transport, base: &str) -> Result<(), String> {
    let response = transport.request(base, "GET", "/v1/capabilities", &[], &[])?;
    if response.status != 200 {
        return Err("broker capabilities unavailable".to_owned());
    }
    crate::broker_protocol::capabilities(&response.body)
}
fn origin(settings: &crate::server_config::Settings) -> Result<String, String> {
    let mut address = settings.listen;
    if address.port() == 0 {
        return Err("address requires a fixed origin port".to_owned());
    }
    if address.ip().is_unspecified() {
        address.set_ip(if address.is_ipv6() {
            std::net::Ipv6Addr::LOCALHOST.into()
        } else {
            std::net::Ipv4Addr::LOCALHOST.into()
        });
    }
    if !address.ip().is_loopback() {
        return Err("free address requires a loopback or wildcard listener".to_owned());
    }
    Ok(format!("http://{address}"))
}
#[derive(Deserialize)]
struct Authorization {
    device_code: String,
    verification_uri: String,
    expires_in: u32,
    interval: u32,
}
#[derive(Deserialize)]
struct Poll {
    status: String,
    interval: Option<u32>,
}
#[derive(Deserialize)]
struct Enrolled {
    library_id: String,
    job_id: String,
    hostnames: Hosts,
}
fn begin(
    directory: &Path,
    setup: &Setup,
    settings: &crate::server_config::Settings,
    transport: &impl Transport,
) -> Result<Identity, String> {
    let Action::Enroll {
        base,
        institution,
        email,
        name,
    } = &setup.action
    else {
        return Err("invalid enrollment action".to_owned());
    };
    let base = crate::broker_protocol::origin(base)?;
    check_capabilities(transport, &base)?;
    if institution.trim().len() < 3
        || institution.len() > 200
        || email.len() > 254
        || !email.contains('@')
        || name.len() > 100
        || [institution, email, name]
            .iter()
            .any(|value| value.chars().any(char::is_control))
    {
        return Err("invalid institution or contact".to_owned());
    }
    let mut identity = if location(directory).exists() {
        let identity = load(directory)?;
        if identity.base != base || identity.origin != origin(settings)? {
            return Err("existing address identity must be reviewed before replacement".to_owned());
        }
        if identity.library.is_some()
            || (identity.device_code.is_some()
                && identity.expires_at > jiff::Timestamp::now().as_second())
        {
            return Ok(identity);
        }
        identity
    } else {
        let mut seed = zeroize::Zeroizing::new([0_u8; 32]);
        getrandom::fill(seed.as_mut()).map_err(|_| "cannot generate address signing key")?;
        Identity {
            schema: 1,
            base,
            seed: base64::engine::general_purpose::STANDARD.encode(seed.as_ref()),
            origin: origin(settings)?,
            device_code: None,
            verification_uri: None,
            expires_at: 0,
            library: None,
            job: None,
            hosts: None,
            ready: false,
        }
    };
    save(directory, &identity)?;
    let body = serde_json::to_vec(&serde_json::json!({"institution_name":institution,"contact_email":email,"requested_name":name,"services":["elzevir"],"origin":identity.origin})).map_err(|_| "cannot encode enrollment")?;
    let device: Authorization = transport
        .request(&identity.base, "POST", "/v1/device/start", &[], &body)?
        .json()?;
    if device.device_code.is_empty()
        || device.device_code.len() > 64
        || !(1..=86_400).contains(&device.expires_in)
        || !(1..=60).contains(&device.interval)
    {
        return Err("invalid enrollment response".to_owned());
    }
    verification_url(&identity.base, &device.verification_uri)?;
    identity.device_code = Some(device.device_code);
    identity.verification_uri = Some(device.verification_uri);
    identity.expires_at = jiff::Timestamp::now()
        .as_second()
        .saturating_add(i64::from(device.expires_in));
    save(directory, &identity)?;
    Ok(identity)
}
fn enroll(
    directory: &Path,
    identity: &mut Identity,
    transport: &impl Transport,
    progress: Option<&crate::process_runner::Progress>,
) -> Result<(), String> {
    if identity.library.is_some() {
        return Ok(());
    }
    if let (Some(progress), Some(uri)) = (progress, &identity.verification_uri) {
        progress(uri.as_bytes())?;
    }
    let started = Instant::now();
    loop {
        if identity.expires_at <= jiff::Timestamp::now().as_second()
            || started.elapsed() > Duration::from_mins(15)
        {
            return Err("address authorization expired; retry enrollment after review".to_owned());
        }
        let device = identity.device_code.as_ref().ok_or("enrollment was interrupted before its device code was saved; review the private identity before retrying")?;
        let body = zeroize::Zeroizing::new(
            serde_json::to_vec(&serde_json::json!({"device_code":device}))
                .map_err(|_| "cannot encode enrollment poll")?,
        );
        let poll: Poll = transport
            .request(&identity.base, "POST", "/v1/device/poll", &[], &body)?
            .json()?;
        match poll.status.as_str() {
            "approved" | "consumed" => break,
            "pending" => std::thread::sleep(Duration::from_secs(u64::from(
                poll.interval.unwrap_or(10).clamp(1, 60),
            ))),
            _ => {
                return Err("address authorization was denied or expired".to_owned());
            }
        }
    }
    let body = zeroize::Zeroizing::new(serde_json::to_vec(&serde_json::json!({"device_code":identity.device_code,"public_key":identity.key()?.public()})).map_err(|_| "cannot encode enrollment")?);
    let mut enrolled: Enrolled = transport
        .request(&identity.base, "POST", "/v1/enroll", &[], &body)?
        .json()?;
    enrolled.hostnames.validate()?;
    if !identifier(&enrolled.library_id) || !identifier(&enrolled.job_id) {
        return Err("invalid enrollment identity".to_owned());
    }
    identity.library = Some(enrolled.library_id);
    identity.job = Some(enrolled.job_id);
    identity.hosts = Some(enrolled.hostnames);
    identity.device_code = None;
    identity.verification_uri = None;
    save(directory, identity)
}

#[derive(Deserialize)]
struct Library {
    id: String,
    status: String,
    services: Vec<String>,
    hostnames: Hosts,
    public_listing: bool,
    staff_remote_access: bool,
}
#[derive(Deserialize)]
struct Provision {
    state: String,
}
fn provisioned(identity: &Identity, transport: &impl Transport) -> Result<(), String> {
    let Some(job) = &identity.job else {
        return Ok(());
    };
    let started = Instant::now();
    while started.elapsed() < Duration::from_mins(10) {
        let result: Provision = identity
            .signed(transport, "GET", &format!("/v1/jobs/{job}"), &[])?
            .json()?;
        match result.state.as_str() {
            "done" | "succeeded" => return Ok(()),
            "queued" | "running" => std::thread::sleep(Duration::from_secs(5)),
            _ => {
                return Err(
                    "address provisioning failed; the private identity was preserved".to_owned(),
                );
            }
        }
    }
    Err("address provisioning still pending; retry to resume".to_owned())
}
fn library(identity: &Identity, transport: &impl Transport) -> Result<Library, String> {
    let mut info: Library = identity
        .signed(transport, "GET", "/v1/library", &[])?
        .json()?;
    info.hostnames.validate()?;
    if Some(info.id.as_str()) != identity.library.as_deref()
        || !info.services.iter().any(|value| value == "elzevir")
        || info.status != "active"
    {
        return Err("address service did not confirm an active Elzevir institution".to_owned());
    }
    Ok(info)
}
#[derive(Deserialize)]
struct Token {
    tunnel_token: String,
}
impl Drop for Token {
    fn drop(&mut self) {
        use zeroize::Zeroize as _;
        self.tunnel_token.zeroize();
    }
}
trait Connector {
    fn connect(
        &self,
        directory: &Path,
        setup: &Setup,
        hosts: &Hosts,
        token: &str,
    ) -> Result<(), String>;
    fn stop(&self, directory: &Path) -> Result<(), String>;
}
struct NativeConnector;
impl Connector for NativeConnector {
    fn connect(
        &self,
        directory: &Path,
        setup: &Setup,
        hosts: &Hosts,
        token: &str,
    ) -> Result<(), String> {
        crate::managed_tools::ensure(crate::managed_tools::Tool::Cloudflared)?;
        self.stop(directory)?;
        let token_path = directory.join("secrets/cf-token.txt");
        publish(directory, &token_path, token.as_bytes())?;
        let logs = directory.join("logs/cloudflared");
        for path in [directory.join("logs"), logs.clone()] {
            let new = !path.exists();
            crate::backup_cli::make_directory(&path)?;
            if new {
                crate::file_owner::private_tree(&path, directory)?;
            }
        }
        let connector = serde_json::json!({"token-file":token_path,"protocol":"auto","logfile":logs.join("events.jsonl"),"loglevel":"info","logformat":"json"});
        let config = directory.join("secrets/cf-config.json");
        publish(
            directory,
            &config,
            &serde_json::to_vec(&connector).map_err(|_| "cannot encode remote connector")?,
        )?;
        let original = read(&setup.config)?;
        let original =
            std::str::from_utf8(&original).map_err(|_| "invalid server configuration")?;
        let text = crate::server_config::with_hosts(original, &hosts.opac, &hosts.staff)?;
        let mut file = tempfile::NamedTempFile::new_in(
            setup
                .config
                .parent()
                .ok_or("server configuration parent required")?,
        )
        .map_err(|_| "cannot stage server configuration")?;
        file.write_all(text.as_bytes())
            .and_then(|()| file.as_file().sync_all())
            .map_err(|_| "cannot save server configuration")?;
        crate::file_owner::configuration(file.path(), &setup.config)?;
        file.persist(&setup.config)
            .map_err(|_| "cannot publish server configuration")?;
        crate::service::native_action("restart", &setup.service)?;
        crate::host_jobs::wait_healthy(directory)?;
        let observed = crate::status::inspect(directory);
        let expected = crate::access_config::StaffMode::Hostname {
            catalog_host: hosts.opac.clone(),
            staff_host: hosts.staff.clone(),
        };
        if observed.state != "running" || observed.staff.as_ref() != Some(&expected) {
            return Err("server did not confirm required public host separation".to_owned());
        }
        let name = crate::service::install_tunnel(directory, &config)?;
        crate::service::native_action("start", &name)
    }
    fn stop(&self, directory: &Path) -> Result<(), String> {
        let name = crate::service::tunnel_name(directory)?;
        let status = crate::service::native_status(&name)?;
        match status.get("state").and_then(serde_json::Value::as_str) {
            Some("not_installed" | "stopped") => Ok(()),
            Some("running" | "starting" | "stopping" | "failed") => {
                crate::service::native_action("stop", &name)
            }
            _ => Err("cannot observe managed tunnel service".to_owned()),
        }
    }
}
fn connect(
    directory: &Path,
    setup: &Setup,
    identity: &mut Identity,
    transport: &impl Transport,
    connector: &impl Connector,
) -> Result<(), String> {
    provisioned(identity, transport)?;
    let info = library(identity, transport)?;
    identity.hosts = Some(info.hostnames.clone());
    let token: Token = identity
        .signed(transport, "GET", "/v1/tunnel-token", &[])?
        .json()?;
    if !(32..=16_384).contains(&token.tunnel_token.len())
        || !token.tunnel_token.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'=' | b'+' | b'/' | b'_' | b'-')
        })
    {
        return Err("invalid remote tunnel token".to_owned());
    }
    identity.ready = true;
    identity.job = None;
    save(directory, identity)?;
    let result = connector.connect(directory, setup, &info.hostnames, &token.tunnel_token);
    if result.is_err() {
        connector.stop(directory)?;
    }
    result
}
pub(crate) fn execute(
    directory: &Path,
    setup: &Setup,
    progress: Option<&crate::process_runner::Progress>,
) -> Result<(), String> {
    crate::service::require_operator()?;
    execute_with(directory, setup, &Http, &NativeConnector, progress)
}
fn execute_with(
    directory: &Path,
    setup: &Setup,
    transport: &impl Transport,
    connector: &impl Connector,
    progress: Option<&crate::process_runner::Progress>,
) -> Result<(), String> {
    if !crate::service::valid_name(&setup.service) {
        return Err("invalid address service name".to_owned());
    }
    let settings = crate::server_config::ServeArgs::for_status(Some(setup.config.clone()), None)
        .resolve()
        .map_err(|_| "address server configuration unavailable")?;
    if settings
        .data_dir
        .canonicalize()
        .map_err(|_| "institution unavailable")?
        != directory
            .canonicalize()
            .map_err(|_| "institution unavailable")?
        || directory.join("secrets/cf-intent.json").exists()
    {
        return Err(
            "address requires this institution without an existing own-domain tunnel".to_owned(),
        );
    }
    let mut identity = selected_identity(directory, setup, &settings, transport)?;
    if identity.origin != origin(&settings)? {
        return Err("configured origin differs from the enrolled origin".to_owned());
    }
    match &setup.action {
        Action::Enroll { .. } => {
            enroll(directory, &mut identity, transport, progress)?;
            connect(directory, setup, &mut identity, transport, connector)?;
        }
        Action::Recover { .. } => connect(directory, setup, &mut identity, transport, connector)?,
        Action::Export { output } => {
            crate::backup_archive::write_new(
                output,
                &zeroize::Zeroizing::new(
                    serde_json::to_vec(&identity)
                        .map_err(|_| "cannot encode address recovery file")?,
                ),
            )?;
        }
        Action::Renew => {
            identity
                .signed(transport, "POST", "/v1/rotate", b"{}")?
                .json::<serde_json::Value>()?;
            connect(directory, setup, &mut identity, transport, connector)?;
        }
        Action::Withdraw => {
            connector.stop(directory)?;
            identity
                .signed(transport, "DELETE", "/v1/library", &[])?
                .json::<serde_json::Value>()?;
            identity.ready = false;
            save(directory, &identity)?;
            if directory.join("secrets/cf-token.txt").exists() {
                std::fs::remove_file(directory.join("secrets/cf-token.txt"))
                    .map_err(|_| "cannot remove revoked connector token")?;
            }
        }
        Action::Listing { public } => {
            let bytes = serde_json::to_vec(&serde_json::json!({"public":public}))
                .map_err(|_| "cannot encode listing request")?;
            identity
                .signed(transport, "PUT", "/v1/listing", &bytes)?
                .json::<serde_json::Value>()?;
        }
        Action::Staff {
            username,
            password,
            cidrs,
        } => {
            validate_staff(username, password, cidrs)?;
            let body = zeroize::Zeroizing::new(serde_json::to_vec(&serde_json::json!({"username":username,"password":password,"allow_cidrs":cidrs})).map_err(|_| "cannot encode staff access request")?);
            identity
                .signed(transport, "PUT", "/v1/staff-credentials", &body)?
                .json::<serde_json::Value>()?;
        }
        Action::DisableStaff => {
            identity
                .signed(transport, "DELETE", "/v1/staff-credentials", &[])?
                .json::<serde_json::Value>()?;
        }
        Action::Refresh => {
            library(&identity, transport)?;
        }
    }
    if !matches!(&setup.action, Action::Withdraw | Action::Export { .. }) {
        let info = library(&identity, transport)?;
        let view = serde_json::json!({"schema":1,"at":jiff::Timestamp::now().to_string(),"catalog":info.hostnames.opac,"staff":info.hostnames.staff,"listed":info.public_listing,"staff_remote":info.staff_remote_access});
        publish(
            directory,
            &directory.join("broker-observation.json"),
            &serde_json::to_vec(&view).map_err(|_| "cannot encode address observation")?,
        )?;
    }
    Ok(())
}
fn selected_identity(
    directory: &Path,
    setup: &Setup,
    settings: &crate::server_config::Settings,
    transport: &impl Transport,
) -> Result<Identity, String> {
    match &setup.action {
        Action::Enroll { .. } => begin(directory, setup, settings, transport),
        Action::Recover { input } => {
            let identity: Identity = serde_json::from_slice(&read(input)?)
                .map_err(|_| "invalid address recovery file")?;
            identity.validate()?;
            if location(directory).exists() {
                let current = load(directory)?;
                if current.base != identity.base
                    || current.library != identity.library
                    || current.key()?.public() != identity.key()?.public()
                {
                    return Err("existing address identity differs from the recovery file; it was preserved".to_owned());
                }
            }
            check_capabilities(transport, &identity.base)?;
            library(&identity, transport)?;
            if identity.origin != origin(settings)? {
                return Err("recovery requires the original loopback origin port; review server configuration".to_owned());
            }
            save(directory, &identity)?;
            identity
                .signed(transport, "POST", "/v1/rotate", b"{}")?
                .json::<serde_json::Value>()?;
            Ok(identity)
        }
        _ => load(directory),
    }
}

fn validate_staff(username: &str, password: &str, cidrs: &[String]) -> Result<(), String> {
    if !(3..=64).contains(&username.len())
        || !username
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'@' | b'-'))
        || !(12..=256).contains(&password.chars().count())
        || password.chars().any(char::is_control)
        || cidrs.len() > 20
        || cidrs
            .iter()
            .any(|value| value.parse::<ipnet::IpNet>().is_err())
    {
        return Err("invalid remote staff credentials or network ranges".to_owned());
    }
    Ok(())
}
pub(crate) fn heartbeat(directory: &Path) -> Result<(), String> {
    if !location(directory).exists() {
        return Ok(());
    }
    let identity = load(directory)?;
    if !identity.ready {
        return Ok(());
    }
    let body =
        serde_json::to_vec(&serde_json::json!({"installer_version":env!("CARGO_PKG_VERSION")}))
            .map_err(|_| "cannot encode address heartbeat")?;
    identity
        .signed(&Http, "POST", "/v1/heartbeat", &body)?
        .json::<serde_json::Value>()?;
    Ok(())
}
pub(crate) fn hosts(directory: &Path) -> Result<Option<Hosts>, String> {
    if !location(directory).exists() {
        return Ok(None);
    }
    let identity = load(directory)?;
    Ok(if identity.ready {
        identity.hosts.clone()
    } else {
        None
    })
}
pub(crate) fn require_host_policy(settings: &crate::server_config::Settings) -> Result<(), String> {
    if let Some(hosts) = hosts(&settings.data_dir)?
        && settings.staff
            != (crate::access_config::StaffMode::Hostname {
                catalog_host: hosts.opac,
                staff_host: hosts.staff,
            })
    {
        return Err(
            "free address requires the configured catalog and staff hostname policy".to_owned(),
        );
    }
    Ok(())
}

pub(crate) fn panel_setup(
    config: &Path,
    operation: &elzevir_panel::AddressOperation,
) -> Result<Setup, String> {
    use elzevir_panel::AddressOperation as Panel;
    let mut service = "elzevir".to_owned();
    let action = match operation {
        Panel::Enroll {
            base,
            institution,
            email,
            name,
            service: name_service,
        } => {
            service.clone_from(name_service);
            Action::Enroll {
                base: base.clone(),
                institution: institution.clone(),
                email: email.clone(),
                name: name.clone(),
            }
        }
        Panel::Refresh => Action::Refresh,
        Panel::Listing { public } => Action::Listing { public: *public },
        Panel::Staff {
            username,
            password,
            cidrs,
        } => Action::Staff {
            username: username.clone(),
            password: password.clone(),
            cidrs: cidrs
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .collect(),
        },
        Panel::DisableStaff => Action::DisableStaff,
        Panel::Renew {
            service: name_service,
        } => {
            service.clone_from(name_service);
            Action::Renew
        }
        Panel::Withdraw => Action::Withdraw,
        Panel::Export { output } => Action::Export {
            output: output.into(),
        },
        Panel::Recover {
            input,
            service: name_service,
        } => {
            service.clone_from(name_service);
            Action::Recover {
                input: input.into(),
            }
        }
    };
    Ok(Setup {
        config: config
            .canonicalize()
            .map_err(|_| "server configuration unavailable")?,
        service,
        action,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Observation {
    schema: u8,
    at: String,
    catalog: String,
    staff: String,
    listed: bool,
    staff_remote: bool,
}
pub(crate) fn observe(directory: &Path) -> Result<Option<elzevir_panel::RemoteAddress>, String> {
    let Some(hosts) = hosts(directory)? else {
        return Ok(None);
    };
    let path = directory.join("broker-observation.json");
    if !path.exists() {
        return Ok(None);
    }
    let observed: Observation =
        serde_json::from_slice(&read(&path)?).map_err(|_| "invalid address observation")?;
    let at = observed
        .at
        .parse::<jiff::Timestamp>()
        .map_err(|_| "invalid address observation time")?;
    if observed.schema != 1
        || observed.catalog != hosts.opac
        || observed.staff != hosts.staff
        || at.as_second() > jiff::Timestamp::now().as_second().saturating_add(300)
    {
        return Err("address observation does not match the current identity".to_owned());
    }
    Ok(Some(elzevir_panel::RemoteAddress {
        at: observed.at,
        listed: observed.listed,
        staff_remote: observed.staff_remote,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::{Cell, RefCell},
        collections::VecDeque,
    };
    struct Fake {
        replies: RefCell<VecDeque<(&'static str, &'static str, serde_json::Value)>>,
        calls: Cell<usize>,
    }
    impl Transport for Fake {
        fn request(
            &self,
            _: &str,
            method: &str,
            path: &str,
            headers: &[(&str, String)],
            body: &[u8],
        ) -> Result<Response, String> {
            self.calls.set(self.calls.get() + 1);
            let (expected_method, expected_path, value) = self
                .replies
                .borrow_mut()
                .pop_front()
                .expect("unexpected broker call");
            assert_eq!((method, path), (expected_method, expected_path));
            if path == "/v1/device/start" {
                let value: serde_json::Value = serde_json::from_slice(body).unwrap();
                assert_eq!(value["origin"], "http://[::1]:9099");
                assert_eq!(value["services"], serde_json::json!(["elzevir"]));
            }
            if path == "/v1/enroll" {
                let value: serde_json::Value = serde_json::from_slice(body).unwrap();
                assert_eq!(
                    base64::engine::general_purpose::STANDARD
                        .decode(value["public_key"].as_str().unwrap())
                        .unwrap()
                        .len(),
                    32
                );
            }
            if path == "/v1/device/start" {
                let request: serde_json::Value = serde_json::from_slice(body).unwrap();
                assert_eq!(request["origin"], "http://[::1]:9099");
                assert_eq!(request["services"], serde_json::json!(["elzevir"]));
            }
            if matches!(path, "/v1/library" | "/v1/tunnel-token") {
                assert!(headers.iter().any(|(name, _)| *name == "X-KEI-Signature"));
            }
            if value
                .get("lost_response")
                .and_then(serde_json::Value::as_bool)
                == Some(true)
            {
                return Err("simulated lost response".to_owned());
            }
            Ok(Response {
                status: 200,
                body: zeroize::Zeroizing::new(serde_json::to_vec(&value).unwrap()),
            })
        }
    }
    struct Connection(Cell<bool>);
    impl Connector for Connection {
        fn connect(
            &self,
            directory: &Path,
            _: &Setup,
            hosts: &Hosts,
            token: &str,
        ) -> Result<(), String> {
            assert!(load(directory).unwrap().ready);
            assert_ne!(hosts.opac, hosts.staff);
            assert_eq!(token, "fictional-token-012345678901234567890123456789");
            self.0.set(true);
            Ok(())
        }
        fn stop(&self, _: &Path) -> Result<(), String> {
            Ok(())
        }
    }
    fn fixture(directory: &Path) -> Setup {
        let config = directory.join("server.toml");
        std::fs::write(
            &config,
            crate::server_config::render_init(
                "Fictional institution",
                directory,
                "[::]:9099".parse().unwrap(),
                false,
            )
            .unwrap(),
        )
        .unwrap();
        Setup {
            config,
            service: "elzevir".to_owned(),
            action: Action::Enroll {
                base: "https://broker.example.org".to_owned(),
                institution: "Fictional library".to_owned(),
                email: "fictional@example.org".to_owned(),
                name: "fictional".to_owned(),
            },
        }
    }
    fn info() -> serde_json::Value {
        serde_json::json!({"id":"fictional-library","status":"active","services":["elzevir"],"hostnames":{"opac":"catalog.example.org","staff":"staff.example.org"},"public_listing":false,"staff_remote_access":false})
    }
    #[test]
    fn unsupported_broker_cannot_receive_personal_data_or_create_identity() {
        let directory = tempfile::tempdir().unwrap();
        let setup = fixture(directory.path());
        let fake = Fake {
            replies: RefCell::new(VecDeque::from([(
                "GET",
                "/v1/capabilities",
                serde_json::json!({"services":["koha"]}),
            )])),
            calls: Cell::new(0),
        };
        let connection = Connection(Cell::new(false));
        assert!(
            execute_with(directory.path(), &setup, &fake, &connection, None)
                .unwrap_err()
                .starts_with("B1:")
        );
        assert_eq!(fake.calls.get(), 1);
        assert!(!location(directory.path()).exists());
        assert!(!connection.0.get());
    }
    fn happy_broker() -> Fake {
        Fake {
            replies: RefCell::new(VecDeque::from([
                (
                    "GET",
                    "/v1/capabilities",
                    serde_json::json!({"services":["elzevir"],"elzevir":{"contract":1,"configurable_origin":true}}),
                ),
                (
                    "POST",
                    "/v1/device/start",
                    serde_json::json!({"device_code":"fictional-device","verification_uri":"https://broker.example.org/join?c=BCDF-GHJK","expires_in":600,"interval":1}),
                ),
                (
                    "POST",
                    "/v1/device/poll",
                    serde_json::json!({"status":"approved"}),
                ),
                (
                    "POST",
                    "/v1/enroll",
                    serde_json::json!({"library_id":"fictional-library","job_id":"fictional-job","hostnames":{"opac":"catalog.example.org","staff":"staff.example.org"}}),
                ),
                (
                    "GET",
                    "/v1/jobs/fictional-job",
                    serde_json::json!({"state":"done"}),
                ),
                ("GET", "/v1/library", info()),
                (
                    "GET",
                    "/v1/tunnel-token",
                    serde_json::json!({"tunnel_token":"fictional-token-012345678901234567890123456789"}),
                ),
                ("GET", "/v1/library", info()),
            ])),
            calls: Cell::new(0),
        }
    }
    #[test]
    fn enrollment_persists_identity_and_origin_before_connecting_without_storing_token() {
        let directory = tempfile::tempdir().unwrap();
        let setup = fixture(directory.path());
        let fake = happy_broker();
        let connection = Connection(Cell::new(false));
        execute_with(directory.path(), &setup, &fake, &connection, None).unwrap();
        assert!(connection.0.get());
        let identity = load(directory.path()).unwrap();
        assert_eq!(identity.library.as_deref(), Some("fictional-library"));
        assert!(identity.device_code.is_none());
        assert!(
            !std::fs::read_to_string(location(directory.path()))
                .unwrap()
                .contains("fictional-token")
        );
        let settings = crate::server_config::ServeArgs::for_status(Some(setup.config), None)
            .resolve()
            .unwrap();
        assert!(require_host_policy(&settings).is_err());
    }
    #[test]
    fn lost_authorization_response_reuses_the_identity_and_retries_device_start() {
        let directory = tempfile::tempdir().unwrap();
        let setup = fixture(directory.path());
        let first = happy_broker();
        first.replies.borrow_mut().get_mut(1).unwrap().2 =
            serde_json::json!({"lost_response":true});
        let connection = Connection(Cell::new(false));
        assert!(execute_with(directory.path(), &setup, &first, &connection, None).is_err());
        let public_key = load(directory.path()).unwrap().key().unwrap().public();
        let retry = happy_broker();
        execute_with(directory.path(), &setup, &retry, &connection, None).unwrap();
        assert_eq!(
            load(directory.path()).unwrap().key().unwrap().public(),
            public_key
        );
        assert!(connection.0.get());
    }
    #[test]
    fn consumed_enrollment_after_lost_response_reuses_the_same_signing_key() {
        let directory = tempfile::tempdir().unwrap();
        let setup = fixture(directory.path());
        let first = happy_broker();
        first.replies.borrow_mut().get_mut(3).unwrap().2 =
            serde_json::json!({"lost_response":true});
        let connection = Connection(Cell::new(false));
        assert!(execute_with(directory.path(), &setup, &first, &connection, None).is_err());
        let public_key = load(directory.path()).unwrap().key().unwrap().public();
        let retry = happy_broker();
        retry.replies.borrow_mut().remove(1);
        retry.replies.borrow_mut().get_mut(1).unwrap().2 = serde_json::json!({"status":"consumed"});
        execute_with(directory.path(), &setup, &retry, &connection, None).unwrap();
        assert_eq!(
            load(directory.path()).unwrap().key().unwrap().public(),
            public_key
        );
        assert!(connection.0.get());
    }
    #[test]
    fn recovery_can_retry_rotation_with_the_identity_persisted_before_failure() {
        let directory = tempfile::tempdir().unwrap();
        let mut setup = fixture(directory.path());
        let connection = Connection(Cell::new(false));
        execute_with(directory.path(), &setup, &happy_broker(), &connection, None).unwrap();
        let identity = load(directory.path()).unwrap();
        let public_key = identity.key().unwrap().public();
        let exported = directory.path().join("recovery.json");
        crate::backup_archive::write_new(&exported, &serde_json::to_vec(&identity).unwrap())
            .unwrap();
        setup.action = Action::Recover { input: exported };
        for lost in [true, false] {
            let mut replies = happy_broker().replies.into_inner();
            replies.retain(|(_, path, _)| {
                matches!(
                    *path,
                    "/v1/capabilities" | "/v1/library" | "/v1/tunnel-token"
                )
            });
            replies.insert(
                2,
                (
                    "POST",
                    "/v1/rotate",
                    if lost {
                        serde_json::json!({"lost_response":true})
                    } else {
                        serde_json::json!({})
                    },
                ),
            );
            let fake = Fake {
                replies: RefCell::new(replies),
                calls: Cell::new(0),
            };
            fake.replies
                .borrow_mut()
                .insert(3, ("GET", "/v1/library", info()));
            connection.0.set(false);
            assert_eq!(
                execute_with(directory.path(), &setup, &fake, &connection, None).is_err(),
                lost
            );
            assert_eq!(connection.0.get(), !lost);
            assert_eq!(
                load(directory.path()).unwrap().key().unwrap().public(),
                public_key
            );
        }
    }
}
