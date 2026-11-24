// SPDX-License-Identifier: GPL-3.0-or-later
//! Backup-first verified updates, with binary and data rollback after failed health.
use crate::{
    backup_archive as archive,
    process_runner::{Native, Request, Runner as _},
};
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

trait Backend {
    fn step(&self, name: &'static str) -> Result<(), String>;
}
fn transaction(backend: &impl Backend) -> Result<(), String> {
    backend.step("download")?;
    backend.step("backup")?;
    if let Err(error) = backend.step("stop") {
        backend
            .step("start")
            .map_err(|_| "stop failed and the previous service could not be restarted")?;
        backend.step("rollback_health")?;
        backend.step("finalize")?;
        return Err(error);
    }
    let result = backend
        .step("final_backup")
        .and_then(|()| backend.step("swap"))
        .and_then(|()| backend.step("start"))
        .and_then(|()| backend.step("health"));
    if result.is_err() {
        backend.step("stop").map_err(
            |_| "update failed and the replacement could not be stopped; recovery files retained",
        )?;
        backend.step("rollback_binary")?;
        backend.step("rollback_data")?;
        backend.step("start")?;
        backend.step("rollback_health")?;
        backend.step("finalize")?;
        return Err("update failed; the previous version and database were restored".to_owned());
    }
    backend.step("finalize")
}

#[derive(clap::Args)]
pub(crate) struct Args {
    #[arg(long)]
    config: Option<PathBuf>,
    /// Data directory to back up for a personal installation.
    #[arg(long, conflicts_with = "config")]
    data_dir: Option<PathBuf>,
    #[arg(long, default_value = "elzevir", requires = "config")]
    service: String,
    #[arg(long, default_value = "latest")]
    version: String,
    #[arg(long, requires_all = ["checksums", "config"])]
    package: Option<PathBuf>,
    #[arg(long, requires = "package")]
    checksums: Option<PathBuf>,
    #[arg(long)]
    dry_run: bool,
    #[arg(long)]
    confirm_update: bool,
    #[arg(long)]
    wait: bool,
}
pub(crate) fn run(args: Args, no_self_update: bool) -> Result<(), String> {
    let executable = std::env::current_exe()
        .and_then(|path| path.canonicalize())
        .map_err(|_| "update executable unavailable")?;
    crate::personal_update::allowed(&executable, no_self_update)?;
    if args.version != "latest" && !crate::release::valid_version(&args.version) {
        return Err("invalid release version".into());
    }
    let Some(config) = args.config else {
        return crate::personal_update::run(
            executable,
            args.data_dir,
            args.version,
            args.dry_run,
            args.wait,
        );
    };
    let settings = crate::server_config::ServeArgs::for_status(Some(config.clone()), None)
        .resolve()
        .map_err(|error| error.to_string())?;
    if !crate::service::valid_name(&args.service) {
        return Err("invalid update service".to_owned());
    }
    if args.dry_run {
        println!(
            "{}",
            serde_json::json!({"schema":1,"action":"update","version":args.version,"target":crate::release::target()?,"service":args.service,"config":config})
        );
        return Ok(());
    }
    if !args.confirm_update {
        return Err("update requires --confirm-update".to_owned());
    }
    let directory = settings.data_dir;
    let task = crate::host_jobs::Task::Update {
        config: config
            .canonicalize()
            .map_err(|_| "update configuration unavailable")?,
        name: args.service,
        version: args.version,
        package: args.package,
        checksums: args.checksums,
        executable,
    };
    let id = crate::host_jobs::launch(&directory, task)?;
    println!("{}", serde_json::json!({"id":id,"state":"queued"}));
    if args.wait {
        crate::host_jobs::wait(&directory, &id)?;
    }
    Ok(())
}

struct Prepared {
    scratch: tempfile::TempDir,
    staged: Option<PathBuf>,
    previous: PathBuf,
    swapped: bool,
    journal: Option<crate::update_journal::Journal>,
}
struct Context {
    directory: PathBuf,
    config: PathBuf,
    name: String,
    version: String,
    package: Option<PathBuf>,
    checksums: Option<PathBuf>,
    executable: PathBuf,
    prepared: RefCell<Prepared>,
}

pub(crate) struct Update {
    pub config: PathBuf,
    pub name: String,
    pub version: String,
    pub package: Option<PathBuf>,
    pub checksums: Option<PathBuf>,
    pub executable: PathBuf,
}
pub(crate) fn execute(directory: &Path, update: Update) -> Result<(), String> {
    if !crate::service::valid_name(&update.name) {
        return Err("invalid update service".to_owned());
    }
    crate::personal_update::allowed(&update.executable, false)?;
    crate::service::require_operator()?;
    crate::installation::verify(&update.executable, directory, &update.config, &update.name)?;
    let _program_lock = crate::installation::lock(&update.executable)?;
    archive::regular(&update.executable)?;
    if crate::update_journal::pending(&update.executable)? {
        return Err("recover the interrupted update before installing another version".to_owned());
    }
    let parent = update
        .executable
        .parent()
        .ok_or("update executable needs parent")?;
    let scratch = crate::private_temp::directory_in(parent)
        .map_err(|_| "update requires write access to the executable directory")?;
    let previous = parent.join(format!(
        "elzevir-previous-{}{}",
        archive::identifier()?,
        if cfg!(windows) { ".exe" } else { "" }
    ));
    let context = Context {
        directory: directory.to_owned(),
        config: update.config,
        name: update.name,
        version: update.version,
        package: update.package,
        checksums: update.checksums,
        executable: update.executable,
        prepared: RefCell::new(Prepared {
            scratch,
            staged: None,
            previous,
            swapped: false,
            journal: None,
        }),
    };
    let result = transaction(&context);
    if result.is_err()
        && !context.prepared.borrow().swapped
        && let Some(journal) = &context.prepared.borrow().journal
        && journal.phase == crate::update_journal::Phase::Prepared
        && crate::update_journal::pending(&context.executable)?
    {
        journal.finish()?;
    }
    result
}

impl Backend for Context {
    fn step(&self, name: &'static str) -> Result<(), String> {
        match name {
            "download" => self.prepare(),
            "backup" => {
                drop(backup(&self.directory, &self.config, false)?);
                Ok(())
            }
            "final_backup" => {
                let backup = backup(&self.directory, &self.config, true)?;
                {
                    let mut prepared = self.prepared.borrow_mut();
                    prepared
                        .journal
                        .as_mut()
                        .ok_or("update journal absent")?
                        .protect_backup(&backup)?;
                }
                Ok(())
            }
            "stop" => crate::service::native_action("stop", &self.name),
            "swap" => self.swap(),
            "start" => {
                if let Some(journal) = self.prepared.borrow_mut().journal.as_mut()
                    && matches!(journal.phase, crate::update_journal::Phase::Swapping)
                {
                    journal.phase(crate::update_journal::Phase::Starting)?;
                }
                crate::service::native_action("start", &self.name)
            }
            "health" => self.healthy(true),
            "rollback_health" => self.healthy(false),
            "finalize" => {
                if let Some(journal) = &self.prepared.borrow().journal
                    && crate::update_journal::pending(&self.executable)?
                {
                    journal.finish()?;
                }
                Ok(())
            }
            "rollback_binary" => self.rollback_binary(),
            "rollback_data" => {
                if self.prepared.borrow().swapped {
                    let backup = self
                        .prepared
                        .borrow()
                        .journal
                        .as_ref()
                        .ok_or("update recovery journal absent")?
                        .backup()?;
                    crate::backup_cli::restore_rollback(
                        &backup,
                        &self.directory.join("backup.key"),
                        &self.directory,
                        Some(&self.config),
                    )?;
                    self.prepared
                        .borrow_mut()
                        .journal
                        .as_mut()
                        .ok_or("update recovery journal absent")?
                        .phase(crate::update_journal::Phase::Restored)?;
                }
                Ok(())
            }
            _ => Err("invalid update transition".to_owned()),
        }
    }
}
pub(crate) fn verify_executable(staged: &Path, version: &str) -> Result<String, String> {
    let new_hash = crate::release::hash(staged)?;
    let output = Native.run(Request {
        environment: None,
        progress: None,
        program: staged.to_owned(),
        args: vec!["--version".into()],
        input: None,
        timeout: Duration::from_secs(10),
        limit: 4096,
    })?;
    if !output.success
        || std::str::from_utf8(&output.stdout)
            .map_err(|_| "invalid release version output")?
            .trim()
            != format!("elzevir {}", version.trim_start_matches('v'))
    {
        return Err("release executable version mismatch".to_owned());
    }
    if crate::release::hash(staged)? != new_hash {
        return Err("release executable changed during its version check".to_owned());
    }
    Ok(new_hash)
}

impl Context {
    fn prepare(&self) -> Result<(), String> {
        let version = if self.version == "latest" {
            crate::release::latest(&crate::release::Http)?.tag_name
        } else {
            self.version.clone()
        };
        if !crate::release::valid_version(&version) {
            return Err("invalid update version".to_owned());
        }
        let staged = if let (Some(package), Some(checksums)) = (&self.package, &self.checksums) {
            self.stage_local(&version, package, checksums)?
        } else {
            crate::release::stage(
                &crate::release::Http,
                &version,
                self.prepared.borrow().scratch.path(),
            )?
        };
        let new_hash = verify_executable(&staged, &version)?;
        crate::file_owner::program(&staged, &self.executable, &self.name)?;
        let journal = crate::update_journal::Journal::begin(
            &self.executable,
            &self.directory,
            &self.config,
            &self.name,
            &self.prepared.borrow().previous,
            &version,
            new_hash,
        )?;
        let mut prepared = self.prepared.borrow_mut();
        prepared.staged = Some(staged);
        prepared.journal = Some(journal);
        Ok(())
    }
    fn swap(&self) -> Result<(), String> {
        let mut prepared = self.prepared.borrow_mut();
        let staged = prepared.staged.as_ref().ok_or("verified update absent")?;
        if std::fs::symlink_metadata(&prepared.previous).is_ok() {
            return Err("previous-version destination must be new".to_owned());
        }
        let journal = prepared.journal.as_ref().ok_or("update journal absent")?;
        if crate::release::hash(&self.executable)? != journal.old_hash
            || crate::release::hash(staged)? != journal.new_hash
        {
            return Err("update executable changed before publication".to_owned());
        }
        prepared
            .journal
            .as_mut()
            .ok_or("update journal absent")?
            .phase(crate::update_journal::Phase::Swapping)?;
        std::fs::rename(&self.executable, &prepared.previous)
            .map_err(|_| "cannot retain previous executable")?;
        prepared.swapped = true;
        std::fs::rename(
            prepared.staged.as_ref().ok_or("verified update absent")?,
            &self.executable,
        )
        .map_err(|_| "cannot publish update executable")?;
        crate::installation::sync_directory(
            self.executable.parent().ok_or("program parent required")?,
        )
    }
    fn rollback_binary(&self) -> Result<(), String> {
        let mut prepared = self.prepared.borrow_mut();
        if prepared.swapped {
            let journal = prepared.journal.as_mut().ok_or("update journal absent")?;
            journal.phase(crate::update_journal::Phase::Recovering)?;
            journal.rollback_binary()?;
        }
        Ok(())
    }
    fn healthy(&self, new: bool) -> Result<(), String> {
        crate::host_jobs::wait_healthy(&self.directory)?;
        let expected = {
            let prepared = self.prepared.borrow();
            let journal = prepared.journal.as_ref().ok_or("update journal absent")?;
            if new {
                journal.new_version.clone()
            } else {
                journal.old_version.clone()
            }
        };
        let observed = crate::status::inspect(&self.directory);
        if observed
            .health
            .is_none_or(|health| health.version != expected)
        {
            return Err("service version did not match the update transaction".to_owned());
        }
        Ok(())
    }
    fn stage_local(
        &self,
        version: &str,
        package: &Path,
        checksums: &Path,
    ) -> Result<PathBuf, String> {
        use std::io::Read as _;
        if archive::regular(checksums)? > 65_536 || archive::regular(package)? > 512 * 1024 * 1024 {
            return Err("local release package or checksums exceed limit".to_owned());
        }
        let mut sums = String::new();
        std::fs::File::open(checksums)
            .and_then(|file| file.take(65_537).read_to_string(&mut sums))
            .map_err(|_| "release checksums unavailable")?;
        if sums.len() > 65_536 {
            return Err("release checksums exceed limit".to_owned());
        }
        let name = format!(
            "elzevir-{version}-{}.{}",
            crate::release::target()?,
            if cfg!(windows) { "zip" } else { "tar.gz" }
        );
        if package.file_name().and_then(|value| value.to_str()) != Some(name.as_str()) {
            return Err("package does not match release/platform".to_owned());
        }
        let expected = crate::release::checksum(&sums, &name)?;
        let prepared = self.prepared.borrow();
        let staged = prepared.scratch.path().join(&name);
        archive::write_new(&staged, &[])?;
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .open(&staged)
            .map_err(|_| "cannot stage local release")?;
        let mut input = std::fs::File::open(package)
            .map_err(|_| "local release unavailable")?
            .take(512 * 1024 * 1024 + 1);
        if std::io::copy(&mut input, &mut output).map_err(|_| "cannot stage local release")?
            > 512 * 1024 * 1024
        {
            return Err("local release exceeds limit".to_owned());
        }
        output.sync_all().map_err(|_| "cannot sync local release")?;
        if crate::release::hash(&staged)? != expected {
            return Err("release checksum mismatch".to_owned());
        }
        crate::release::unpack(&staged, prepared.scratch.path(), cfg!(windows))
    }
}

pub(crate) fn backup_stopped(
    directory: &Path,
    settings: &crate::server_config::Settings,
) -> Result<PathBuf, String> {
    crate::backup_cli::make_directory(&directory.join("backups"))?;
    let path = crate::backup_cli::artifact(directory, &archive::identifier()?)?;
    archive::create(
        directory,
        &crate::server_config::snapshot(settings)
            .map_err(|_| "cannot snapshot update configuration")?,
        &path,
        None,
    )?;
    crate::file_owner::private_tree(&path, directory)?;
    archive::verify(&path, &directory.join("backup.key"))?;
    Ok(path)
}

fn backup(directory: &Path, config: &Path, stopped: bool) -> Result<PathBuf, String> {
    if stopped || crate::status::inspect(directory).state == "stopped" {
        let _lock = crate::backup_cli::lock(directory)?;
        let settings = crate::server_config::ServeArgs::for_status(Some(config.to_owned()), None)
            .resolve()
            .map_err(|_| "update configuration unavailable")?;
        return backup_stopped(directory, &settings);
    }
    let info = crate::runtime_info::read(&directory.join(crate::runtime_info::NAME))?;
    let job = crate::admin::start_backup(&info)?;
    let started = Instant::now();
    loop {
        let view = crate::admin::operations(&info)?;
        let current = view
            .jobs
            .iter()
            .find(|entry| entry.id == job.id)
            .ok_or("update backup job unavailable")?;
        match current.state {
            crate::operations::State::Completed => break,
            crate::operations::State::Queued | crate::operations::State::Running
                if started.elapsed() < Duration::from_mins(30) =>
            {
                std::thread::sleep(Duration::from_millis(200));
            }
            _ => return Err("update backup did not complete".to_owned()),
        }
    }
    let path = crate::backup_cli::artifact(directory, &job.id)?;
    archive::verify(&path, &directory.join("backup.key"))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    struct Fake {
        calls: RefCell<Vec<&'static str>>,
        fail: &'static str,
    }
    impl Backend for Fake {
        fn step(&self, name: &'static str) -> Result<(), String> {
            self.calls.borrow_mut().push(name);
            if name == self.fail {
                Err("forced failure".to_owned())
            } else {
                Ok(())
            }
        }
    }
    #[test]
    fn checksum_or_backup_failure_never_stops_the_service() {
        for fail in ["download", "backup"] {
            let fake = Fake {
                calls: RefCell::new(vec![]),
                fail,
            };
            assert!(transaction(&fake).is_err());
            assert!(!fake.calls.borrow().contains(&"stop"));
        }
    }
    #[test]
    fn failed_initial_stop_restarts_and_checks_the_old_service_without_swapping() {
        let fake = Fake {
            calls: RefCell::new(vec![]),
            fail: "stop",
        };
        assert!(transaction(&fake).is_err());
        assert_eq!(
            *fake.calls.borrow(),
            [
                "download",
                "backup",
                "stop",
                "start",
                "rollback_health",
                "finalize"
            ]
        );
    }
    #[test]
    fn failed_health_restores_both_database_and_binary_before_starting_the_old_version() {
        let fake = Fake {
            calls: RefCell::new(vec![]),
            fail: "health",
        };
        assert!(transaction(&fake).is_err());
        assert_eq!(
            *fake.calls.borrow(),
            [
                "download",
                "backup",
                "stop",
                "final_backup",
                "swap",
                "start",
                "health",
                "stop",
                "rollback_binary",
                "rollback_data",
                "start",
                "rollback_health",
                "finalize"
            ]
        );
    }
}
