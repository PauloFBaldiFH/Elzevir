// SPDX-License-Identifier: GPL-3.0-or-later
//! The personal server runs in the background, apart from the panel, so
//! closing the panel never takes the catalog offline.

use std::ffi::OsString;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::server_config::{ServeArgs, Settings};

/// How long a starting server may take before the panel reports it.
const STARTUP: Duration = Duration::from_mins(1);
const POLL: Duration = Duration::from_millis(250);

/// Arguments of `elzevir background`.
#[derive(clap::Args)]
pub(crate) struct Args {
    #[command(flatten)]
    serve: ServeArgs,
    /// Serve in this process; used by the detached child.
    #[arg(long, hide = true)]
    attached: bool,
}

/// `elzevir background`: started at login, by the panel and by the tray.
pub(crate) fn run(args: Args) -> Result<(), String> {
    if args.attached {
        let (settings, _) = crate::first_use::resolve(args.serve)?;
        return crate::serve(&settings);
    }
    let (settings, config) = crate::first_use::resolve(args.serve)?;
    if running(&settings.data_dir) {
        return Ok(());
    }
    launch(&settings, config.as_deref())
}

fn running(directory: &Path) -> bool {
    matches!(
        crate::status::inspect(directory).state,
        "running" | "degraded"
    )
}

/// The child's command line: the resolved institution, never a new wizard.
fn child_arguments(settings: &Settings, config: Option<&Path>) -> Vec<OsString> {
    let mut arguments: Vec<OsString> = vec!["background".into(), "--attached".into()];
    if let Some(config) = config {
        arguments.push("--config".into());
        arguments.push(config.into());
    } else {
        arguments.push("--data-dir".into());
        arguments.push(settings.data_dir.clone().into());
    }
    arguments.push("--listen".into());
    arguments.push(settings.listen.to_string().into());
    arguments.push(format!("--insecure-cookies={}", settings.insecure_cookies).into());
    arguments.push("--lang".into());
    arguments.push(settings.lang.clone().into());
    arguments
}

/// Starts the server detached from this terminal and waits until it answers.
pub(crate) fn launch(settings: &Settings, config: Option<&Path>) -> Result<(), String> {
    let executable = std::env::current_exe().map_err(|_| "cannot locate the elzevir executable")?;
    let mut command = std::process::Command::new(executable);
    command
        .args(child_arguments(settings, config))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    detach(&mut command);
    let mut child = command
        .spawn()
        .map_err(|_| "cannot start the background server")?;
    let started = Instant::now();
    while started.elapsed() < STARTUP {
        if running(&settings.data_dir) {
            return Ok(());
        }
        if let Ok(Some(_)) = child.try_wait() {
            return Err(format!(
                "the background server stopped while starting; see the logs in {}",
                settings.data_dir.display()
            ));
        }
        std::thread::sleep(POLL);
    }
    Err("the background server did not answer in time".into())
}

#[cfg(unix)]
fn detach(command: &mut std::process::Command) {
    use std::os::unix::process::CommandExt;
    // Its own process group: Ctrl+C and closing the terminal do not reach it.
    command.process_group(0);
}

#[cfg(windows)]
fn detach(command: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    // No console window, and console events of this terminal do not reach it.
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
}

/// Starts the background server from `executable` and waits until it
/// answers: after an update replaced the program this process was started
/// from, or from the panel.
pub(crate) fn restart(executable: &Path, directory: &Path, config: &Path) -> Result<(), String> {
    let mut command = std::process::Command::new(executable);
    command.arg("background");
    if config.is_file() {
        command.arg("--config").arg(config);
    } else {
        command.arg("--data-dir").arg(directory);
    }
    let status = command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|_| "cannot start the updated server")?;
    if status.success() && running(directory) {
        Ok(())
    } else {
        Err("the server did not start; see the logs in the data directory".into())
    }
}

/// Asks a running server to stop and waits until its lock is released.
pub(crate) fn stop(directory: &Path) -> Result<bool, String> {
    if !running(directory) {
        return Ok(false);
    }
    let info = crate::runtime_info::read(&directory.join(crate::runtime_info::NAME))?;
    crate::admin::stop(&info)?;
    let started = Instant::now();
    while started.elapsed() < STARTUP {
        if crate::status::inspect(directory).state == "stopped" {
            return Ok(true);
        }
        std::thread::sleep(POLL);
    }
    Err("the server did not stop in time".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_child_serves_the_resolved_institution() {
        let mut serve = ServeArgs::default();
        serve.data_dir = Some(std::env::temp_dir().join("fictional-library"));
        let settings = serve.resolve().unwrap();
        let config = std::env::temp_dir().join("server.toml");
        let with_config = child_arguments(&settings, Some(&config));
        assert_eq!(with_config[..3], ["background", "--attached", "--config"]);
        assert_eq!(with_config[3], config.as_os_str());
        assert!(with_config.contains(&"--listen".into()));
        assert!(with_config.contains(&"127.0.0.1:7390".into()));
        assert!(with_config.contains(&"--insecure-cookies=false".into()));
        let without = child_arguments(&settings, None);
        assert_eq!(without[2], "--data-dir");
        assert_eq!(without[3], settings.data_dir.as_os_str());
    }

    #[test]
    fn stopping_a_stopped_institution_is_a_no_op() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("elzevir.lock"), b"").unwrap();
        assert_eq!(stop(directory.path()), Ok(false));
    }
}
