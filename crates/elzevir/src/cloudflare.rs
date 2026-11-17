// SPDX-License-Identifier: GPL-3.0-or-later
//! Explicit Cloudflare account login and institution-private credentials.
use crate::backup_archive as archive;
use crate::process_runner::{Native, Progress, Request, Runner};
use base64::Engine as _;
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

fn allowed_url(value: &str) -> bool {
    if value.len() > 2048 {
        return false;
    }
    let Ok(uri) = value.parse::<ureq::http::Uri>() else {
        return false;
    };
    uri.scheme_str() == Some("https")
        && uri
            .authority()
            .is_some_and(|authority| authority.as_str() == "dash.cloudflare.com")
        && uri.path() == "/argotunnel"
}
fn extract_url(text: &str) -> Option<String> {
    for (offset, _) in text.match_indices("https://") {
        let value = text
            .get(offset..)?
            .chars()
            .take_while(|character| {
                !character.is_whitespace()
                    && !character.is_control()
                    && !matches!(character, '"' | '\'' | '<' | '>' | '\\')
            })
            .take(2049)
            .collect::<String>();
        if allowed_url(&value) {
            return Some(value);
        }
    }
    None
}
/// Projects only the expected interactive authorization URL, never raw tool output.
pub(crate) fn authorization_url(bytes: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?;
    for line in text.lines() {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(line)
            && let Some(message) = value.get("message").and_then(serde_json::Value::as_str)
            && let Some(url) = extract_url(message)
        {
            return Some(url);
        }
        if let Some(url) = extract_url(line) {
            return Some(url);
        }
    }
    None
}

#[derive(serde::Deserialize)]
struct Origin {
    #[serde(rename = "accountID")]
    account_id: String,
    #[serde(rename = "apiToken")]
    api_token: String,
}
impl Drop for Origin {
    fn drop(&mut self) {
        use zeroize::Zeroize as _;
        self.api_token.zeroize();
    }
}

fn certificate(path: &Path) -> Result<(), String> {
    if archive::regular(path)? > 65_536 {
        return Err("Cloudflare certificate exceeds limit".to_owned());
    }
    let mut source = zeroize::Zeroizing::new(String::new());
    std::fs::File::open(path)
        .and_then(|file| file.take(65_537).read_to_string(&mut source))
        .map_err(|_| "Cloudflare certificate unavailable")?;
    if source.len() > 65_536 {
        return Err("Cloudflare certificate exceeds limit".to_owned());
    }
    let payload = zeroize::Zeroizing::new(
        source
            .split_once("-----BEGIN ARGO TUNNEL TOKEN-----")
            .and_then(|(_, body)| body.split_once("-----END ARGO TUNNEL TOKEN-----"))
            .map(|(body, _)| {
                body.chars()
                    .filter(|character| !character.is_ascii_whitespace())
                    .collect::<String>()
            })
            .ok_or("invalid Cloudflare certificate")?,
    );
    let decoded = zeroize::Zeroizing::new(
        base64::engine::general_purpose::STANDARD
            .decode(payload.as_bytes())
            .map_err(|_| "invalid Cloudflare certificate")?,
    );
    let certificate: Origin =
        serde_json::from_slice(&decoded).map_err(|_| "invalid Cloudflare certificate")?;
    if certificate.account_id.len() != 32
        || !certificate
            .account_id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || certificate.api_token.is_empty()
        || certificate.api_token.len() > 16_384
    {
        return Err("invalid Cloudflare certificate".to_owned());
    }

    Ok(())
}
fn home_certificate() -> Result<PathBuf, String> {
    let directory = if cfg!(windows) {
        std::env::var_os("USERPROFILE")
    } else {
        std::env::var_os("HOME")
    }
    .ok_or("operator home directory unavailable")?;
    Ok(PathBuf::from(directory)
        .join(".cloudflared")
        .join("cert.pem"))
}

pub(crate) fn login(_directory: &Path, progress: Option<Progress>) -> Result<(), String> {
    crate::service::require_operator()?;
    let program = crate::managed_tools::ensure(crate::managed_tools::Tool::Cloudflared)?;
    login_with(&home_certificate()?, &program, &Native, progress)
}
fn login_with(
    source: &Path,
    program: &Path,
    runner: &impl Runner,
    progress: Option<Progress>,
) -> Result<(), String> {
    if std::fs::symlink_metadata(source).is_err() {
        let output = runner.run(Request {
            environment: None,
            program: program.to_owned(),
            args: vec!["--no-autoupdate".into(), "tunnel".into(), "login".into()],
            input: None,
            progress,
            timeout: Duration::from_mins(10),
            limit: 65_536,
        })?;
        if !output.success {
            return Err("Cloudflare sign-in failed or was cancelled".to_owned());
        }
    }
    certificate(source)?;
    crate::file_owner::operator_private(source)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Setup {
    pub config: PathBuf,
    pub catalog: String,
    pub staff: String,
    pub name: String,
    pub service: String,
    pub protocol: String,
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Intent {
    schema: u8,
    catalog: String,
    staff: String,
    name: String,
    id: Option<String>,
    catalog_created: bool,
    staff_created: bool,
    enforce_hosts: bool,
}

fn read_text(path: &Path, limit: u64) -> Result<String, String> {
    if archive::regular(path)? > limit {
        return Err("tunnel input exceeds limit".to_owned());
    }
    let mut text = String::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(limit + 1).read_to_string(&mut text))
        .map_err(|_| "tunnel input unavailable")?;
    if u64::try_from(text.len()).map_err(|_| "tunnel input exceeds limit")? > limit {
        return Err("tunnel input exceeds limit".to_owned());
    }
    Ok(text)
}
fn uuid(value: &str) -> bool {
    let lengths = [8, 4, 4, 4, 12];
    let pieces = value.split('-').collect::<Vec<_>>();
    pieces.len() == lengths.len()
        && pieces.iter().zip(lengths).all(|(piece, length)| {
            piece.len() == length && piece.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
}
fn load_intent(directory: &Path) -> Result<Option<Intent>, String> {
    let path = directory.join("secrets/cf-intent.json");
    if !path
        .try_exists()
        .map_err(|_| "tunnel configuration unavailable")?
    {
        return Ok(None);
    }
    let intent: Intent = serde_json::from_str(&read_text(&path, 16_384)?)
        .map_err(|_| "invalid tunnel configuration")?;
    let mut mode = crate::access_config::StaffMode::Hostname {
        catalog_host: intent.catalog.clone(),
        staff_host: intent.staff.clone(),
    };
    mode.validate(None)
        .map_err(|_| "invalid tunnel host policy")?;
    if intent.schema != 1
        || intent.name.is_empty()
        || intent.name.len() > 64
        || intent.name.starts_with('-')
        || intent.name.chars().any(char::is_control)
        || intent.id.as_ref().is_some_and(|id| !uuid(id))
    {
        return Err("invalid tunnel configuration".to_owned());
    }
    Ok(Some(intent))
}
fn publish(directory: &Path, path: &Path, bytes: &[u8], configuration: bool) -> Result<(), String> {
    if std::fs::symlink_metadata(path).is_ok() {
        archive::regular(path)?;
    }
    let mut staged =
        tempfile::NamedTempFile::new_in(path.parent().ok_or("tunnel file needs a parent")?)
            .map_err(|_| "cannot prepare tunnel configuration")?;
    staged
        .write_all(bytes)
        .and_then(|()| staged.as_file().sync_all())
        .map_err(|_| "cannot write tunnel configuration")?;
    let reference = if path.exists() { path } else { directory };
    if configuration {
        crate::file_owner::configuration(staged.path(), reference)?;
    } else {
        crate::file_owner::private_tree(staged.path(), directory)?;
    }
    staged
        .persist(path)
        .map_err(|_| "cannot publish tunnel configuration")?;
    Ok(())
}
fn save_intent(directory: &Path, intent: &Intent) -> Result<(), String> {
    publish(
        directory,
        &directory.join("secrets/cf-intent.json"),
        &serde_json::to_vec(intent).map_err(|_| "cannot encode tunnel configuration")?,
        false,
    )
}

trait TunnelManager {
    fn stop_existing(&self, directory: &Path) -> Result<(), String>;
    fn restart_core(
        &self,
        name: &str,
        directory: &Path,
        hosts: &crate::access_config::StaffMode,
    ) -> Result<(), String>;
    fn start(&self, directory: &Path, config: &Path) -> Result<(), String>;
}
struct NativeManager;
impl TunnelManager for NativeManager {
    fn stop_existing(&self, directory: &Path) -> Result<(), String> {
        let name = crate::service::tunnel_name(directory)?;
        let status = crate::service::native_status(&name)?;
        match status.get("state").and_then(serde_json::Value::as_str) {
            Some("not_installed" | "stopped") => Ok(()),
            Some("running" | "starting" | "stopping" | "failed") => {
                crate::service::native_action("stop", &name)
            }
            _ => Err("cannot observe the managed tunnel service".to_owned()),
        }
    }
    fn restart_core(
        &self,
        name: &str,
        directory: &Path,
        hosts: &crate::access_config::StaffMode,
    ) -> Result<(), String> {
        crate::service::native_action("restart", name)?;
        crate::host_jobs::wait_healthy(directory)?;
        let observed = crate::status::inspect(directory);
        if observed.state != "running" || observed.staff.as_ref() != Some(hosts) {
            return Err("server did not confirm the required staff host policy".to_owned());
        }
        Ok(())
    }
    fn start(&self, directory: &Path, config: &Path) -> Result<(), String> {
        let name = crate::service::install_tunnel(directory, config)?;
        crate::service::native_action("start", &name)
    }
}
pub(crate) fn configure(directory: &Path, setup: &Setup) -> Result<(), String> {
    crate::service::require_operator()?;
    let program = crate::managed_tools::ensure(crate::managed_tools::Tool::Cloudflared)?;
    configure_with(
        directory,
        setup,
        &program,
        &home_certificate()?,
        &Native,
        &NativeManager,
    )
}
fn cloudflared(
    runner: &impl Runner,
    program: &Path,
    args: Vec<std::ffi::OsString>,
) -> Result<(), String> {
    let output = runner.run(Request {
        environment: None,
        program: program.to_owned(),
        args,
        input: None,
        progress: None,
        timeout: Duration::from_mins(2),
        limit: 65_536,
    })?;
    if output.success {
        Ok(())
    } else {
        Err("Cloudflare refused the tunnel operation".to_owned())
    }
}
fn prepare_intent(directory: &Path, setup: &Setup) -> Result<Intent, String> {
    if !crate::service::valid_name(&setup.service)
        || !matches!(setup.protocol.as_str(), "auto" | "http2")
    {
        return Err("invalid tunnel service or transport".to_owned());
    }
    let catalog =
        elzevir_web::normalize_hostname(&setup.catalog).map_err(|_| "invalid catalog hostname")?;
    let staff =
        elzevir_web::normalize_hostname(&setup.staff).map_err(|_| "invalid staff hostname")?;
    let name = if setup.name.is_empty() {
        crate::service::tunnel_name(directory)?
    } else {
        setup.name.clone()
    };
    if name.len() > 64
        || name.starts_with('-')
        || name.chars().any(char::is_control)
        || catalog == staff
    {
        return Err("tunnel requires distinct valid hosts and a bounded name".to_owned());
    }
    if let Some(intent) = load_intent(directory)? {
        if intent.catalog != catalog || intent.staff != staff || intent.name != name {
            return Err(
                "existing tunnel choices differ; review the managed address before replacing it"
                    .to_owned(),
            );
        }
        return Ok(intent);
    }
    if std::fs::symlink_metadata(directory.join("secrets/cf-credentials.json")).is_ok()
        || std::fs::symlink_metadata(directory.join("secrets/cf-config.json")).is_ok()
    {
        return Err("existing tunnel files are not recognized; they were preserved".to_owned());
    }
    Ok(Intent {
        schema: 1,
        catalog,
        staff,
        name,
        id: None,
        catalog_created: false,
        staff_created: false,
        enforce_hosts: false,
    })
}
fn credential_id(path: &Path) -> Result<String, String> {
    #[derive(serde::Deserialize)]
    struct Identity {
        #[serde(rename = "TunnelID")]
        id: String,
    }
    let credential: Identity = serde_json::from_str(&read_text(path, 65_536)?)
        .map_err(|_| "invalid tunnel credentials")?;
    if !uuid(&credential.id) {
        return Err("invalid tunnel credential identity".to_owned());
    }
    Ok(credential.id)
}
fn provision(
    directory: &Path,
    intent: &mut Intent,
    certificate: &Path,
    program: &Path,
    runner: &impl Runner,
) -> Result<(), String> {
    let credentials = directory.join("secrets/cf-credentials.json");
    if intent.id.is_none() {
        if !credentials.exists() {
            let stage =
                crate::private_temp::directory().map_err(|_| "cannot stage tunnel credentials")?;
            let pending = stage.path().join("credentials.json");
            cloudflared(
                runner,
                program,
                vec![
                    "--no-autoupdate".into(),
                    "tunnel".into(),
                    "--origincert".into(),
                    certificate.as_os_str().to_owned(),
                    "create".into(),
                    "--output".into(),
                    "json".into(),
                    "--credentials-file".into(),
                    pending.as_os_str().to_owned(),
                    intent.name.clone().into(),
                ],
            )?;
            credential_id(&pending)?;
            crate::backup_cli::copy_new(&pending, &credentials)?;
            crate::file_owner::private_tree(&credentials, directory)?;
        }
        intent.id = Some(credential_id(&credentials)?);
        save_intent(directory, intent)?;
    }
    let id = intent.id.as_ref().ok_or("tunnel identity absent")?.clone();
    for staff in [false, true] {
        if if staff {
            intent.staff_created
        } else {
            intent.catalog_created
        } {
            continue;
        }
        let host = if staff {
            &intent.staff
        } else {
            &intent.catalog
        };
        cloudflared(
            runner,
            program,
            vec![
                "--no-autoupdate".into(),
                "tunnel".into(),
                "--origincert".into(),
                certificate.as_os_str().to_owned(),
                "route".into(),
                "dns".into(),
                id.clone().into(),
                host.into(),
            ],
        )?;
        if staff {
            intent.staff_created = true;
        } else {
            intent.catalog_created = true;
        }
        save_intent(directory, intent)?;
    }
    Ok(())
}
fn configure_with(
    directory: &Path,
    setup: &Setup,
    program: &Path,
    certificate_path: &Path,
    runner: &impl Runner,
    manager: &impl TunnelManager,
) -> Result<(), String> {
    if !crate::service::valid_name(&setup.service)
        || !matches!(setup.protocol.as_str(), "auto" | "http2")
    {
        return Err("invalid tunnel service or transport protocol".to_owned());
    }
    archive::regular(&directory.join("elzevir.db"))?;
    let directory = directory
        .canonicalize()
        .map_err(|_| "institution directory unavailable")?;
    let settings = crate::server_config::ServeArgs::for_status(Some(setup.config.clone()), None)
        .resolve()
        .map_err(|_| "tunnel server configuration unavailable")?;
    if settings
        .data_dir
        .canonicalize()
        .map_err(|_| "institution directory unavailable")?
        != directory
        || settings.listen.port() == 0
    {
        return Err("tunnel requires this institution and a fixed local origin port".to_owned());
    }
    if directory.join("secrets/broker.json").exists() {
        return Err(
            "existing free address must be reviewed before configuring an own-domain tunnel"
                .to_owned(),
        );
    }
    certificate(certificate_path)?;
    let mut intent = prepare_intent(&directory, setup)?;
    let original = read_text(&setup.config, 65_536)?;
    manager.stop_existing(&directory)?;
    save_intent(&directory, &intent)?;
    provision(&directory, &mut intent, certificate_path, program, runner)?;
    let hosts = crate::access_config::StaffMode::Hostname {
        catalog_host: intent.catalog.clone(),
        staff_host: intent.staff.clone(),
    };
    let source = crate::server_config::with_hosts(&original, &intent.catalog, &intent.staff)?;
    let address = if settings.listen.ip().is_unspecified() {
        std::net::SocketAddr::new(
            if settings.listen.is_ipv6() {
                std::net::IpAddr::V6(std::net::Ipv6Addr::LOCALHOST)
            } else {
                std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
            },
            settings.listen.port(),
        )
    } else {
        settings.listen
    };
    let origin = format!("http://{address}");
    let log_parent = directory.join("logs");
    let new_log_parent = !log_parent.exists();
    crate::backup_cli::make_directory(&log_parent)?;
    if new_log_parent {
        crate::file_owner::private_tree(&log_parent, &directory)?;
    }
    let log_directory = log_parent.join("cloudflared");
    let new_log_directory = !log_directory.exists();
    crate::backup_cli::make_directory(&log_directory)?;
    if new_log_directory {
        crate::file_owner::private_tree(&log_directory, &directory)?;
    }
    let connector = serde_json::json!({"tunnel":intent.id,"credentials-file":directory.join("secrets/cf-credentials.json"),"protocol":setup.protocol,"logfile":log_directory.join("events.jsonl"),"loglevel":"info","logformat":"json","ingress":[{"hostname":intent.catalog,"service":origin},{"hostname":intent.staff,"service":origin},{"service":"http_status:404"}]});
    let connector_path = directory.join("secrets/cf-config.json");
    publish(
        &directory,
        &connector_path,
        &serde_json::to_vec(&connector).map_err(|_| "cannot encode tunnel connector")?,
        false,
    )?;
    intent.enforce_hosts = true;
    save_intent(&directory, &intent)?;
    let result = (|| {
        publish(&directory, &setup.config, source.as_bytes(), true)?;
        manager.restart_core(&setup.service, &directory, &hosts)?;
        manager.start(&directory, &connector_path)
    })();
    if result.is_err() {
        // Never relax the host policy while an old or partially started connector can run.
        manager.stop_existing(&directory)?;
        intent.enforce_hosts = false;
        save_intent(&directory, &intent)?;
        publish(&directory, &setup.config, original.as_bytes(), true)?;
        manager.restart_core(&setup.service, &directory, &settings.staff)?;
    }
    result
}

/// A managed public connector cannot outlive a restart into an unprotected host policy.
pub(crate) fn require_host_policy(settings: &crate::server_config::Settings) -> Result<(), String> {
    if let Some(intent) = load_intent(&settings.data_dir)?
        && intent.enforce_hosts
        && settings.staff
            != (crate::access_config::StaffMode::Hostname {
                catalog_host: intent.catalog,
                staff_host: intent.staff,
            })
    {
        return Err(
            "managed tunnel requires the configured catalog and staff hostname policy".to_owned(),
        );
    }
    Ok(())
}

pub(crate) fn public_hosts(directory: &Path) -> Result<Option<(String, String)>, String> {
    if let Some(hosts) = crate::broker::hosts(directory)? {
        return Ok(Some((hosts.opac, hosts.staff)));
    }
    Ok(load_intent(directory)?
        .filter(|intent| {
            intent.enforce_hosts
                && intent.id.is_some()
                && intent.catalog_created
                && intent.staff_created
        })
        .map(|intent| (intent.catalog, intent.staff)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn existing_operator_certificate_stays_outside_the_institution() {
        struct NoLogin;
        impl Runner for NoLogin {
            fn run(&self, _: Request) -> Result<crate::process_runner::Output, String> {
                Err("existing certificate must not trigger login".to_owned())
            }
        }
        let operator = tempfile::tempdir().unwrap();
        let institution = tempfile::tempdir().unwrap();
        let source = operator.path().join("cert.pem");
        configuration_tests::fixture_certificate(&source);
        login_with(&source, Path::new("fixture"), &NoLogin, None).unwrap();
        assert!(source.exists());
        assert!(!institution.path().join("secrets").exists());
    }
    #[test]
    fn authorization_links_require_the_expected_cloudflare_host_and_path() {
        assert_eq!(
            authorization_url(b"Open https://dash.cloudflare.com/argotunnel?callback=fixture\n")
                .as_deref(),
            Some("https://dash.cloudflare.com/argotunnel?callback=fixture")
        );
        for url in [
            "http://dash.cloudflare.com/argotunnel",
            "https://dash.cloudflare.com.evil/argotunnel",
            "https://evil.invalid/argotunnel",
            "https://dash.cloudflare.com/account",
        ] {
            assert!(authorization_url(url.as_bytes()).is_none());
        }
    }
}

#[cfg(test)]
mod configuration_tests {
    use super::*;
    use std::cell::Cell;
    struct Fake {
        dns_fail: bool,
        restarts: Cell<usize>,
        dns_calls: Cell<usize>,
    }
    impl Runner for Fake {
        fn run(&self, request: Request) -> Result<crate::process_runner::Output, String> {
            if request.args.iter().any(|arg| arg == "create") {
                let index = request
                    .args
                    .iter()
                    .position(|arg| arg == "--credentials-file")
                    .unwrap()
                    + 1;
                let path = PathBuf::from(request.args.get(index).unwrap());
                std::fs::write(
                    path,
                    serde_json::to_vec(
                        &serde_json::json!({"TunnelID":"00000000-0000-4000-8000-000000000001"}),
                    )
                    .unwrap(),
                )
                .unwrap();
            }
            if request.args.iter().any(|arg| arg == "dns") {
                self.dns_calls.set(self.dns_calls.get() + 1);
                if self.dns_fail {
                    return Err("fictional DNS refusal".to_owned());
                }
            }
            Ok(crate::process_runner::Output {
                success: true,
                stdout: Vec::new(),
            })
        }
    }
    impl TunnelManager for Fake {
        fn stop_existing(&self, _directory: &Path) -> Result<(), String> {
            Ok(())
        }
        fn restart_core(
            &self,
            _name: &str,
            _directory: &Path,
            _hosts: &crate::access_config::StaffMode,
        ) -> Result<(), String> {
            self.restarts.set(self.restarts.get() + 1);
            Ok(())
        }
        fn start(&self, _directory: &Path, _config: &Path) -> Result<(), String> {
            Ok(())
        }
    }
    pub(super) fn fixture_certificate(path: &Path) {
        let payload = base64::engine::general_purpose::STANDARD.encode(
            serde_json::to_vec(
                &serde_json::json!({"accountID":"a".repeat(32),"apiToken":"fictional-token"}),
            )
            .unwrap(),
        );
        std::fs::write(
            path,
            format!(
                "-----BEGIN ARGO TUNNEL TOKEN-----\n{payload}\n-----END ARGO TUNNEL TOKEN-----\n"
            ),
        )
        .unwrap();
    }

    #[test]
    fn tunnel_uses_the_real_origin_port_and_enforces_host_separation_across_restarts() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("elzevir.db"), "fixture").unwrap();
        let config = directory.path().join("server.toml");
        std::fs::write(
            &config,
            crate::server_config::render_init(
                "Fictional library",
                directory.path(),
                "[::]:9099".parse().unwrap(),
                false,
            )
            .unwrap(),
        )
        .unwrap();
        crate::backup_cli::make_directory(&directory.path().join("secrets")).unwrap();
        fixture_certificate(&directory.path().join("secrets/cf-cert.pem"));
        let fake = Fake {
            dns_fail: false,
            restarts: Cell::new(0),
            dns_calls: Cell::new(0),
        };
        let options = Setup {
            config,
            catalog: "catalog.example.org".to_owned(),
            staff: "staff.example.org".to_owned(),
            name: "fictional-library".to_owned(),
            service: "elzevir".to_owned(),
            protocol: "http2".to_owned(),
        };
        configure_with(
            directory.path(),
            &options,
            Path::new("fictional-cloudflared"),
            &directory.path().join("secrets/cf-cert.pem"),
            &fake,
            &fake,
        )
        .unwrap();
        let text =
            std::fs::read_to_string(directory.path().join("secrets/cf-config.json")).unwrap();
        let connector: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            connector
                .pointer("/ingress/0/service")
                .and_then(serde_json::Value::as_str),
            Some("http://[::1]:9099")
        );
        assert_eq!(
            connector
                .pointer("/ingress/2/service")
                .and_then(serde_json::Value::as_str),
            Some("http_status:404")
        );
        assert!(!text.contains("fictional-token"));
        let mut settings =
            crate::server_config::ServeArgs::for_status(Some(options.config.clone()), None)
                .resolve()
                .unwrap();
        require_host_policy(&settings).unwrap();
        settings.staff = crate::access_config::StaffMode::SameAddress {};
        assert!(require_host_policy(&settings).is_err());
        configure_with(
            directory.path(),
            &options,
            Path::new("fictional-cloudflared"),
            &directory.path().join("secrets/cf-cert.pem"),
            &fake,
            &fake,
        )
        .unwrap();
        assert_eq!(fake.dns_calls.get(), 2);
    }

    #[test]
    fn dns_failure_does_not_rewrite_or_restart_the_library_server() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("elzevir.db"), "fixture").unwrap();
        let config = directory.path().join("server.toml");
        let original = crate::server_config::render_init(
            "Fictional library",
            directory.path(),
            "127.0.0.1:9099".parse().unwrap(),
            false,
        )
        .unwrap();
        std::fs::write(&config, &original).unwrap();
        crate::backup_cli::make_directory(&directory.path().join("secrets")).unwrap();
        fixture_certificate(&directory.path().join("secrets/cf-cert.pem"));
        let fake = Fake {
            dns_fail: true,
            restarts: Cell::new(0),
            dns_calls: Cell::new(0),
        };
        let options = Setup {
            config,
            catalog: "catalog.example.org".to_owned(),
            staff: "staff.example.org".to_owned(),
            name: "fictional-library".to_owned(),
            service: "elzevir".to_owned(),
            protocol: "http2".to_owned(),
        };
        assert!(
            configure_with(
                directory.path(),
                &options,
                Path::new("fictional-cloudflared"),
                &directory.path().join("secrets/cf-cert.pem"),
                &fake,
                &fake
            )
            .is_err()
        );
        assert_eq!(std::fs::read_to_string(&options.config).unwrap(), original);
        assert_eq!(fake.restarts.get(), 0);
        assert_eq!(fake.dns_calls.get(), 1);
    }
}
