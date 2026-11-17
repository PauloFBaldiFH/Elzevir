// SPDX-License-Identifier: GPL-3.0-or-later
//! Updating a connector preserves its previously observed running intent.
trait Backend {
    fn running(&self) -> Result<bool, String>;
    fn step(&self, action: &'static str) -> Result<(), String>;
}

fn transaction(backend: &impl Backend) -> Result<(), String> {
    if !backend.running()? {
        return backend.step("install");
    }
    if let Err(error) = backend.step("stop") {
        backend.step("start")?;
        return Err(error);
    }
    let installed = backend.step("install");
    let restarted = backend.step("start");
    installed.and(restarted)
}
use std::path::Path;
struct Native {
    service: String,
}
impl Backend for Native {
    fn running(&self) -> Result<bool, String> {
        let status = crate::service::native_status(&self.service)?;
        match status.get("state").and_then(serde_json::Value::as_str) {
            Some("not_installed" | "stopped") => Ok(false),
            Some("running") => Ok(true),
            _ => Err("wait until the connector has a stable service state".to_owned()),
        }
    }
    fn step(&self, action: &'static str) -> Result<(), String> {
        if action == "install" {
            crate::managed_tools::install(crate::managed_tools::Tool::Cloudflared).map(drop)
        } else {
            crate::service::native_action(action, &self.service)
        }
    }
}
pub(crate) fn execute(directory: &Path, tool: crate::managed_tools::Tool) -> Result<(), String> {
    crate::service::require_operator()?;
    if tool == crate::managed_tools::Tool::Rclone {
        return crate::managed_tools::install(tool).map(drop);
    }
    transaction(&Native {
        service: crate::service::tunnel_name(directory)?,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    struct Fake {
        running: bool,
        fail: &'static str,
        calls: RefCell<Vec<&'static str>>,
    }
    impl Backend for Fake {
        fn running(&self) -> Result<bool, String> {
            Ok(self.running)
        }
        fn step(&self, action: &'static str) -> Result<(), String> {
            self.calls.borrow_mut().push(action);
            if self.fail == action {
                Err("forced failure".to_owned())
            } else {
                Ok(())
            }
        }
    }
    #[test]
    fn running_connector_is_restarted_after_both_success_and_install_failure() {
        for fail in ["none", "install"] {
            let backend = Fake {
                running: true,
                fail,
                calls: RefCell::new(vec![]),
            };
            assert_eq!(transaction(&backend).is_ok(), fail == "none");
            assert_eq!(*backend.calls.borrow(), ["stop", "install", "start"]);
        }
    }
    #[test]
    fn stopped_connector_stays_stopped_and_stop_failure_prevents_installation() {
        let backend = Fake {
            running: false,
            fail: "none",
            calls: RefCell::new(vec![]),
        };
        transaction(&backend).unwrap();
        assert_eq!(*backend.calls.borrow(), ["install"]);
        let backend = Fake {
            running: true,
            fail: "stop",
            calls: RefCell::new(vec![]),
        };
        assert!(transaction(&backend).is_err());
        assert!(!backend.calls.borrow().contains(&"install"));
    }
}
