// SPDX-License-Identifier: GPL-3.0-or-later
//! Owned systemd units and bounded, argument-based manager calls.

use super::{Backend, Plan};
use std::{
    io::{Read, Seek, SeekFrom, Write},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const MARKER: &str = "# Managed by Elzevir service v1.\n";

fn quote(path: &Path, exec: bool) -> Result<String, String> {
    let value = path.to_str().ok_or("service paths must be Unicode")?;
    if value.chars().any(char::is_control) {
        return Err("service paths contain controls".to_owned());
    }
    let mut quoted = String::from("\"");
    for c in value.chars() {
        match c {
            '\\' | '"' => {
                quoted.push('\\');
                quoted.push(c);
            }
            '%' => quoted.push_str("%%"),
            '$' if exec => quoted.push_str("$$"),
            _ => quoted.push(c),
        }
    }
    quoted.push('"');
    Ok(quoted)
}

pub(super) fn render(plan: &Plan) -> Result<String, String> {
    let executable = quote(&plan.executable, true)?;
    let config = quote(&plan.config, true)?;
    let data = quote(&plan.data_dir, false)?;
    let capability = if plan.privileged_port {
        "CapabilityBoundingSet=CAP_NET_BIND_SERVICE\nAmbientCapabilities=CAP_NET_BIND_SERVICE\n"
    } else {
        "CapabilityBoundingSet=\n"
    };
    Ok(format!(
        "{MARKER}[Unit]\nDescription=Elzevir library service ({name})\nAfter=network.target\n\n[Service]\nType=simple\nUser={account}\nUMask=0077\nWorkingDirectory=/\nExecStart={executable} serve --config {config}\nRestart=on-failure\nRestartSec=5s\nKillSignal=SIGTERM\nTimeoutStopSec=45s\nNoNewPrivileges=true\nProtectSystem=strict\nProtectHome=true\nReadWritePaths={data}\nRuntimeDirectory={name}\nRuntimeDirectoryMode=0700\nEnvironment=TMPDIR=/run/{name}\n{capability}\n[Install]\nWantedBy=multi-user.target\n",
        name = plan.name,
        account = plan.account
    ))
}

pub(super) struct Output {
    pub(super) success: bool,
    pub(super) stdout: String,
}
pub(super) trait Runner {
    fn run(&self, program: &str, args: &[&str]) -> Result<Output, String>;
}

pub(super) struct Native;
impl Runner for Native {
    fn run(&self, program: &str, args: &[&str]) -> Result<Output, String> {
        // Temporary files avoid pipe deadlock; only bounded output enters memory.
        let stdout = tempfile::tempfile().map_err(|_| "cannot capture service manager output")?;
        let stderr = tempfile::tempfile().map_err(|_| "cannot capture service manager errors")?;
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(
                stdout
                    .try_clone()
                    .map_err(|_| "cannot capture manager output")?,
            )
            .stderr(
                stderr
                    .try_clone()
                    .map_err(|_| "cannot capture manager errors")?,
            )
            .spawn()
            .map_err(|_| "cannot execute system service manager")?;
        let deadline = Instant::now() + Duration::from_mins(1);
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None)
                    if Instant::now() < deadline
                        && stdout.metadata().is_ok_and(|m| m.len() <= 16_384)
                        && stderr.metadata().is_ok_and(|m| m.len() <= 16_384) =>
                {
                    std::thread::sleep(Duration::from_millis(20));
                }
                _ => {
                    drop(child.kill());
                    drop(child.wait());
                    return Err("service manager failed or timed out".to_owned());
                }
            }
        };
        let mut stdout = stdout;
        stdout
            .seek(SeekFrom::Start(0))
            .map_err(|_| "cannot read manager output")?;
        let mut text = String::new();
        stdout
            .take(16_385)
            .read_to_string(&mut text)
            .map_err(|_| "invalid manager output")?;
        if text.len() > 16_384
            || stderr
                .metadata()
                .map_err(|_| "cannot inspect manager errors")?
                .len()
                > 16_384
        {
            return Err("service manager output exceeded limit".to_owned());
        }
        Ok(Output {
            success: status.success(),
            stdout: text,
        })
    }
}

pub(super) struct Systemd<R = Native> {
    directory: PathBuf,
    runner: R,
}
impl Default for Systemd {
    fn default() -> Self {
        Self {
            directory: PathBuf::from("/etc/systemd/system"),
            runner: Native,
        }
    }
}

impl<R: Runner> Systemd<R> {
    fn file(&self, name: &str) -> PathBuf {
        self.directory.join(format!("{name}.service"))
    }

    fn owned(&self, name: &str) -> Result<Option<String>, String> {
        let path = self.file(name);
        match std::fs::symlink_metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Ok(m) if m.is_file() && !m.is_symlink() && m.len() <= 65_536 => {}
            _ => return Err("service unit is not a bounded regular file".to_owned()),
        }
        let mut value = String::new();
        std::fs::File::open(&path)
            .and_then(|f| f.take(65_537).read_to_string(&mut value))
            .map_err(|_| "cannot inspect service unit")?;
        if value.len() > 65_536 || !value.starts_with(MARKER) {
            return Err("service unit is not managed by Elzevir".to_owned());
        }
        Ok(Some(value))
    }

    fn call(&self, args: &[&str]) -> Result<Output, String> {
        self.runner.run("/usr/bin/systemctl", args)
    }
    fn checked(&self, args: &[&str]) -> Result<(), String> {
        if self.call(args)?.success {
            Ok(())
        } else {
            Err(
                "systemd operation failed; inspect the system journal and account permissions"
                    .to_owned(),
            )
        }
    }
    fn reload(&self) -> Result<(), String> {
        self.checked(&["--no-ask-password", "daemon-reload"])
    }
    fn remove_unchanged(&self, name: &str, value: &str) -> Result<(), String> {
        if self.owned(name)?.as_deref() != Some(value) {
            return Err("service unit changed; refusing removal".to_owned());
        }
        std::fs::remove_file(self.file(name))
            .map_err(|_| "cannot remove owned service unit".to_owned())
    }
}

impl<R: Runner> Backend for Systemd<R> {
    fn install(&self, plan: &Plan) -> Result<(), String> {
        if self.owned(&plan.name)?.is_some() {
            return Err("service already exists; it was preserved".to_owned());
        }
        for path in [&plan.config, &plan.data_dir, &plan.executable] {
            if path.starts_with("/home") || path.starts_with("/root") {
                return Err(
                    "system services need permanent paths outside /home and /root".to_owned(),
                );
            }
        }
        let user = self.runner.run("/usr/bin/id", &["-u", &plan.account])?;
        if !user.success
            || user
                .stdout
                .trim()
                .parse::<u32>()
                .map_or(true, |uid| uid == 0)
        {
            return Err("service account must already exist and must not be root".to_owned());
        }
        let unit = plan.unit.as_deref().ok_or("missing systemd unit plan")?;
        let mut file = tempfile::NamedTempFile::new_in(&self.directory)
            .map_err(|_| "cannot create unit; use an elevated terminal")?;
        file.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o644))
            .map_err(|_| "cannot set unit permissions")?;
        file.write_all(unit.as_bytes())
            .and_then(|()| file.as_file().sync_all())
            .map_err(|_| "cannot write service unit")?;
        file.persist_noclobber(self.file(&plan.name))
            .map_err(|_| "cannot publish service unit without replacement")?;
        let name = format!("{}.service", plan.name);
        if let Err(error) = self
            .reload()
            .and_then(|()| self.checked(&["--no-ask-password", "enable", "--", &name]))
        {
            // A failed enable can leave symlinks; only this newly-created unit is touched.
            let disabled = self.checked(&["--no-ask-password", "disable", "--", &name]);
            let removed = self.remove_unchanged(&plan.name, unit);
            let reloaded = self.reload();
            if disabled.is_err() || removed.is_err() || reloaded.is_err() {
                return Err(format!(
                    "{error}; rollback incomplete; inspect the named unit"
                ));
            }
            return Err(error);
        }
        Ok(())
    }

    fn action(&self, action: &str, name: &str) -> Result<(), String> {
        let owned = self
            .owned(name)?
            .ok_or("managed service is not installed")?;
        let unit = format!("{name}.service");
        if action == "uninstall" {
            self.checked(&["--no-ask-password", "disable", "--now", "--", &unit])?;
            self.remove_unchanged(name, &owned)?;
            self.reload()
        } else {
            self.checked(&["--no-ask-password", action, "--", &unit])
        }
    }

    fn status(&self, name: &str) -> Result<serde_json::Value, String> {
        if self.owned(name)?.is_none() {
            return Ok(
                serde_json::json!({"schema":1,"manager":"systemd","name":name,"state":"not_installed"}),
            );
        }
        let output = self.call(&[
            "--no-ask-password",
            "--no-pager",
            "show",
            "--property=LoadState,ActiveState,SubState,MainPID",
            "--",
            &format!("{name}.service"),
        ])?;
        let field = |key: &str| {
            output
                .stdout
                .lines()
                .find_map(|line| line.strip_prefix(key))
        };
        let state = if output.success {
            match field("ActiveState=") {
                Some("active") => "running",
                Some("inactive") => "stopped",
                Some("failed") => "failed",
                Some("activating") => "starting",
                Some("deactivating") => "stopping",
                _ => "unavailable",
            }
        } else {
            "unavailable"
        };
        Ok(
            serde_json::json!({"schema":1,"manager":"systemd","name":name,"state":state,"pid":field("MainPID=").and_then(|p|p.parse::<u32>().ok())}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    struct Fake {
        calls: RefCell<Vec<Vec<String>>>,
        fail_enable: bool,
    }
    impl Runner for Fake {
        fn run(&self, program: &str, args: &[&str]) -> Result<Output, String> {
            self.calls
                .borrow_mut()
                .push(args.iter().map(|s| (*s).to_owned()).collect());
            Ok(Output {
                success: !(self.fail_enable && args.contains(&"enable")),
                stdout: if program.ends_with("/id") {
                    "1000\n".to_owned()
                } else {
                    "ActiveState=active\nMainPID=42\n".to_owned()
                },
            })
        }
    }
    fn plan() -> Plan {
        Plan {
            schema: 1,
            platform: "systemd",
            action: "install",
            name: "elzevir-test".to_owned(),
            executable: "/usr/local/bin/elzevir".into(),
            config: "/etc/elzevir/library.toml".into(),
            data_dir: "/var/lib/elzevir/test".into(),
            account: "elzevir".to_owned(),
            privileged_port: false,
            unit: None,
        }
    }

    #[test]
    fn unit_quoting_preserves_metacharacters_without_shell_expansion() {
        let mut plan = plan();
        plan.config = PathBuf::from("/etc/elzevir/a $x %i \\\".toml");
        let unit = render(&plan).unwrap();
        assert!(unit.contains("$$x %%i"));
        assert!(unit.contains("User=elzevir\n"));
        assert!(unit.contains("RuntimeDirectory=elzevir-test\n"));
        assert!(!unit.contains("AmbientCapabilities="));
        plan.privileged_port = true;
        assert!(
            render(&plan)
                .unwrap()
                .contains("AmbientCapabilities=CAP_NET_BIND_SERVICE")
        );
    }

    #[test]
    fn existing_units_are_preserved_and_failed_install_rolls_back_only_its_unit() {
        let root = tempfile::tempdir().unwrap();
        let backend = Systemd {
            directory: root.path().to_owned(),
            runner: Fake {
                calls: RefCell::default(),
                fail_enable: true,
            },
        };
        let mut plan = plan();
        plan.unit = Some(render(&plan).unwrap());
        let file = backend.file(&plan.name);
        std::fs::write(&file, "manual unit").unwrap();
        assert!(backend.install(&plan).is_err());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "manual unit");
        assert!(backend.runner.calls.borrow().is_empty());
        std::fs::remove_file(&file).unwrap();
        assert!(backend.install(&plan).is_err());
        assert!(!file.exists());
        assert!(
            backend
                .runner
                .calls
                .borrow()
                .iter()
                .any(|a| a.iter().any(|s| s == "disable"))
        );
    }

    #[test]
    fn owned_uninstall_uses_separate_arguments_and_never_removes_data() {
        let root = tempfile::tempdir().unwrap();
        let backend = Systemd {
            directory: root.path().to_owned(),
            runner: Fake {
                calls: RefCell::default(),
                fail_enable: false,
            },
        };
        let mut plan = plan();
        plan.unit = Some(render(&plan).unwrap());
        std::fs::write(root.path().join("institution.db"), "preserve").unwrap();
        backend.install(&plan).unwrap();
        assert_eq!(backend.status(&plan.name).unwrap()["pid"], 42);
        backend.action("uninstall", &plan.name).unwrap();
        assert!(!backend.file(&plan.name).exists());
        assert_eq!(
            std::fs::read_to_string(root.path().join("institution.db")).unwrap(),
            "preserve"
        );
    }
}
