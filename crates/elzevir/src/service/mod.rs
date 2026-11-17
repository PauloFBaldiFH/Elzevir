// SPDX-License-Identifier: GPL-3.0-or-later
//! Explicit system-service management; dry runs never call a manager.

#[cfg(target_os = "linux")]
mod openrc;
#[cfg(target_os = "linux")]
mod systemd;
#[cfg(windows)]
mod windows;

use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(clap::Args)]
pub(crate) struct ServiceArgs {
    #[command(subcommand)]
    action: Action,
}

#[derive(clap::Subcommand)]
enum Action {
    /// Register an existing institution for automatic startup; does not start it.
    Install(Install),
    /// Stop, disable and unregister only the managed service, preserving all data.
    Uninstall(Target),
    /// Start the managed service.
    Start(Target),
    /// Request graceful shutdown of the managed service.
    Stop(Target),
    /// Stop and start the managed service.
    Restart(Target),
    /// Print manager state as JSON; this is distinct from application health.
    Status(Name),
    #[command(hide = true)]
    Run(Entry),
}

#[derive(clap::Args)]
struct Name {
    /// Managed service name: elzevir or elzevir- followed by a lowercase identifier.
    #[arg(long, default_value = "elzevir")]
    name: String,
}

#[derive(clap::Args)]
struct Target {
    #[command(flatten)]
    target: Name,
    /// Print the action without invoking the manager or changing files.
    #[arg(long)]
    dry_run: bool,
}

#[derive(clap::Args)]
struct Install {
    /// Existing institution configuration file.
    #[arg(long)]
    config: PathBuf,
    #[command(flatten)]
    target: Target,
    /// Existing non-root systemd account (default: elzevir); Windows uses a virtual account.
    #[arg(long)]
    account: Option<String>,
}

#[derive(clap::Args)]
pub(crate) struct Entry {
    #[arg(long)]
    pub(crate) config: PathBuf,
    #[arg(long)]
    pub(crate) name: String,
}

#[derive(Serialize)]
pub(crate) struct Plan {
    schema: u8,
    platform: &'static str,
    action: &'static str,
    pub(crate) name: String,
    pub(crate) executable: PathBuf,
    pub(crate) config: PathBuf,
    pub(crate) data_dir: PathBuf,
    pub(crate) account: String,
    pub(crate) privileged_port: bool,
    unit: Option<String>,
}

pub(crate) trait Backend {
    fn install(&self, plan: &Plan) -> Result<(), String>;
    fn action(&self, action: &str, name: &str) -> Result<(), String>;
    fn status(&self, name: &str) -> Result<serde_json::Value, String>;
}

pub(crate) fn valid_name(name: &str) -> bool {
    name == "elzevir"
        || (name.len() <= 64
            && name.strip_prefix("elzevir-").is_some_and(|suffix| {
                !suffix.is_empty()
                    && !suffix.starts_with('-')
                    && !suffix.ends_with('-')
                    && suffix
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            }))
}

fn platform() -> Result<&'static str, String> {
    if cfg!(target_os = "linux") {
        Ok(if use_openrc() { "openrc" } else { "systemd" })
    } else if cfg!(windows) {
        Ok("windows")
    } else {
        Err("service management is not implemented on this platform".to_owned())
    }
}

fn path(path: &Path) -> Result<PathBuf, String> {
    let path = path
        .canonicalize()
        .map_err(|_| "service paths must already exist")?;
    if path
        .to_str()
        .is_none_or(|s| s.chars().any(char::is_control))
    {
        return Err("service paths must be Unicode without control characters".to_owned());
    }
    Ok(path)
}

fn plan(args: &Install) -> Result<Plan, String> {
    if !valid_name(&args.target.target.name) {
        return Err("invalid managed service name".to_owned());
    }
    let metadata = std::fs::symlink_metadata(&args.config)
        .map_err(|_| "service configuration must already exist")?;
    if !metadata.is_file() || metadata.is_symlink() {
        return Err("service configuration must be a regular file without symlinks".to_owned());
    }
    let config = path(&args.config)?;
    let settings = crate::server_config::ServeArgs::for_status(Some(config.clone()), None)
        .resolve()
        .map_err(|e| e.to_string())?;
    let data_dir = path(&settings.data_dir)?;
    let db = std::fs::symlink_metadata(data_dir.join("elzevir.db"))
        .map_err(|_| "initialize the institution before installing a service")?;
    if !db.is_file() || db.is_symlink() {
        return Err("service database must be a regular file".to_owned());
    }
    crate::local_disk::check(&data_dir)?;
    let account = if cfg!(windows) {
        if args.account.is_some() {
            return Err("Windows services use a dedicated virtual account".to_owned());
        }
        format!("NT SERVICE\\{}", args.target.target.name)
    } else {
        let account = args.account.clone().unwrap_or_else(|| "elzevir".to_owned());
        if account == "root"
            || account.is_empty()
            || account.len() > 32
            || !account
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
            || account.starts_with('-')
            || account.as_bytes().first().is_some_and(u8::is_ascii_digit)
        {
            return Err("service account must be an existing non-root account name".to_owned());
        }
        account
    };
    let low = |a: std::net::SocketAddr| a.port() != 0 && a.port() < 1024;
    let privileged_port = low(settings.listen)
        || matches!(settings.staff,crate::access_config::StaffMode::SeparateListener {listen,..} if low(listen));
    let plan = Plan {
        schema: 1,
        platform: platform()?,
        action: "install",
        name: args.target.target.name.clone(),
        executable: path(
            &std::env::current_exe().map_err(|_| "cannot locate service executable")?,
        )?,
        config,
        data_dir,
        account,
        privileged_port,
        unit: None,
    };
    #[cfg(target_os = "linux")]
    let plan = Plan {
        unit: Some(if use_openrc() {
            openrc::render(&plan)?
        } else {
            systemd::render(&plan)?
        }),
        ..plan
    };
    Ok(plan)
}

pub(crate) fn run(args: ServiceArgs) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    let backend = native_backend();
    #[cfg(windows)]
    let backend: Box<dyn Backend> = Box::new(windows::Windows);
    #[cfg(any(target_os = "linux", windows))]
    {
        dispatch(&*backend, args.action)
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        drop(args);
        Err("service management is not implemented on this platform".to_owned())
    }
}

fn dispatch(backend: &(impl Backend + ?Sized), action: Action) -> Result<(), String> {
    match action {
        Action::Install(args) => {
            let plan = plan(&args)?;
            if args.target.dry_run {
                println!(
                    "{}",
                    serde_json::to_string(&plan).map_err(|_| "cannot encode service plan")?
                );
            } else {
                backend.install(&plan)?;
                println!(
                    "elzevir: service installed and enabled; start it after checking account permissions"
                );
            }
        }
        Action::Status(target) => {
            if !valid_name(&target.name) {
                return Err("invalid managed service name".to_owned());
            }
            println!("{}", backend.status(&target.name)?);
        }
        Action::Run(entry) => {
            if !valid_name(&entry.name) {
                return Err("invalid managed service name".to_owned());
            }
            #[cfg(windows)]
            return windows::dispatch(entry);
            #[cfg(not(windows))]
            {
                drop(entry);
                return Err(
                    "native service entry is only available through the Windows SCM".to_owned(),
                );
            }
        }
        Action::Start(target) => control(backend, "start", &target)?,
        Action::Stop(target) => control(backend, "stop", &target)?,
        Action::Restart(target) => control(backend, "restart", &target)?,
        Action::Uninstall(target) => control(backend, "uninstall", &target)?,
    }
    Ok(())
}

fn control(backend: &(impl Backend + ?Sized), action: &str, target: &Target) -> Result<(), String> {
    if !valid_name(&target.target.name) {
        return Err("invalid managed service name".to_owned());
    }
    if target.dry_run {
        println!(
            "{}",
            serde_json::json!({"schema":1,"platform":platform()?,"action":action,"name":target.target.name})
        );
    } else {
        backend.action(action, &target.target.name)?;
        println!("elzevir: service {action} completed");
    }
    Ok(())
}

/// Composition entry for a separately confirmed host operation.
pub(crate) fn native_action(action: &str, name: &str) -> Result<(), String> {
    if !valid_name(name) || !matches!(action, "start" | "stop" | "restart") {
        return Err("invalid service operation".to_owned());
    }
    #[cfg(target_os = "linux")]
    let backend = native_backend();
    #[cfg(windows)]
    let backend: Box<dyn Backend> = Box::new(windows::Windows);
    #[cfg(any(target_os = "linux", windows))]
    {
        backend.action(action, name)
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        Err("service management unavailable".to_owned())
    }
}

#[cfg(target_os = "linux")]
fn use_openrc() -> bool {
    !Path::new("/run/systemd/system").is_dir() && Path::new("/sbin/openrc").is_file()
}
#[cfg(target_os = "linux")]
fn native_backend() -> Box<dyn Backend> {
    if use_openrc() {
        Box::new(openrc::OpenRc::default())
    } else {
        Box::new(systemd::Systemd::default())
    }
}

#[cfg(not(target_os = "linux"))]
fn use_openrc() -> bool {
    false
}
