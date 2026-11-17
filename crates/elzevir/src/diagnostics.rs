// SPDX-License-Identifier: GPL-3.0-or-later
//! Explicit diagnostics composed from the existing process and safe local observations.

use elzevir_panel::{CheckResult, Diagnostics, ExportState, ObservationRequest};
use std::path::{Path, PathBuf};

#[derive(clap::Args)]
pub(crate) struct DiagnoseArgs {
    /// Institution configuration file.
    #[arg(long)]
    config: Option<PathBuf>,
    /// Data directory, overriding the configuration file.
    #[arg(long, required_unless_present = "config")]
    data_dir: Option<PathBuf>,
    /// New redacted ZIP file; an existing file is never overwritten.
    #[arg(long)]
    output: Option<PathBuf>,
}

pub(crate) fn run(args: DiagnoseArgs) -> Result<(), String> {
    let directory = crate::server_config::ServeArgs::for_status(args.config, args.data_dir)
        .resolve()
        .map_err(|error| error.to_string())?
        .data_dir;
    let status = crate::status::inspect(&directory);
    let mut context = Context::default();
    let diagnostics = context.observe(&directory, &status, &ObservationRequest::Verify);
    if let Some(output) = args.output {
        export(&directory, &status, &diagnostics, Some(&output))?;
    }
    println!("{}", report(&directory, &status, &diagnostics));
    Ok(())
}

#[derive(Default)]
pub(crate) struct Context {
    values: Diagnostics,
    instance: Option<String>,
    addresses: crate::addresses::Context,
}

impl Context {
    pub(crate) fn observe(
        &mut self,
        directory: &Path,
        status: &crate::status::Status,
        request: &ObservationRequest,
    ) -> Diagnostics {
        let instance = status.health.as_ref().map(|health| health.instance.clone());
        if self.instance != instance {
            self.values.quick_check = CheckResult::NotRun;
            self.values.catalog_http = CheckResult::NotRun;
            self.values.verified_at = None;
            self.instance = instance;
        }
        self.values.logs = crate::diagnostic_logs::read(directory);
        self.values.local_volume = if directory.is_dir() {
            if crate::local_disk::check(directory).is_ok() {
                CheckResult::Ok
            } else {
                CheckResult::Unknown
            }
        } else {
            CheckResult::Unknown
        };
        self.values.directory_access = access(directory);
        let addresses = self.addresses.observe(status, request);
        if matches!(
            request,
            ObservationRequest::Verify | ObservationRequest::Export
        ) {
            self.values.verified_at = Some(jiff::Timestamp::now().to_string());
            self.values.quick_check = integrity(directory, status);
            self.values.catalog_http = addresses.catalog_test;
        }
        if request == &ObservationRequest::Export {
            self.values.export = match export(directory, status, &self.values, None) {
                Ok(path) => ExportState::Saved(path.to_string_lossy().into_owned()),
                Err(_) => ExportState::Failed,
            };
        }
        self.values.clone()
    }

    pub(crate) fn addresses(&self) -> elzevir_panel::AddressBook {
        self.addresses.book()
    }
}

fn integrity(directory: &Path, status: &crate::status::Status) -> CheckResult {
    let Some(health) = &status.health else {
        return CheckResult::Unknown;
    };
    let Ok(info) = crate::runtime_info::read(&directory.join(crate::runtime_info::NAME)) else {
        return CheckResult::Unknown;
    };
    if info.instance != health.instance || info.pid != health.pid {
        return CheckResult::Unknown;
    }
    match crate::admin::diagnose(&info) {
        Ok(crate::health::Integrity::Ok) => CheckResult::Ok,
        Ok(crate::health::Integrity::Failed) => CheckResult::Failed,
        Ok(crate::health::Integrity::Unknown) | Err(_) => CheckResult::Unknown,
    }
}

#[cfg(unix)]
fn access(directory: &Path) -> CheckResult {
    use rustix::fs::{Access, access};
    if !directory.is_dir() {
        return CheckResult::Unknown;
    }
    if access(
        directory,
        Access::READ_OK | Access::WRITE_OK | Access::EXEC_OK,
    )
    .is_ok()
    {
        CheckResult::Ok
    } else {
        CheckResult::Failed
    }
}

#[cfg(not(unix))]
fn access(_directory: &Path) -> CheckResult {
    CheckResult::Unknown
}

pub(crate) fn report(
    directory: &Path,
    status: &crate::status::Status,
    values: &Diagnostics,
) -> serde_json::Value {
    // Explicit field allowlist: never serialize configuration, metadata or raw health/logs.
    let database = status.health.as_ref().map(|h| check_code(h.database));
    let indexer = status.health.as_ref().map(|h| check_code(h.indexer));
    // An inaccessible measurement stays absent rather than an invented zero.
    let available = fs4::available_space(directory).ok();
    serde_json::json!({
        "schema": 1, "version": env!("CARGO_PKG_VERSION"),
        "generated_at": jiff::Timestamp::now().to_string(),
        "state": status.state, "database": database, "indexer": indexer,
        "quick_check": values.quick_check.code(), "catalog_http": values.catalog_http.code(),
        "verified_at": values.verified_at, "local_volume": values.local_volume.code(),
        "directory_access": values.directory_access.code(), "available_bytes": available,
        "space": CheckResult::available_space(available).code(),
        "clock_synchronization": "not_checked", "tunnel": "not_checked", "latest_release": "not_checked",
        "logs": {"state": match values.logs.state { elzevir_panel::LogState::Missing => "missing", elzevir_panel::LogState::Ready => "ready", elzevir_panel::LogState::Unavailable => "unavailable" },
            "count": values.logs.entries.len(), "limited": values.logs.limited, "omitted": values.logs.omitted}
    })
}

fn check_code(value: crate::health::Check) -> &'static str {
    match value {
        crate::health::Check::Starting => "starting",
        crate::health::Check::Ok => "ok",
        crate::health::Check::Failed => "failed",
    }
}

fn export(
    directory: &Path,
    status: &crate::status::Status,
    values: &Diagnostics,
    output: Option<&Path>,
) -> Result<PathBuf, String> {
    let report = serde_json::to_vec_pretty(&report(directory, status, values))
        .map_err(|_| "cannot encode diagnostic report")?;
    let mut events = Vec::new();
    for entry in &values.logs.entries {
        serde_json::to_writer(&mut events, &serde_json::json!({"timestamp":entry.timestamp,"level":entry.event.level(),"event":entry.event.code()})).map_err(|_| "cannot encode diagnostic events")?;
        events.push(b'\n');
    }
    crate::support_archive::publish(&report, &events, output)
        .map_err(|_| "cannot publish diagnostic archive".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_checks_survive_polls_but_not_a_new_or_missing_identity() {
        let root = tempfile::tempdir().unwrap();
        let mut status = crate::status::inspect(root.path());
        status.health = Some(crate::health::Report {
            schema: 1,
            instance: "0".repeat(32),
            pid: 1,
            version: "test".to_owned(),
            uptime_seconds: 0,
            database: crate::health::Check::Ok,
            indexer: crate::health::Check::Ok,
        });
        let mut context = Context {
            instance: Some("0".repeat(32)),
            values: Diagnostics {
                quick_check: CheckResult::Ok,
                verified_at: Some("2026-11-16T12:00:00Z".to_owned()),
                ..Diagnostics::default()
            },
            ..Context::default()
        };
        assert_eq!(
            context
                .observe(root.path(), &status, &ObservationRequest::Poll)
                .quick_check,
            CheckResult::Ok
        );
        if let Some(health) = &mut status.health {
            health.instance = "1".repeat(32);
        }
        let restarted = context.observe(root.path(), &status, &ObservationRequest::Poll);
        assert_eq!(restarted.quick_check, CheckResult::NotRun);
        assert!(restarted.verified_at.is_none());
        context.values.quick_check = CheckResult::Ok;
        status.health = None;
        assert_eq!(
            context
                .observe(root.path(), &status, &ObservationRequest::Poll)
                .quick_check,
            CheckResult::NotRun
        );
    }
}
