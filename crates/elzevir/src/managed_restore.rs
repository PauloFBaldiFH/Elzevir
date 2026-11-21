// SPDX-License-Identifier: GPL-3.0-or-later
//! Managed restore keeps original files available until the service confirms health.
pub(crate) const BEFORE: &str = "restore_before_swap";
pub(crate) const PENDING: &str = "restore_recovery_pending";
pub(crate) const RECOVERED: &str = "restore_recovered";
pub(crate) fn signature(manifest: &crate::backup_archive::Manifest) -> Result<String, String> {
    use sha2::Digest as _;
    Ok(format!(
        "{:x}",
        sha2::Sha256::digest(serde_json::to_vec(manifest).map_err(|_| BEFORE)?)
    ))
}
trait Backend {
    fn step(&self, name: &'static str) -> Result<(), String>;
}

fn transaction(backend: &impl Backend) -> Result<(), String> {
    backend
        .step("verify")
        .map_err(|e| if e == PENDING { PENDING } else { BEFORE })?;
    if backend.step("stop").is_err() {
        backend
            .step("start")
            .and_then(|()| backend.step("health"))
            .map_err(|_| PENDING)?;
        return Err(BEFORE.into());
    }
    let result = backend
        .step("restore")
        .and_then(|()| backend.step("start"))
        .and_then(|()| backend.step("health"));
    if let Err(error) = result {
        if error == BEFORE {
            backend
                .step("start")
                .and_then(|()| backend.step("health"))
                .map_err(|_| PENDING)?;
            return Err(error);
        }
        for step in ["stop", "rollback", "start", "health"] {
            backend.step(step).map_err(|_| PENDING)?;
        }
        return Err(RECOVERED.into());
    }
    Ok(())
}
use std::{cell::RefCell, path::Path};
struct Native<'a> {
    directory: &'a Path,
    reviewed: &'a str,
    prepared: RefCell<Option<crate::backup_cli::Prepared>>,
    input: &'a Path,
    key: &'a Path,
    config: &'a Path,
    service: String,
    journal: RefCell<Option<crate::restore_journal::Journal>>,
}
impl Backend for Native<'_> {
    fn step(&self, name: &'static str) -> Result<(), String> {
        match name {
            "verify" => {
                let extracted = crate::backup_archive::extract(self.input, self.key)?;
                if signature(&extracted.manifest)? != self.reviewed {
                    return Err(BEFORE.into());
                }
                if crate::restore_journal::pending(self.directory) {
                    return Err(PENDING.into());
                }
                self.prepared.replace(Some(crate::backup_cli::prepare(
                    extracted,
                    self.directory,
                    true,
                )?));
                Ok(())
            }
            "stop" | "start" => crate::service::native_action(name, &self.service),
            "health" => crate::host_jobs::wait_healthy(self.directory),
            "restore" => {
                let journal = crate::backup_cli::restore_managed(
                    self.prepared.take().ok_or(BEFORE)?,
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
    reviewed: &str,
) -> Result<(), String> {
    crate::service::require_operator()?;
    let config = config.ok_or("managed restore requires a configuration file")?;
    let executable = std::env::current_exe().map_err(|_| "restore executable unavailable")?;
    let _lock = crate::installation::lock(&executable)?;
    let service = crate::installation::selected_service(&executable, directory, config)?;
    transaction(&Native {
        directory,
        reviewed,
        prepared: RefCell::new(None),
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
    fn each_failure_reports_whether_recovery_completed() {
        struct FailedRecovery(&'static str, std::cell::Cell<bool>);
        impl Backend for FailedRecovery {
            fn step(&self, step: &'static str) -> Result<(), String> {
                if step == "restore" {
                    self.1.set(true);
                    return Err("rebuild".into());
                }
                if self.1.get() && step == self.0 {
                    return Err("forced recovery failure".into());
                }
                Ok(())
            }
        }
        for (fail, expected) in [
            ("verify", BEFORE),
            ("stop", BEFORE),
            ("restore", RECOVERED),
            ("start", RECOVERED),
            ("health", RECOVERED),
        ] {
            let backend = Fake {
                calls: RefCell::default(),
                fail,
            };
            assert_eq!(transaction(&backend).unwrap_err(), expected);
        }
        for fail in ["stop", "rollback", "start", "health"] {
            assert_eq!(
                transaction(&FailedRecovery(fail, false.into())).unwrap_err(),
                PENDING
            );
        }
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
