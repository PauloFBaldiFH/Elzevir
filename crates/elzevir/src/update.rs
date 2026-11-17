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
    backend.step("stop")?;
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
        return Err("update failed; the previous version and database were restored".to_owned());
    }
    Ok(())
}

#[derive(clap::Args)]
pub(crate) struct Args {
    #[arg(long)]
    config: PathBuf,
    #[arg(long, default_value = "elzevir")]
    service: String,
    #[arg(long, default_value = "latest")]
    version: String,
    #[arg(long, requires = "checksums")]
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
pub(crate) fn run(args: Args) -> Result<(), String> {
    let settings = crate::server_config::ServeArgs::for_status(Some(args.config.clone()), None)
        .resolve()
        .map_err(|error| error.to_string())?;
    if !crate::service::valid_name(&args.service)
        || (args.version != "latest" && !crate::release::valid_version(&args.version))
    {
        return Err("invalid update service or version".to_owned());
    }
    if args.dry_run {
        println!(
            "{}",
            serde_json::json!({"schema":1,"action":"update","version":args.version,"target":crate::release::target()?,"service":args.service,"config":args.config})
        );
        return Ok(());
    }
    if !args.confirm_update {
        return Err("update requires --confirm-update".to_owned());
    }
    let directory = settings.data_dir;
    let task = crate::host_jobs::Task::Update {
        config: args
            .config
            .canonicalize()
            .map_err(|_| "update configuration unavailable")?,
        name: args.service,
        version: args.version,
        package: args.package,
        checksums: args.checksums,
        executable: std::env::current_exe()
            .map_err(|_| "update executable unavailable")?
            .canonicalize()
            .map_err(|_| "update executable unavailable")?,
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
    backup: Option<PathBuf>,
    previous: PathBuf,
    swapped: bool,
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
    archive::regular(&update.executable)?;
    let parent = update
        .executable
        .parent()
        .ok_or("update executable needs parent")?;
    let scratch = tempfile::tempdir_in(parent)
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
            backup: None,
            previous,
            swapped: false,
        }),
    };
    transaction(&context)
}

impl Backend for Context {
    fn step(&self, name: &'static str) -> Result<(), String> {
        match name {
            "download" => self.prepare(),
            "backup" => {
                let backup = backup(&self.directory, &self.config, false)?;
                self.prepared.borrow_mut().backup = Some(backup);
                Ok(())
            }
            "final_backup" => {
                let backup = backup(&self.directory, &self.config, true)?;
                self.prepared.borrow_mut().backup = Some(backup);
                Ok(())
            }
            "stop" => crate::service::native_action("stop", &self.name),
            "swap" => self.swap(),
            "start" => crate::service::native_action("start", &self.name),
            "health" | "rollback_health" => crate::host_jobs::wait_healthy(&self.directory),
            "rollback_binary" => self.rollback_binary(),
            "rollback_data" => {
                if self.prepared.borrow().swapped {
                    let backup = self
                        .prepared
                        .borrow()
                        .backup
                        .clone()
                        .ok_or("update recovery backup absent")?;
                    crate::backup_cli::restore_rollback(
                        &backup,
                        &self.directory.join("backup.key"),
                        &self.directory,
                        Some(&self.config),
                    )?;
                }
                Ok(())
            }
            _ => Err("invalid update transition".to_owned()),
        }
    }
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
            if archive::regular(checksums)? > 65_536 {
                return Err("release checksums exceed limit".to_owned());
            }
            let sums =
                std::fs::read_to_string(checksums).map_err(|_| "release checksums unavailable")?;
            let expected_name = format!(
                "elzevir-{version}-{}.{}",
                crate::release::target()?,
                if cfg!(windows) { "zip" } else { "tar.gz" }
            );
            if package.file_name().and_then(|name| name.to_str()) != Some(expected_name.as_str()) {
                return Err("package name does not match release version and platform".to_owned());
            }
            let expected = crate::release::checksum(&sums, &expected_name)?;
            if crate::release::hash(package)? != expected {
                return Err("release checksum mismatch".to_owned());
            }
            crate::release::unpack(
                package,
                self.prepared.borrow().scratch.path(),
                cfg!(windows),
            )?
        } else {
            crate::release::stage(
                &crate::release::Http,
                &version,
                self.prepared.borrow().scratch.path(),
            )?
        };
        let output = Native.run(Request {
            program: staged.clone(),
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
        self.prepared.borrow_mut().staged = Some(staged);
        Ok(())
    }
    fn swap(&self) -> Result<(), String> {
        let mut prepared = self.prepared.borrow_mut();
        let staged = prepared.staged.as_ref().ok_or("verified update absent")?;
        if std::fs::symlink_metadata(&prepared.previous).is_ok() {
            return Err("previous-version destination must be new".to_owned());
        }
        let permissions = std::fs::metadata(&self.executable)
            .map_err(|_| "installed executable unavailable")?
            .permissions();
        std::fs::set_permissions(staged, permissions)
            .map_err(|_| "cannot retain executable permissions")?;
        std::fs::rename(&self.executable, &prepared.previous)
            .map_err(|_| "cannot retain previous executable")?;
        prepared.swapped = true;
        std::fs::rename(
            prepared.staged.as_ref().ok_or("verified update absent")?,
            &self.executable,
        )
        .map_err(|_| "cannot publish update executable")?;
        Ok(())
    }
    fn rollback_binary(&self) -> Result<(), String> {
        let prepared = self.prepared.borrow();
        if prepared.swapped {
            if self.executable.exists() {
                std::fs::rename(
                    &self.executable,
                    prepared.scratch.path().join(if cfg!(windows) {
                        "failed.exe"
                    } else {
                        "failed"
                    }),
                )
                .map_err(|_| "cannot set aside failed executable")?;
            }
            std::fs::rename(&prepared.previous, &self.executable)
                .map_err(|_| "cannot restore previous executable")?;
        }
        Ok(())
    }
}

fn backup(directory: &Path, config: &Path, stopped: bool) -> Result<PathBuf, String> {
    let path = if stopped || crate::status::inspect(directory).state == "stopped" {
        let _lock = crate::backup_cli::lock(directory)?;
        crate::backup_cli::make_directory(&directory.join("backups"))?;
        let path = crate::backup_cli::artifact(directory, &archive::identifier()?)?;
        let settings = crate::server_config::ServeArgs::for_status(Some(config.to_owned()), None)
            .resolve()
            .map_err(|_| "update configuration unavailable")?;
        archive::create(
            directory,
            &crate::server_config::snapshot(&settings)
                .map_err(|_| "cannot snapshot update configuration")?,
            &path,
            None,
        )?;
        crate::file_owner::private_tree(&path, directory)?;
        path
    } else {
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
        crate::backup_cli::artifact(directory, &job.id)?
    };
    archive::extract(&path, &directory.join("backup.key"))?;
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
                "rollback_health"
            ]
        );
    }
}
