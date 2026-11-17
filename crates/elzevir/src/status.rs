// SPDX-License-Identifier: GPL-3.0-or-later
//! Read-only local status: lock observation and identity-checked health response.

use crate::{health::Report, runtime_info};
use serde::Serialize;
use std::path::PathBuf;
use std::time::Duration;

#[derive(clap::Args)]
pub(crate) struct StatusArgs {
    /// Configuration file identifying the data directory.
    #[arg(long)]
    config: Option<PathBuf>,
    /// Data directory (overrides the configuration file).
    #[arg(long, required_unless_present = "config")]
    data_dir: Option<PathBuf>,
    /// Print the versioned JSON status document.
    #[arg(long)]
    json: bool,
}

#[derive(Serialize)]
pub(crate) struct Status {
    schema: u8,
    pub(crate) state: &'static str,
    pub(crate) health: Option<Report>,
    pub(crate) listen: Option<std::net::SocketAddr>,
    pub(crate) staff: Option<crate::access_config::StaffMode>,
    reason: Option<&'static str>,
}

impl Status {
    fn state(state: &'static str, reason: &'static str) -> Self {
        Self {
            schema: 1,
            state,
            health: None,
            listen: None,
            staff: None,
            reason: Some(reason),
        }
    }
}

pub(crate) fn run(args: StatusArgs) -> Result<(), String> {
    let directory = crate::server_config::ServeArgs::for_status(args.config, args.data_dir)
        .resolve()
        .map_err(|error| error.to_string())?
        .data_dir;
    let report = inspect(&directory);
    if args.json {
        println!(
            "{}",
            serde_json::to_string(&report).map_err(|_| "cannot encode status")?
        );
    } else {
        println!(
            "elzevir: {} ({})",
            report.state,
            report.reason.unwrap_or(if report.state == "running" {
                "components ready"
            } else {
                "components not ready"
            })
        );
    }
    if report.state == "running" {
        Ok(())
    } else {
        Err(format!("server status: {}", report.state))
    }
}

pub(crate) fn inspect(directory: &std::path::Path) -> Status {
    let path = directory.join("elzevir.lock");
    if !std::fs::symlink_metadata(&path).is_ok_and(|file| file.is_file() && !file.is_symlink()) {
        return Status::state("unavailable", "lock_unavailable");
    }
    let Ok(lock) = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
    else {
        return Status::state("unavailable", "lock_unavailable");
    };
    match lock.try_lock() {
        Ok(()) => return Status::state("stopped", "lock_free"),
        Err(std::fs::TryLockError::WouldBlock) => {}
        Err(_) => return Status::state("unavailable", "lock_unavailable"),
    }
    let Ok(info) = runtime_info::read(&directory.join(runtime_info::NAME)) else {
        return Status::state("unavailable", "invalid_runtime_metadata");
    };
    if info.admin.is_some() {
        return match crate::admin::status(&info) {
            Ok(health) => from_health(health, info.listen, info.staff),
            Err(_) => Status::state("unavailable", "admin_unavailable"),
        };
    }
    let config = ureq::Agent::config_builder()
        .proxy(None)
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(2)))
        .max_response_header_size(4096)
        .max_redirects(0)
        .build();
    let agent = ureq::Agent::new_with_config(config);
    let Ok(mut response) = agent.get(format!("http://{}/health", info.address)).call() else {
        return Status::state("unavailable", "health_unreachable");
    };
    let code = response.status().as_u16();
    let Ok(body) = response
        .body_mut()
        .with_config()
        .limit(4096)
        .read_to_string()
    else {
        return Status::state("unavailable", "invalid_health_response");
    };
    let health: Report = match serde_json::from_str(&body) {
        Ok(health) => health,
        Err(_) => return Status::state("unavailable", "invalid_health_response"),
    };
    if health.schema != 1 || health.instance != info.instance || health.pid != info.pid {
        return Status::state("unavailable", "health_identity_mismatch");
    }
    if !matches!((code, health.healthy()), (200, true) | (503, false)) {
        return Status::state("unavailable", "invalid_health_response");
    }
    from_health(health, info.listen, info.staff)
}

fn from_health(
    health: Report,
    listen: Option<std::net::SocketAddr>,
    staff: Option<crate::access_config::StaffMode>,
) -> Status {
    Status {
        schema: 1,
        state: if health.healthy() {
            "running"
        } else {
            "degraded"
        },
        health: Some(health),
        listen,
        staff,
        reason: None,
    }
}
