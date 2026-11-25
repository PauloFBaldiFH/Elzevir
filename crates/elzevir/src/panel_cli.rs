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
    #[arg(long, value_parser = ["en", "pt-BR"])]
    lang: Option<String>,
    /// Draw borders with ASCII characters.
    #[arg(long)]
    ascii: bool,
}

pub(crate) fn run(args: PanelArgs, no_self_update: bool) -> Result<(), String> {
    if args.demo {
        let language = crate::first_use::language(args.lang.as_deref());
        return elzevir_panel::run(language, args.ascii).map_err(|error| error.to_string());
    }
    if !crate::first_use::interactive() {
        return Err("panel requires an interactive terminal (including with --demo)".into());
    }
    let mut input = crate::server_config::ServeArgs::for_status(args.config, args.data_dir);
    input.lang = args.lang;
    let (settings, config) = crate::first_use::resolve(input)?;
    connected(&settings, config, args.ascii, no_self_update, || false)
}

pub(crate) fn connected(
    settings: &crate::server_config::Settings,
    config: Option<PathBuf>,
    ascii: bool,
    no_self_update: bool,
    stopping: impl Fn() -> bool,
) -> Result<(), String> {
    let institution = Institution {
        name: settings.institution_name.clone(),
        configured_listen: config.as_ref().map(|_| settings.listen),
        configured_access: config
            .as_ref()
            .map(|_| crate::addresses::presentation(&settings.staff)),
        language: Some(settings.lang.clone()),
        managed: managed(),
    };
    let mut collector = Collector::new(settings.data_dir.clone());
    collector.config = config;
    collector.no_self_update = no_self_update;
    // Without a thread the panel shows no release news; the panel itself still works.
    collector.release_check = std::thread::Builder::new()
        .name("elzevir-release-check".into())
        .spawn(|| crate::release::latest(&crate::release::Metadata))
        .ok();
    elzevir_panel::run_live_until(
        &settings.lang,
        ascii,
        institution,
        move |request| collector.collect(&request),
        stopping,
    )
    .map_err(|error| error.to_string())
}

/// Whether this program belongs to an installation run by the system's
/// service manager (`install.sh`/`install.ps1` with service options).
fn managed() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|executable| crate::installation::record_path(&executable).ok())
        .is_some_and(|record| std::fs::symlink_metadata(record).is_ok())
}

struct PreparedRestore {
    review: elzevir_panel::RestoreReview,
    task: crate::host_jobs::Task,
}
struct Collector {
    restore: Option<PreparedRestore>,
    directory: PathBuf,
    system: Option<sysinfo::System>,
    cpu_at: Option<Instant>,
    cpu: Option<f32>,
    diagnostics: crate::diagnostics::Context,
    operation_result: Option<&'static str>,
    config: Option<PathBuf>,
    latest: Option<crate::release::Latest>,
    release_check: Option<std::thread::JoinHandle<Result<crate::release::Latest, String>>>,
    no_self_update: bool,
}

impl Collector {
    fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            system: None,
            cpu_at: None,
            restore: None,
            cpu: None,
            diagnostics: crate::diagnostics::Context::default(),
            operation_result: None,
            config: None,
            latest: None,
            release_check: None,
            no_self_update: false,
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
        self.observe_request(request);
        let mut result = elzevir_panel::Operations {
            result: self.operation_result,
            review: self.restore.as_ref().map(|r| r.review.clone()),
            backup_folder: Some(self.directory.join("backups")),
            latest: self.latest.as_ref().map(|release| release.tag_name.clone()),
            update_available: self.latest.as_ref().is_some_and(|release| {
                crate::personal_update::newer(&release.tag_name, env!("CARGO_PKG_VERSION"))
            }),
            release_notes: self
                .latest
                .as_ref()
                .and_then(|release| release.body.clone()),
            ..elzevir_panel::Operations::default()
        };
        if let Ok(ids) = crate::local_artifacts::ids(&self.directory) {
            result.local_backup = crate::local_artifacts::observation(&self.directory, &ids);
            result.managed_ids = ids;
        }
        // Unreadable cloud or broker state is shown as unknown in the panel, not as an error.
        result.cloud = crate::cloud::view(&self.directory).ok();
        result.remote_address = crate::broker::observe(&self.directory).ok().flatten();
        // Invalid private configuration becomes unavailable, never a successful registration.
        result.domain = crate::broker::domain_status(&self.directory).ok();
        result.tunnel = match crate::tunnel_observe::observe(&self.directory) {
            Ok(Some(tunnel)) => elzevir_panel::TunnelState::Configured(tunnel),
            Ok(None) => elzevir_panel::TunnelState::NotConfigured,
            Err(_) => elzevir_panel::TunnelState::Unavailable,
        };
        // Client-side jobs are separate from the daemon that they may need to stop.
        self.host_history(&mut result);
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
        // Unreadable sessions leave the list empty; the rest of the panel stays usable.
        result.sessions = crate::admin::sessions(&info).ok();
        result.jobs.extend(
            view.jobs
                .into_iter()
                .rev()
                .map(|job| elzevir_panel::Job {
                    phase: job.phase.map(crate::backup_archive::Phase::label),
                    id: job.id,
                    at: job.at,
                    action: match job.task {
                        crate::operations::Task::Backup { .. } => "op-backup-now",
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

    fn host_history(&self, result: &mut elzevir_panel::Operations) {
        if let Ok(receipts) = crate::host_jobs::receipts(&self.directory) {
            let exports = receipts.iter().filter(|r| r.action == "export_backup");
            if let Some(latest) = exports
                .clone()
                .map(|r| r.created)
                .max()
                .filter(|at| *at > 0)
            {
                let mut dated = exports.filter(|r| r.created == latest);
                if let Some(receipt) = dated.next()
                    && dated.next().is_none()
                {
                    result.latest_export_failed =
                        Some(matches!(receipt.state.as_str(), "failed" | "interrupted"));
                }
            }
            result.authorization_uris = receipts
                .iter()
                .filter_map(|receipt| receipt.authorization_uri.clone())
                .collect();
            result.jobs.extend(
                receipts
                    .into_iter()
                    .rev()
                    .map(|receipt| elzevir_panel::Job {
                        phase: None,
                        id: receipt.id,
                        at: String::new(),
                        action: match receipt.action.as_str() {
                            "restore" => "op-restore",
                            "restart" => "op-restart",
                            "update" => "op-update",
                            "recover_update" => "op-update-recover",
                            "cloudflare_login" => "op-cloudflare-login",
                            "cloudflare_setup" => "op-cloudflare-setup",
                            "staff_test" => "op-staff-test",
                            "tunnel_test" => "op-tunnel-test",
                            "broker" => "broker-operation",
                            "install_cloudflared" => "op-install-cloudflared",
                            "export_backup" => "op-backup-export",
                            "install_rclone" => "op-install-rclone",
                            _ => "op-reboot",
                        },
                        state: match receipt.state.as_str() {
                            "queued" => "op-queued",
                            "running" => "op-running",
                            "interrupted" => "op-interrupted",
                            "completed" => "op-completed",
                            crate::managed_restore::BEFORE => "restore-before",
                            crate::managed_restore::RECOVERED => "restore-recovered",
                            crate::managed_restore::PENDING => "restore-pending",
                            "unsupported" => "broker-unsupported",
                            _ => "op-failed",
                        },
                    }),
            );
        }
    }
    fn export_backup(&self, id: &str, output: &str) -> Result<(), String> {
        let source = crate::backup_cli::artifact(&self.directory, id)?;
        let target = if output.ends_with(std::path::MAIN_SEPARATOR) {
            std::fs::create_dir_all(output).map_err(|_| "cannot create export folder")?;
            Path::new(output).join(source.file_name().ok_or("artifact name missing")?)
        } else {
            Path::new(output).to_owned()
        };
        crate::backup_cli::export_managed(&self.directory, id, &target)
    }
    fn backup_now(
        &self,
        info: &crate::runtime_info::RuntimeInfo,
        options: elzevir_panel::BackupOptions,
        folder: &str,
    ) -> Result<(), String> {
        let job = crate::admin::start_selected_backup(info, options)?;
        if !folder.is_empty() && Path::new(folder) != self.directory.join("backups") {
            crate::host_jobs::launch(
                &self.directory,
                crate::host_jobs::Task::ExportBackup {
                    id: job.id,
                    folder: std::path::absolute(folder).map_err(|_| "invalid export directory")?,
                },
            )?;
        }
        Ok(())
    }
    fn observe_request(&mut self, request: &ObservationRequest) {
        if self
            .release_check
            .as_ref()
            .is_some_and(std::thread::JoinHandle::is_finished)
            && let Some(check) = self.release_check.take()
            && let Ok(Ok(release)) = check.join()
            && self.latest.is_none()
        {
            self.latest = Some(release);
        }
        if let ObservationRequest::Operate(operation) = request {
            self.operation_result = Some(match self.operate(operation) {
                // Starting and stopping finish here; there is no job to follow.
                Ok(()) if matches!(operation, elzevir_panel::Operation::Server(_)) => {
                    "op-server-done"
                }
                Ok(()) if matches!(operation, elzevir_panel::Operation::Settings { .. }) => {
                    "settings-saved"
                }
                Err(_) if matches!(operation, elzevir_panel::Operation::Settings { .. }) => {
                    "settings-invalid"
                }
                Ok(()) if matches!(operation, elzevir_panel::Operation::Restore { .. }) => {
                    "restore-ready"
                }
                Err(_) if matches!(operation, elzevir_panel::Operation::Restore { .. }) => {
                    "restore-invalid"
                }
                Ok(()) => "op-submitted",
                Err(error) if error.starts_with("B1:") => "broker-unsupported",
                Err(_) => "op-refused",
            });
        }
    }

    fn validate_restore(&mut self, input: &str, key: &str) -> Result<(), String> {
        self.restore = None;
        let input = crate::installation::absolute(Path::new(input))?;
        let key = crate::installation::absolute(Path::new(key))?;
        let extracted = crate::backup_archive::extract(&input, &key)?;
        let m = extracted.manifest.clone();
        if input.parent()
            == self
                .directory
                .join("cloud-downloads")
                .canonicalize()
                .ok()
                .as_deref()
            && input.file_name() != Some(std::ffi::OsStr::new(&format!("elzevir-{}.age", m.id)))
        {
            return Err("downloaded archive identity differs from the selected object".into());
        }
        crate::backup_cli::prepare(extracted, &self.directory, true)?;
        let reviewed = crate::managed_restore::signature(&m)?;
        let managed = crate::local_artifacts::ids(&self.directory)
            .unwrap_or_default()
            .contains(&m.id)
            && crate::backup_cli::artifact(&self.directory, &m.id)?
                .canonicalize()
                .is_ok_and(|owned| owned == input);
        self.restore = Some(PreparedRestore {
            review: elzevir_panel::RestoreReview {
                token: crate::backup_archive::identifier()?,
                id: m.id,
                created: m.created_at,
                scope: m.scope,
                schema: m.database_schema,
                managed,
                file: input
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
            },
            task: crate::host_jobs::Task::Restore {
                input,
                key,
                config: self.config.clone(),
                reviewed,
            },
        });
        Ok(())
    }
    fn operate(&mut self, operation: &elzevir_panel::Operation) -> Result<(), String> {
        use elzevir_panel::Operation;
        if let Operation::Cloud(action) = operation {
            let info = crate::runtime_info::read(&self.directory.join(crate::runtime_info::NAME))?;
            crate::admin::cloud(&info, crate::cloud::intention(action)?)?;
            return Ok(());
        }
        if let Operation::Server(action) = operation {
            return self.server(*action);
        }
        if let Operation::Settings {
            name,
            listen,
            language,
        } = operation
        {
            return self.settings(name, listen, language);
        }
        if matches!(operation, Operation::CheckUpdates) {
            self.latest = Some(crate::release::latest(&crate::release::Metadata)?);
            return Ok(());
        }
        if let Operation::ExportKey { output } = operation {
            let identity = crate::backup_archive::key(&self.directory.join("backup.key"), false)?;
            return crate::backup_archive::write_new(
                Path::new(output),
                identity.to_string().expose_secret().as_bytes(),
            );
        }
        if let Operation::Restore { input, key } = operation {
            return self.validate_restore(input, key);
        }
        if let Operation::ApplyRestore(review) = operation {
            let prepared = self
                .restore
                .take()
                .filter(|r| r.review == *review)
                .ok_or("restore review expired")?;
            crate::host_jobs::launch(&self.directory, prepared.task)?;
            return Ok(());
        }
        let host = self.host_operation(operation)?;
        if let Some(task) = host {
            crate::host_jobs::launch(&self.directory, task)?;
            return Ok(());
        }
        self.daemon_operation(operation)
    }

    fn daemon_operation(&mut self, operation: &elzevir_panel::Operation) -> Result<(), String> {
        use elzevir_panel::Operation;
        let info = crate::runtime_info::read(&self.directory.join(crate::runtime_info::NAME))?;
        match operation {
            Operation::BackupNow { options, folder } => {
                self.backup_now(&info, *options, folder)?;
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
                            crate::operations::Task::Backup { .. }
                                | crate::operations::Task::CloudBackup { .. }
                        ) && job.state == crate::operations::State::Completed
                            && crate::backup_cli::artifact(&self.directory, &job.id)
                                .is_ok_and(|path| path.is_file())
                    })
                    .ok_or("no completed backup")?;
                if let Operation::ExportLatest { output } = operation {
                    self.export_backup(&job.id, output)?;
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
            | Operation::Server(_)
            | Operation::Settings { .. }
            | Operation::ApplyRestore(_)
            | Operation::Restore { .. }
            | Operation::Restart { .. }
            | Operation::Reboot
            | Operation::Update { .. }
            | Operation::RecoverUpdate { .. }
            | Operation::CheckUpdates
            | Operation::InstallCloudflared
            | Operation::InstallRclone
            | Operation::TestStaff
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
    /// Starts, stops or restarts a personal installation's server, which
    /// has no service manager to ask.
    fn server(&self, action: elzevir_panel::ServerAction) -> Result<(), String> {
        use elzevir_panel::ServerAction;
        if managed() {
            return Err("the service manager starts and stops this server".into());
        }
        if matches!(action, ServerAction::Stop | ServerAction::Restart) {
            crate::background::stop(&self.directory)?;
        }
        if matches!(action, ServerAction::Start | ServerAction::Restart) {
            let executable =
                std::env::current_exe().map_err(|_| "cannot locate the elzevir executable")?;
            let config = self
                .config
                .clone()
                .unwrap_or_else(|| self.directory.join(crate::first_use::CONFIG));
            crate::background::restart(&executable, &self.directory, &config)?;
        }
        Ok(())
    }

    /// Saves the settings form; the server applies it when it restarts.
    fn settings(&self, name: &str, listen: &str, language: &str) -> Result<(), String> {
        let config = self
            .config
            .as_deref()
            .ok_or("settings require a configuration file")?;
        crate::server_config::validate_name(name).map_err(|error| error.to_string())?;
        let listen: std::net::SocketAddr = listen
            .trim()
            .parse()
            .map_err(|_| "address must look like 127.0.0.1:7390")?;
        if listen.port() < elzevir_panel::MIN_PORT {
            return Err("port must be 1024 or above".into());
        }
        let language = language.trim();
        if !crate::server_config::LANGUAGES.contains(&language) {
            return Err("language must be en or pt-BR".into());
        }
        let current = crate::server_config::ServeArgs::for_status(Some(config.to_owned()), None)
            .resolve()
            .map_err(|error| error.to_string())?;
        // The running server holds its own address; any other must be free.
        if listen != current.listen && !crate::listen_port::free(listen) {
            return Err("the new address is in use".into());
        }
        crate::listen_port::rewrite(config, |source| {
            crate::server_config::with_settings(
                source,
                crate::server_config::Edit {
                    name: name.to_owned(),
                    listen,
                    lang: language.to_owned(),
                },
            )
        })
    }

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
            Operation::TestStaff => Some(crate::host_jobs::Task::TestStaff),
            Operation::TestTunnel => Some(crate::host_jobs::Task::TestTunnel),
            Operation::InstallCloudflared => Some(crate::host_jobs::Task::InstallTool {
                tool: crate::managed_tools::Tool::Cloudflared,
            }),
            Operation::InstallRclone => Some(crate::host_jobs::Task::InstallTool {
                tool: crate::managed_tools::Tool::Rclone,
            }),
            Operation::Update { version, name } => {
                let executable =
                    std::env::current_exe().map_err(|_| "update executable unavailable")?;
                crate::personal_update::allowed(&executable, self.no_self_update)?;
                Some(crate::host_jobs::Task::Update {
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
                    executable,
                })
            }
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
    fn validating_a_replaced_archive_leaves_the_active_institution_and_clears_review() {
        let directory = tempfile::tempdir().unwrap();
        let data = crate::open_store(directory.path()).unwrap();
        let mut collector = Collector::new(directory.path().into());
        let source = directory.path().join("old.age");
        let key = directory.path().join("fixture.key");
        crate::backup_archive::write_new(
            &source,
            include_bytes!("../tests/data/gzip-v1/library.age"),
        )
        .unwrap();
        crate::backup_archive::write_new(&key, include_bytes!("../tests/data/gzip-v1/fixture.key"))
            .unwrap();
        collector
            .validate_restore(source.to_str().unwrap(), key.to_str().unwrap())
            .unwrap();
        let reviewed = collector.restore.as_ref().unwrap().review.clone();
        assert!(!reviewed.managed);
        std::fs::write(&source, b"partial archive").unwrap();
        assert!(
            collector
                .validate_restore(source.to_str().unwrap(), key.to_str().unwrap())
                .is_err()
        );
        assert!(
            collector
                .operate(&elzevir_panel::Operation::ApplyRestore(reviewed))
                .is_err()
        );
        assert!(!directory.path().join("host-jobs").exists());
        assert!(crate::backup_cli::lock(directory.path()).is_err());
        data.store.shutdown().unwrap();
    }

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
