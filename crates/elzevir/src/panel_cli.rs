// SPDX-License-Identifier: GPL-3.0-or-later
//! Composition boundary: observations enter the TUI without storage dependencies.

use age::secrecy::ExposeSecret as _;
use elzevir_panel::{
    Component, Details, Host, Institution, ObservationRequest, ServerStatus, Snapshot, Storage,
};
use std::{
    path::{Path, PathBuf},
    time::Instant,
};

#[derive(clap::Args)]
#[command(group(clap::ArgGroup::new("panel_mode").required(true).multiple(true).args(["demo", "config", "data_dir"])))]
pub(crate) struct PanelArgs {
    /// Use simulated data without querying a server or measuring the host.
    #[arg(long, conflicts_with_all = ["config", "data_dir"])]
    demo: bool,
    /// Institution configuration file.
    #[arg(long)]
    config: Option<PathBuf>,
    /// Data directory, overriding the configuration file.
    #[arg(long)]
    data_dir: Option<PathBuf>,
    /// Interface language.
    #[arg(long, default_value = "en", value_parser = ["en", "pt-BR"])]
    lang: String,
    /// Draw borders with ASCII characters.
    #[arg(long)]
    ascii: bool,
}

pub(crate) fn run(args: PanelArgs) -> Result<(), String> {
    if args.demo {
        return elzevir_panel::run(&args.lang, args.ascii).map_err(|error| error.to_string());
    }
    let configured = args.config.is_some();
    let settings = crate::server_config::ServeArgs::for_status(args.config.clone(), args.data_dir)
        .resolve()
        .map_err(|error| error.to_string())?;
    let institution = Institution {
        name: settings.institution_name,
        configured_listen: configured.then_some(settings.listen),
        configured_access: configured.then(|| crate::addresses::presentation(&settings.staff)),
    };
    let mut collector = Collector::new(settings.data_dir);
    collector.config = args.config;
    elzevir_panel::run_live(&args.lang, args.ascii, institution, move |request| {
        collector.collect(&request)
    })
    .map_err(|error| error.to_string())
}

struct Collector {
    directory: PathBuf,
    system: Option<sysinfo::System>,
    cpu_at: Option<Instant>,
    cpu: Option<f32>,
    diagnostics: crate::diagnostics::Context,
    operation_result: Option<&'static str>,
    config: Option<PathBuf>,
    latest: Option<crate::release::Latest>,
}

impl Collector {
    fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            system: None,
            cpu_at: None,
            cpu: None,
            diagnostics: crate::diagnostics::Context::default(),
            operation_result: None,
            config: None,
            latest: None,
        }
    }

    fn collect(&mut self, request: &ObservationRequest) -> Snapshot {
        let observed = crate::status::inspect(&self.directory);
        let operations = self.operations(request);
        let diagnostics = self
            .diagnostics
            .observe(&self.directory, &observed, request);
        let details = observed.health.map(|health| Details {
            version: health.version,
            pid: health.pid,
            uptime_seconds: health.uptime_seconds,
            database: component(health.database),
            indexer: component(health.indexer),
            listen: observed.listen,
        });
        let status = match (observed.state, details) {
            ("running", Some(details)) => ServerStatus::Running(details),
            ("degraded", Some(details)) => ServerStatus::Degraded(details),
            ("stopped", _) => ServerStatus::Stopped,
            _ => ServerStatus::Unavailable,
        };
        let database_bytes = regular_size(&self.directory.join("elzevir.db"));
        let wal_bytes = match std::fs::symlink_metadata(self.directory.join("elzevir.db-wal")) {
            Ok(metadata) if metadata.is_file() => Some(metadata.len()),
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound && database_bytes.is_some() =>
            {
                Some(0)
            }
            _ => None,
        };
        // Filesystem and host metrics are optional observations, not substitutes for readiness.
        let available_bytes = fs4::available_space(&self.directory).ok();
        Snapshot {
            status,
            storage: Storage {
                database_bytes,
                wal_bytes,
                available_bytes,
            },
            host: self.host(),
            diagnostics,
            addresses: self.diagnostics.addresses(),
            operations,
        }
    }

    fn host(&mut self) -> Host {
        if self.system.is_none() {
            // Unsupported/temporarily unavailable host information stays unknown and is retried.
            self.system = sysinfo::System::new().ok();
        }
        let Some(system) = self.system.as_mut() else {
            return Host::default();
        };
        system.refresh_memory();
        let now = Instant::now();
        if self
            .cpu_at
            .is_none_or(|at| now.duration_since(at) >= sysinfo::MINIMUM_CPU_UPDATE_INTERVAL)
        {
            system.refresh_cpu_usage();
            if self.cpu_at.is_some() {
                self.cpu = Some(system.global_cpu_usage())
                    .filter(|n| n.is_finite() && (0.0..=100.0).contains(n));
            }
            self.cpu_at = Some(now);
        }
        let memory = (system.used_memory(), system.total_memory());
        Host {
            cpu_percent: self.cpu,
            memory_bytes: (memory.1 > 0 && memory.0 <= memory.1).then_some(memory),
        }
    }
}

fn regular_size(path: &Path) -> Option<u64> {
    // Refuse links and represent inaccessible measurements as unknown, never zero.
    std::fs::symlink_metadata(path)
        .ok()
        .filter(std::fs::Metadata::is_file)
        .map(|metadata| metadata.len())
}

fn component(check: crate::health::Check) -> Component {
    match check {
        crate::health::Check::Ok => Component::Ok,
        crate::health::Check::Starting => Component::Starting,
        crate::health::Check::Failed => Component::Failed,
    }
}

impl Collector {
    fn operations(&mut self, request: &ObservationRequest) -> elzevir_panel::Operations {
        if let ObservationRequest::Operate(operation) = request {
            self.operation_result = Some(match self.operate(operation) {
                Ok(()) => "op-submitted",
                Err(error) if error.starts_with("B1:") => "broker-unsupported",
                Err(_) => "op-refused",
            });
        }
        let mut result = elzevir_panel::Operations {
            result: self.operation_result,
            latest: self.latest.as_ref().map(|release| release.tag_name.clone()),
            release_notes: self
                .latest
                .as_ref()
                .and_then(|release| release.body.clone()),
            ..elzevir_panel::Operations::default()
        };
        result.cloud = crate::cloud::view(&self.directory).ok();
        result.remote_address = crate::broker::observe(&self.directory).ok().flatten();
        result.tunnel = match crate::tunnel_observe::observe(&self.directory) {
            Ok(Some(tunnel)) => elzevir_panel::TunnelState::Configured(tunnel),
            Ok(None) => elzevir_panel::TunnelState::NotConfigured,
            Err(_) => elzevir_panel::TunnelState::Unavailable,
        };
        // Client-side jobs are separate from the daemon that they may need to stop.
        if let Ok(receipts) = crate::host_jobs::receipts(&self.directory) {
            result.authorization_uris = receipts
                .iter()
                .filter_map(|receipt| receipt.authorization_uri.clone())
                .collect();
            result.jobs.extend(
                receipts
                    .into_iter()
                    .rev()
                    .map(|receipt| elzevir_panel::Job {
                        id: receipt.id,
                        at: String::new(),
                        action: match receipt.action.as_str() {
                            "restore" => "op-restore",
                            "restart" => "op-restart",
                            "update" => "op-update",
                            "recover_update" => "op-update-recover",
                            "cloudflare_login" => "op-cloudflare-login",
                            "cloudflare_setup" => "op-cloudflare-setup",
                            "tunnel_test" => "op-tunnel-test",
                            "broker" => "broker-operation",
                            "install_cloudflared" => "op-install-cloudflared",
                            "install_rclone" => "op-install-rclone",
                            _ => "op-reboot",
                        },
                        state: match receipt.state.as_str() {
                            "queued" => "op-queued",
                            "running" => "op-running",
                            "completed" => "op-completed",
                            "unsupported" => "broker-unsupported",
                            _ => "op-failed",
                        },
                    }),
            );
        }
        // Observation failure is represented as missing policy/history, never fake success.
        let Ok(info) = crate::runtime_info::read(&self.directory.join(crate::runtime_info::NAME))
        else {
            return result;
        };
        let Ok(view) = crate::admin::operations(&info) else {
            return result;
        };
        if let Some(uri) = view.authorization_uri {
            result.authorization_uris.push(uri);
        }
        result.keep = Some(view.policy.keep);
        result.daily = view
            .policy
            .daily
            .map(|daily| format!("{:02}:{:02} {}", daily.hour, daily.minute, daily.time_zone));
        result.sessions = crate::admin::sessions(&info).ok();
        result.jobs.extend(
            view.jobs
                .into_iter()
                .rev()
                .map(|job| elzevir_panel::Job {
                    id: job.id,
                    at: job.at,
                    action: match job.task {
                        crate::operations::Task::Backup => "op-backup-now",
                        crate::operations::Task::ConfigureCloud => "cloud-operation",
                        crate::operations::Task::CloudBackup { .. } => "cloud-backup",
                        crate::operations::Task::Verify { .. } => "op-backup-verify",
                    },
                    state: match job.state {
                        crate::operations::State::Queued => "op-queued",
                        crate::operations::State::Running => "op-running",
                        crate::operations::State::Completed => "op-completed",
                        crate::operations::State::Failed => "op-failed",
                        crate::operations::State::Interrupted => "op-interrupted",
                    },
                })
                .collect::<Vec<_>>(),
        );
        result
    }

    fn operate(&mut self, operation: &elzevir_panel::Operation) -> Result<(), String> {
        use elzevir_panel::Operation;
        if let Operation::Cloud(action) = operation {
            let info = crate::runtime_info::read(&self.directory.join(crate::runtime_info::NAME))?;
            crate::admin::cloud(&info, crate::cloud::intention(action)?)?;
            return Ok(());
        }
        if matches!(operation, Operation::CheckUpdates) {
            self.latest = Some(crate::release::latest(&crate::release::Http)?);
            return Ok(());
        }
        if let Operation::ExportKey { output } = operation {
            let identity = crate::backup_archive::key(&self.directory.join("backup.key"), false)?;
            return crate::backup_archive::write_new(
                Path::new(output),
                identity.to_string().expose_secret().as_bytes(),
            );
        }
        let host = self.host_operation(operation)?;
        if let Some(task) = host {
            crate::host_jobs::launch(&self.directory, task)?;
            return Ok(());
        }
        let info = crate::runtime_info::read(&self.directory.join(crate::runtime_info::NAME))?;
        match operation {
            Operation::BackupNow => {
                crate::admin::start_backup(&info)?;
            }
            Operation::VerifyLatest | Operation::ExportLatest { .. } => {
                let view = crate::admin::operations(&info)?;
                let job = view
                    .jobs
                    .into_iter()
                    .rev()
                    .find(|job| {
                        matches!(
                            job.task,
                            crate::operations::Task::Backup
                                | crate::operations::Task::CloudBackup { .. }
                        ) && job.state == crate::operations::State::Completed
                            && crate::backup_cli::artifact(&self.directory, &job.id)
                                .is_ok_and(|path| path.is_file())
                    })
                    .ok_or("no completed backup")?;
                if let Operation::ExportLatest { output } = operation {
                    crate::backup_cli::copy_new(
                        &crate::backup_cli::artifact(&self.directory, &job.id)?,
                        Path::new(output),
                    )?;
                } else {
                    crate::admin::verify_backup(&info, job.id)?;
                }
            }
            Operation::Policy { keep, time, zone } => {
                let daily = if time.is_empty() {
                    None
                } else {
                    let (hour, minute) = time.split_once(':').ok_or("time must be HH:MM")?;
                    Some(crate::operations_config::Daily {
                        hour: hour.parse().map_err(|_| "invalid hour")?,
                        minute: minute.parse().map_err(|_| "invalid minute")?,
                        time_zone: zone.clone(),
                    })
                };
                let policy = crate::operations_config::Policy {
                    keep: keep.parse().map_err(|_| "invalid retention")?,
                    daily,
                };
                crate::admin::backup_policy(&info, policy)?;
            }
            Operation::Address(_)
            | Operation::Cloud(_)
            | Operation::Restore { .. }
            | Operation::Restart { .. }
            | Operation::Reboot
            | Operation::Update { .. }
            | Operation::RecoverUpdate { .. }
            | Operation::CheckUpdates
            | Operation::InstallCloudflared
            | Operation::InstallRclone
            | Operation::TestTunnel
            | Operation::CloudflareLogin
            | Operation::CloudflareSetup { .. } => {
                return Err("invalid host operation dispatch".to_owned());
            }
            Operation::ExportKey { .. } => return Err("invalid export dispatch".to_owned()),
        }
        Ok(())
    }
}

impl Collector {
    fn host_operation(
        &self,
        operation: &elzevir_panel::Operation,
    ) -> Result<Option<crate::host_jobs::Task>, String> {
        use elzevir_panel::Operation;
        Ok(match operation {
            Operation::Address(action) => Some(crate::host_jobs::Task::Broker {
                setup: crate::broker::panel_setup(
                    self.config
                        .as_deref()
                        .ok_or("address requires a configuration file")?,
                    action,
                )?,
            }),
            Operation::CloudflareSetup {
                catalog,
                staff,
                name,
                service,
                protocol,
            } => Some(crate::host_jobs::Task::CloudflareSetup {
                setup: crate::cloudflare::Setup {
                    config: self
                        .config
                        .as_ref()
                        .ok_or("tunnel setup requires a configuration file")?
                        .clone(),
                    catalog: catalog.clone(),
                    staff: staff.clone(),
                    name: name.clone(),
                    service: service.clone(),
                    protocol: protocol.clone(),
                },
            }),
            Operation::CloudflareLogin => Some(crate::host_jobs::Task::CloudflareLogin),
            Operation::TestTunnel => Some(crate::host_jobs::Task::TestTunnel),
            Operation::InstallCloudflared => Some(crate::host_jobs::Task::InstallTool {
                tool: crate::managed_tools::Tool::Cloudflared,
            }),
            Operation::InstallRclone => Some(crate::host_jobs::Task::InstallTool {
                tool: crate::managed_tools::Tool::Rclone,
            }),
            Operation::Update { version, name } => Some(crate::host_jobs::Task::Update {
                config: self
                    .config
                    .as_ref()
                    .ok_or("update requires a configuration file")?
                    .canonicalize()
                    .map_err(|_| "configuration unavailable")?,
                name: name.clone(),
                version: version.clone(),
                package: None,
                checksums: None,
                executable: std::env::current_exe().map_err(|_| "update executable unavailable")?,
            }),
            Operation::RecoverUpdate { executable } => {
                let executable = if executable.trim().is_empty() {
                    std::env::current_exe().map_err(|_| "recovery executable unavailable")?
                } else {
                    executable.into()
                };
                let journal = crate::update_journal::Journal::load(&executable)?;
                let selected = self
                    .directory
                    .canonicalize()
                    .map_err(|_| "selected institution unavailable")?;
                let config = self
                    .config
                    .as_ref()
                    .ok_or("update recovery requires a configuration file")?;
                if journal.directory != selected
                    || crate::installation::absolute(config)? != journal.config
                {
                    return Err(
                        "update recovery belongs to another selected institution".to_owned()
                    );
                }
                Some(crate::host_jobs::Task::RecoverUpdate {
                    executable: journal.executable,
                    config: journal.config,
                })
            }
            Operation::Restore { input, key } => Some(crate::host_jobs::Task::Restore {
                input: input.into(),
                key: key.into(),
                config: self.config.clone(),
            }),
            Operation::Restart { name } => {
                Some(crate::host_jobs::Task::Restart { name: name.clone() })
            }
            Operation::Reboot => Some(crate::host_jobs::Task::Reboot),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sampling_neither_initializes_nor_opens_a_database() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("missing");
        let mut collector = Collector::new(missing.clone());
        let unknown = collector.collect(&ObservationRequest::Poll);
        assert!(matches!(unknown.status, ServerStatus::Unavailable));
        assert!(!missing.exists());
        let file = root.path().join("elzevir.db");
        std::fs::write(&file, "not a database").unwrap();
        let observed = Collector::new(root.path().to_owned()).collect(&ObservationRequest::Poll);
        assert_eq!(observed.storage.database_bytes, Some(14));
        assert_eq!(observed.storage.wal_bytes, Some(0));
        assert!(observed.host.cpu_percent.is_none());
        assert_eq!(std::fs::read_to_string(file).unwrap(), "not a database");
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    }
}
