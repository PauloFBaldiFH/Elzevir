// SPDX-License-Identifier: GPL-3.0-or-later
//! Owned `OpenRC` scripts with explicit quoted argv, never evaluated user strings.
use super::{
    Backend, Plan,
    systemd::{Native, Runner},
};
use std::{
    io::{Read, Write},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};
const MARKER: &str = "#!/sbin/openrc-run\n# Managed by Elzevir service v1.\n";
fn quote(path: &Path) -> Result<String, String> {
    let text = path.to_str().ok_or("OpenRC paths must be Unicode")?;
    if text.chars().any(char::is_control) {
        return Err("OpenRC paths must not contain controls".to_owned());
    }
    Ok(format!("'{}'", text.replace('\'', "'\\''")))
}
pub(super) fn render(plan: &Plan) -> Result<String, String> {
    if plan.privileged_port {
        return Err("OpenRC requires a non-privileged listener port".to_owned());
    }
    let binary = quote(&plan.executable)?;
    let arguments = super::arguments(plan)?
        .iter()
        .map(|argument| quote(Path::new(argument)))
        .collect::<Result<Vec<_>, _>>()?
        .join(" ");
    Ok(format!(
        "{MARKER}description='Elzevir library service'\npidfile='/run/{name}.pid'\ndepend() {{ need net; }}\nstart() {{\n  ebegin 'Starting Elzevir'\n  start-stop-daemon --start --background --make-pidfile --pidfile \"$pidfile\" --user '{account}' --umask 0077 --exec {binary} -- {arguments}\n  eend $?\n}}\nstop() {{\n  ebegin 'Stopping Elzevir'\n  start-stop-daemon --stop --retry TERM/45/KILL/5 --pidfile \"$pidfile\" --exec {binary}\n  result=$?\n  if [ \"$result\" -eq 0 ]; then rm -f -- \"$pidfile\"; fi\n  eend \"$result\"\n}}\nstatus() {{\n  if start-stop-daemon --status --pidfile \"$pidfile\" --exec {binary}; then\n    einfo 'elzevir-state=running'; return 0\n  else\n    einfo 'elzevir-state=stopped'; return 3\n  fi\n}}\n",
        name = plan.name,
        account = plan.account
    ))
}

pub(super) struct OpenRc<R = Native> {
    directory: PathBuf,
    runner: R,
}
impl Default for OpenRc {
    fn default() -> Self {
        Self {
            directory: "/etc/init.d".into(),
            runner: Native,
        }
    }
}
impl<R: Runner> OpenRc<R> {
    fn path(&self, name: &str) -> PathBuf {
        self.directory.join(name)
    }
    fn owned(&self, name: &str) -> Result<Option<String>, String> {
        let path = self.path(name);
        match std::fs::symlink_metadata(&path) {
            Ok(metadata)
                if metadata.is_file() && !metadata.is_symlink() && metadata.len() <= 65_536 => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            _ => return Err("OpenRC script must be a bounded regular file".to_owned()),
        }
        let mut text = String::new();
        std::fs::File::open(path)
            .and_then(|file| file.take(65_537).read_to_string(&mut text))
            .map_err(|_| "OpenRC script unavailable")?;
        if text.len() > 65_536 || !text.starts_with(MARKER) {
            return Err("OpenRC script is not managed by Elzevir".to_owned());
        }
        Ok(Some(text))
    }
    fn checked(&self, program: &str, args: &[&str]) -> Result<(), String> {
        if self.runner.run(program, args)?.success {
            Ok(())
        } else {
            Err("OpenRC refused the operation".to_owned())
        }
    }
    fn remove(&self, name: &str, expected: &str) -> Result<(), String> {
        if self.owned(name)?.as_deref() != Some(expected) {
            return Err("OpenRC script changed during operation".to_owned());
        }
        std::fs::remove_file(self.path(name))
            .map_err(|_| "cannot remove owned OpenRC script".to_owned())
    }
}
impl<R: Runner> Backend for OpenRc<R> {
    fn install(&self, plan: &Plan) -> Result<(), String> {
        if self.owned(&plan.name)?.is_some() {
            return Err("OpenRC service already exists".to_owned());
        }
        let account = self.runner.run("/usr/bin/id", &["-u", &plan.account])?;
        if !account.success
            || account
                .stdout
                .trim()
                .parse::<u32>()
                .map_or(true, |uid| uid == 0)
        {
            return Err("OpenRC requires an existing non-root account".to_owned());
        }
        let script = render(plan)?;
        let mut staged = tempfile::NamedTempFile::new_in(&self.directory)
            .map_err(|_| "cannot prepare OpenRC script")?;
        staged
            .write_all(script.as_bytes())
            .and_then(|()| staged.as_file().sync_all())
            .map_err(|_| "cannot write OpenRC script")?;
        staged
            .as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o755))
            .map_err(|_| "cannot set OpenRC script permissions")?;
        staged
            .persist_noclobber(self.path(&plan.name))
            .map_err(|_| "OpenRC script destination must be new")?;
        if self
            .checked("/sbin/rc-update", &["add", &plan.name, "default"])
            .is_err()
        {
            self.remove(&plan.name, &script)?;
            return Err("cannot enable OpenRC service".to_owned());
        }
        Ok(())
    }
    fn action(&self, action: &str, name: &str) -> Result<(), String> {
        let owned = self.owned(name)?.ok_or("OpenRC service is not installed")?;
        if action == "uninstall" {
            self.checked("/sbin/rc-service", &[name, "stop"])?;
            self.checked("/sbin/rc-update", &["del", name, "default"])?;
            self.remove(name, &owned)
        } else {
            self.checked("/sbin/rc-service", &[name, action])
        }
    }
    fn status(&self, name: &str) -> Result<serde_json::Value, String> {
        if self.owned(name)?.is_none() {
            return Ok(
                serde_json::json!({"schema":1,"manager":"openrc","state":"not_installed","name":name}),
            );
        }
        let output = self.runner.run("/sbin/rc-service", &[name, "status"])?;
        let state = if output.stdout.contains("elzevir-state=running") && output.success {
            "running"
        } else if output.stdout.contains("elzevir-state=stopped") {
            "stopped"
        } else {
            "unavailable"
        };
        Ok(serde_json::json!({"schema":1,"manager":"openrc","state":state,"name":name}))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paths_with_shell_metacharacters_are_literal_in_generated_functions() {
        let plan = Plan {
            role: super::super::Role::Server,
            schema: 1,
            platform: "openrc",
            action: "install",
            name: "elzevir-test".to_owned(),
            executable: "/opt/library ' $(false)/elzevir".into(),
            config: "/etc/library ' $(false).toml".into(),
            data_dir: "/var/lib/elzevir".into(),
            account: "elzevir".to_owned(),
            privileged_port: false,
            unit: None,
        };
        let text = render(&plan).unwrap();
        assert!(text.contains("'\\''"));
        assert!(!text.contains("eval "));
        assert!(text.contains("--retry TERM/45/KILL/5"));
    }
}
