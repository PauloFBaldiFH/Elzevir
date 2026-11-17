// SPDX-License-Identifier: GPL-3.0-or-later
//! Managed restore keeps original files available until the service confirms health.
trait Backend {
    fn step(&self, name: &'static str) -> Result<(), String>;
}

fn transaction(backend: &impl Backend) -> Result<(), String> {
    backend.step("verify")?;
    if let Err(error) = backend.step("stop") {
        backend.step("start")?;
        backend.step("health")?;
        return Err(error);
    }
    let result = backend
        .step("restore")
        .and_then(|()| backend.step("start"))
        .and_then(|()| backend.step("health"));
    if result.is_err() {
        backend.step("stop")?;
        backend.step("rollback")?;
        backend.step("start")?;
        backend.step("health")?;
        return Err("restore failed; previous files and service were recovered".to_owned());
    }
    Ok(())
}
use std::{cell::RefCell, path::Path};
struct Native<'a> {
    directory: &'a Path,
    input: &'a Path,
    key: &'a Path,
    config: &'a Path,
    service: String,
    journal: RefCell<Option<crate::restore_journal::Journal>>,
}
impl Backend for Native<'_> {
    fn step(&self, name: &'static str) -> Result<(), String> {
        match name {
            "verify" => crate::backup_archive::extract(self.input, self.key).map(drop),
            "stop" | "start" => crate::service::native_action(name, &self.service),
            "health" => crate::host_jobs::wait_healthy(self.directory),
            "restore" => {
                let journal = crate::backup_cli::restore_managed(
                    self.input,
                    self.key,
                    self.directory,
                    self.config,
                )?;
                *self.journal.borrow_mut() = Some(journal);
                Ok(())
            }
            "rollback" => {
                if let Some(journal) = self.journal.borrow().as_ref() {
                    journal.recover_committed(self.directory)?;
                } else if crate::restore_journal::pending(self.directory) {
                    crate::restore_journal::recover(self.directory, Some(self.config))?;
                }
                Ok(())
            }
            _ => Err("invalid managed restore transition".to_owned()),
        }
    }
}
pub(crate) fn execute(
    directory: &Path,
    input: &Path,
    key: &Path,
    config: Option<&Path>,
) -> Result<(), String> {
    crate::service::require_operator()?;
    let config = config.ok_or("managed restore requires a configuration file")?;
    let executable = std::env::current_exe().map_err(|_| "restore executable unavailable")?;
    let _lock = crate::installation::lock(&executable)?;
    let service = crate::installation::selected_service(&executable, directory, config)?;
    transaction(&Native {
        directory,
        input,
        key,
        config,
        service,
        journal: RefCell::new(None),
    })
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
            let mut calls = self.calls.borrow_mut();
            let failed_before = calls.contains(&self.fail);
            calls.push(name);
            if name == self.fail && !failed_before {
                Err("forced failure".to_owned())
            } else {
                Ok(())
            }
        }
    }
    #[test]
    fn invalid_archive_never_stops_and_failed_stop_never_restores() {
        for fail in ["verify", "stop"] {
            let backend = Fake {
                calls: RefCell::new(vec![]),
                fail,
            };
            assert!(transaction(&backend).is_err());
            assert!(!backend.calls.borrow().contains(&"restore"));
            if fail == "verify" {
                assert!(!backend.calls.borrow().contains(&"stop"));
            }
        }
    }
    #[test]
    fn failed_health_stops_before_rollback_and_confirms_the_original_service() {
        let backend = Fake {
            calls: RefCell::new(vec![]),
            fail: "health",
        };
        assert!(transaction(&backend).is_err());
        assert_eq!(
            *backend.calls.borrow(),
            [
                "verify", "stop", "restore", "start", "health", "stop", "rollback", "start",
                "health"
            ]
        );
    }
    #[test]
    fn successful_restore_starts_once_and_never_rolls_back() {
        let backend = Fake {
            calls: RefCell::new(vec![]),
            fail: "none",
        };
        transaction(&backend).unwrap();
        assert_eq!(
            *backend.calls.borrow(),
            ["verify", "stop", "restore", "start", "health"]
        );
    }
}
