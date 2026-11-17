// SPDX-License-Identifier: GPL-3.0-or-later
//! Recovery validates the operator's journal before any service or data change.
use crate::update_journal::{Journal, Phase};
use std::path::{Path, PathBuf};
#[derive(clap::Args)]
pub(crate) struct Args {
    #[arg(long)]
    executable: PathBuf,
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long)]
    confirm_recovery: bool,
    #[arg(long)]
    dry_run: bool,
    #[arg(long)]
    wait: bool,
}
pub(crate) fn run(args: &Args) -> Result<(), String> {
    let journal = Journal::load(&args.executable)?;
    if let Some(config) = &args.config
        && crate::installation::absolute(config)? != journal.config
    {
        return Err("recovery configuration differs from the private journal".to_owned());
    }
    if args.dry_run {
        println!(
            "{}",
            serde_json::json!({"schema":1,"action":"recover_update","executable":journal.executable,"directory":journal.directory,"config":journal.config,"service":journal.service,"old_version":journal.old_version,"new_version":journal.new_version,"phase":journal.phase})
        );
        return Ok(());
    }
    if !args.confirm_recovery {
        return Err("update recovery requires --confirm-recovery".to_owned());
    }
    let id = crate::host_jobs::launch(
        &journal.directory,
        crate::host_jobs::Task::RecoverUpdate {
            executable: journal.executable.clone(),
            config: journal.config.clone(),
        },
    )?;
    println!("{}", serde_json::json!({"id":id,"state":"queued"}));
    if args.wait {
        crate::host_jobs::wait(&journal.directory, &id)?;
    }
    Ok(())
}
trait Backend {
    fn healthy(&self, journal: &Journal, hash: &str) -> Result<bool, String>;
    fn stop(&self, journal: &Journal) -> Result<(), String>;
    fn start(&self, journal: &Journal) -> Result<(), String>;
    fn restore(&self, journal: &Journal, backup: &Path) -> Result<(), String>;
}
struct Native;
impl Backend for Native {
    fn healthy(&self, journal: &Journal, hash: &str) -> Result<bool, String> {
        let expected = if hash == journal.new_hash {
            &journal.new_version
        } else {
            &journal.old_version
        };
        let status = crate::status::inspect(&journal.directory);
        Ok(status.state == "running"
            && status
                .health
                .is_some_and(|health| health.version == *expected))
    }
    fn stop(&self, journal: &Journal) -> Result<(), String> {
        crate::service::native_action("stop", &journal.service)
    }
    fn start(&self, journal: &Journal) -> Result<(), String> {
        crate::service::native_action("start", &journal.service)?;
        crate::host_jobs::wait_healthy(&journal.directory)
    }
    fn restore(&self, journal: &Journal, backup: &Path) -> Result<(), String> {
        if crate::restore_journal::pending(&journal.directory) {
            crate::restore_journal::recover(&journal.directory, Some(&journal.config))?;
        }
        crate::backup_cli::restore_rollback(
            backup,
            &journal.directory.join("backup.key"),
            &journal.directory,
            Some(&journal.config),
        )
    }
}
pub(crate) fn execute(directory: &Path, executable: &Path, config: &Path) -> Result<(), String> {
    crate::service::require_operator()?;
    let _lock = crate::installation::lock(executable)?;
    let mut journal = Journal::load(executable)?;
    if directory
        .canonicalize()
        .map_err(|_| "institution unavailable")?
        != journal.directory
        || crate::installation::absolute(config)? != journal.config
    {
        return Err("update recovery belongs to another selected institution".to_owned());
    }
    recover_with(&mut journal, &Native)
}
fn recover_with(journal: &mut Journal, backend: &impl Backend) -> Result<(), String> {
    let current = if journal.executable.exists() {
        Some(crate::release::hash(&journal.executable)?)
    } else {
        None
    };
    if current
        .as_ref()
        .is_some_and(|hash| *hash != journal.old_hash && *hash != journal.new_hash)
    {
        return Err(
            "executable changed outside the update; all recovery files were preserved".to_owned(),
        );
    }
    let pending_restore = crate::restore_journal::pending(&journal.directory);
    let restore_data = pending_restore
        || current.as_deref() != Some(journal.old_hash.as_str())
        || journal.has_backup()
            && matches!(
                journal.phase,
                Phase::Swapping | Phase::Starting | Phase::Recovering
            );
    if !pending_restore
        && let Some(hash) = &current
        && (*hash == journal.new_hash || !restore_data)
        && backend.healthy(journal, hash)?
    {
        return journal.finish();
    }
    let backup = if restore_data {
        Some(journal.backup()?)
    } else {
        None
    };
    backend.stop(journal)?;
    if journal.phase != Phase::Restored {
        journal.phase(Phase::Recovering)?;
    }
    journal.rollback_binary()?;
    if let Some(backup) = backup {
        backend.restore(journal, &backup)?;
    }
    journal.phase(Phase::Restored)?;
    backend.start(journal)?;
    if !backend.healthy(journal, &journal.old_hash)? {
        return Err("previous service did not confirm recovery; journal retained".to_owned());
    }
    journal.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    struct Fixture {
        _root: tempfile::TempDir,
        journal: Journal,
    }
    impl Fixture {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let directory = root.path().join("library");
            std::fs::create_dir(&directory).unwrap();
            let config = root.path().join("server.toml");
            std::fs::write(&config, b"fictional configuration").unwrap();
            let executable = root.path().join("elzevir");
            std::fs::write(&executable, b"old executable").unwrap();
            crate::installation::claim(&executable, &directory, &config, "elzevir").unwrap();
            let new = root.path().join("new");
            std::fs::write(&new, b"new executable").unwrap();
            let previous = root.path().join(format!(
                "elzevir-previous-{}{}",
                "a".repeat(32),
                if cfg!(windows) { ".exe" } else { "" }
            ));
            let journal = Journal::begin(
                &executable,
                &directory,
                &config,
                "elzevir",
                &previous,
                "v1.0.0",
                crate::release::hash(&new).unwrap(),
            )
            .unwrap();
            Self {
                _root: root,
                journal,
            }
        }
        fn swap(&mut self) {
            let backup = self.journal.directory.join("fixture.age");
            std::fs::write(&backup, b"fictional encrypted snapshot").unwrap();
            self.journal.protect_backup(&backup).unwrap();
            self.journal.phase(Phase::Swapping).unwrap();
            std::fs::rename(&self.journal.executable, self.journal.previous().unwrap()).unwrap();
            std::fs::write(&self.journal.executable, b"new executable").unwrap();
            self.journal.phase(Phase::Starting).unwrap();
        }
    }
    struct Fake {
        healthy: Cell<bool>,
        fail_start: bool,
        calls: RefCell<Vec<&'static str>>,
    }
    impl Fake {
        fn new(healthy: bool) -> Self {
            Self {
                healthy: Cell::new(healthy),
                fail_start: false,
                calls: RefCell::new(vec![]),
            }
        }
    }
    impl Backend for Fake {
        fn healthy(&self, _: &Journal, _: &str) -> Result<bool, String> {
            Ok(self.healthy.get())
        }
        fn stop(&self, _: &Journal) -> Result<(), String> {
            self.calls.borrow_mut().push("stop");
            self.healthy.set(false);
            Ok(())
        }
        fn start(&self, _: &Journal) -> Result<(), String> {
            self.calls.borrow_mut().push("start");
            if self.fail_start {
                return Err("forced start failure".to_owned());
            }
            self.healthy.set(true);
            Ok(())
        }
        fn restore(&self, _: &Journal, _: &Path) -> Result<(), String> {
            self.calls.borrow_mut().push("restore");
            Ok(())
        }
    }
    #[test]
    fn interrupted_stop_restarts_the_old_version_without_requiring_a_cold_backup() {
        let mut fixture = Fixture::new();
        fixture.journal.phase(Phase::Starting).unwrap();
        let backend = Fake::new(false);
        recover_with(&mut fixture.journal, &backend).unwrap();
        assert_eq!(*backend.calls.borrow(), ["stop", "start"]);
        assert!(!crate::update_journal::pending(&fixture.journal.executable).unwrap());
    }
    #[test]
    fn healthy_new_version_finishes_without_reverting_database_writes() {
        let mut fixture = Fixture::new();
        fixture.swap();
        let backend = Fake::new(true);
        recover_with(&mut fixture.journal, &backend).unwrap();
        assert!(backend.calls.borrow().is_empty());
        assert_eq!(
            std::fs::read(&fixture.journal.executable).unwrap(),
            b"new executable"
        );
    }
    #[test]
    fn healthy_old_binary_during_rollback_still_requires_database_recovery() {
        let mut fixture = Fixture::new();
        fixture.swap();
        fixture.journal.phase(Phase::Recovering).unwrap();
        fixture.journal.rollback_binary().unwrap();
        let backend = Fake::new(true);
        recover_with(&mut fixture.journal, &backend).unwrap();
        assert_eq!(*backend.calls.borrow(), ["stop", "restore", "start"]);
    }
    #[test]
    fn divergent_binary_and_tampered_backup_are_preserved_before_service_changes() {
        for tamper_binary in [true, false] {
            let mut fixture = Fixture::new();
            fixture.swap();
            let target = if tamper_binary {
                fixture.journal.executable.clone()
            } else {
                fixture.journal.backup().unwrap()
            };
            std::fs::write(&target, b"unexpected contents").unwrap();
            let backend = Fake::new(false);
            assert!(recover_with(&mut fixture.journal, &backend).is_err());
            assert!(backend.calls.borrow().is_empty());
            assert_eq!(std::fs::read(&target).unwrap(), b"unexpected contents");
            assert!(crate::update_journal::pending(&fixture.journal.executable).unwrap());
        }
    }
    #[test]
    fn retry_after_failed_restart_does_not_restore_the_database_twice() {
        let mut fixture = Fixture::new();
        fixture.swap();
        let mut failing = Fake::new(false);
        failing.fail_start = true;
        assert!(recover_with(&mut fixture.journal, &failing).is_err());
        assert_eq!(*failing.calls.borrow(), ["stop", "restore", "start"]);
        let mut reloaded = Journal::load(&fixture.journal.executable).unwrap();
        let backend = Fake::new(false);
        recover_with(&mut reloaded, &backend).unwrap();
        assert_eq!(*backend.calls.borrow(), ["stop", "start"]);
    }
    #[test]
    fn explicit_configuration_with_missing_parent_is_rejected_even_in_dry_run() {
        let fixture = Fixture::new();
        assert!(
            run(&Args {
                executable: fixture.journal.executable.clone(),
                config: Some(fixture.journal.directory.join("missing/server.toml")),
                confirm_recovery: false,
                dry_run: true,
                wait: false,
            })
            .is_err()
        );
    }
}
