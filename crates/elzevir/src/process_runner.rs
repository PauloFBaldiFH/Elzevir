// SPDX-License-Identifier: GPL-3.0-or-later
//! Bounded argument-based child execution shared by optional operational tools.
use std::{
    ffi::OsString,
    io::{Read, Seek, SeekFrom, Write},
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub(crate) type Progress = std::sync::Arc<dyn Fn(&[u8]) -> Result<(), String> + Send + Sync>;

pub(crate) struct Request {
    pub environment: Option<Vec<(String, zeroize::Zeroizing<String>)>>,
    pub progress: Option<Progress>,
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub input: Option<zeroize::Zeroizing<Vec<u8>>>,
    pub timeout: Duration,
    pub limit: u64,
}
pub(crate) struct Output {
    pub success: bool,
    pub stdout: Vec<u8>,
}
impl Drop for Output {
    fn drop(&mut self) {
        use zeroize::Zeroize as _;
        self.stdout.zeroize();
    }
}
pub(crate) trait Runner {
    fn run(&self, request: Request) -> Result<Output, String>;
}
pub(crate) struct Native;
impl Runner for Native {
    fn run(&self, request: Request) -> Result<Output, String> {
        if request.limit > 4 * 1024 * 1024 || request.timeout > Duration::from_hours(24) {
            return Err("invalid process resource limits".to_owned());
        }
        let mut input = tempfile::tempfile().map_err(|_| "cannot prepare process input")?;
        if let Some(bytes) = request.input {
            input
                .write_all(&bytes)
                .map_err(|_| "cannot prepare process input")?;
        }
        input
            .seek(SeekFrom::Start(0))
            .map_err(|_| "cannot rewind process input")?;
        let stdout = tempfile::tempfile().map_err(|_| "cannot capture process output")?;
        let stderr = tempfile::tempfile().map_err(|_| "cannot capture process errors")?;
        let mut command = Command::new(&request.program);
        configure_environment(&mut command, request.environment.as_deref())?;
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt as _;
            command.process_group(0);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt as _;
            command.creation_flags(0x0800_0000 | 0x0000_0200);
        }
        let mut child = command
            .args(&request.args)
            .stdin(Stdio::from(input))
            .stdout(
                stdout
                    .try_clone()
                    .map_err(|_| "cannot capture process output")?,
            )
            .stderr(
                stderr
                    .try_clone()
                    .map_err(|_| "cannot capture process errors")?,
            )
            .spawn()
            .map_err(|_| "cannot start optional tool")?;
        let started = Instant::now();
        let mut observed = (0, 0);
        let status = loop {
            if let Some(progress) = &request.progress {
                if progress(&[]).is_err() {
                    terminate(&mut child);
                    return Err("optional tool interrupted".to_owned());
                }
                let current = (
                    stdout
                        .metadata()
                        .map_or(u64::MAX, |metadata| metadata.len()),
                    stderr
                        .metadata()
                        .map_or(u64::MAX, |metadata| metadata.len()),
                );
                if current != observed {
                    let result = snapshot(&stdout, request.limit)
                        .and_then(|bytes| progress(&bytes))
                        .and_then(|()| snapshot(&stderr, request.limit))
                        .and_then(|bytes| progress(&bytes));
                    if result.is_err() {
                        terminate(&mut child);
                        return Err(
                            "optional tool progress failed or exceeded its limit".to_owned()
                        );
                    }
                    observed = current;
                }
            }

            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None)
                    if started.elapsed() < request.timeout
                        && stdout
                            .metadata()
                            .is_ok_and(|metadata| metadata.len() <= request.limit)
                        && stderr
                            .metadata()
                            .is_ok_and(|metadata| metadata.len() <= request.limit) =>
                {
                    std::thread::sleep(Duration::from_millis(20));
                }
                _ => {
                    // Reap after any timeout/output/error path; never leak an unattended child.
                    terminate(&mut child);
                    return Err("optional tool failed, exceeded its limit or timed out".to_owned());
                }
            }
        };
        finish_output(stdout, &stderr, status.success(), request.limit)
    }
}

fn terminate(child: &mut std::process::Child) {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        if let Ok(raw) = i32::try_from(child.id())
            && let Some(pid) = rustix::process::Pid::from_raw(raw)
            && rustix::process::kill_process_group(pid, rustix::process::Signal::KILL).is_err()
        {
            tracing::debug!(event = "process.group_kill_refused");
        }
    }
    #[cfg(windows)]
    {
        if let Some(root) = std::env::var_os("SystemRoot")
            && let Ok(mut killer) =
                Command::new(std::path::PathBuf::from(root).join("System32/taskkill.exe"))
                    .args(["/T", "/F", "/PID", &child.id().to_string()])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
        {
            let started = Instant::now();
            while started.elapsed() < Duration::from_secs(2) {
                if killer.try_wait().is_ok_and(|status| status.is_some()) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            drop(killer.kill());
            drop(killer.wait());
        }
    }
    drop(child.kill());
    drop(child.wait());
}

fn configure_environment(
    command: &mut Command,
    environment: Option<&[(String, zeroize::Zeroizing<String>)]>,
) -> Result<(), String> {
    if let Some(environment) = environment {
        if environment
            .iter()
            .map(|(name, value)| name.len() + value.len() + 2)
            .sum::<usize>()
            > 24_000
            || environment.iter().any(|(name, value)| {
                !name.starts_with("RCLONE_")
                    || !name
                        .bytes()
                        .all(|byte| byte.is_ascii_uppercase() || byte == b'_')
                    || value.contains('\0')
            })
        {
            return Err("invalid private tool environment".to_owned());
        }
        clear_rclone_environment(command, std::env::vars_os().map(|(name, _)| name));
        for (name, value) in environment {
            command.env(name, value.as_str());
        }
    }
    Ok(())
}

fn clear_rclone_environment(command: &mut Command, names: impl Iterator<Item = OsString>) {
    for name in names {
        if name
            .to_string_lossy()
            .get(..7)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("RCLONE_"))
        {
            command.env_remove(name);
        }
    }
}

fn finish_output(
    mut stdout: std::fs::File,
    stderr: &std::fs::File,
    success: bool,
    limit: u64,
) -> Result<Output, String> {
    if stdout
        .metadata()
        .map_err(|_| "process output unavailable")?
        .len()
        > limit
        || stderr
            .metadata()
            .map_err(|_| "process errors unavailable")?
            .len()
            > limit
    {
        return Err("optional tool output exceeds limit".to_owned());
    }
    stdout
        .seek(SeekFrom::Start(0))
        .map_err(|_| "cannot rewind process output")?;
    let mut bytes = Vec::new();
    stdout
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "process output unavailable")?;
    Ok(Output {
        success,
        stdout: bytes,
    })
}

/// Positional reads leave the child's shared output cursor untouched.
fn snapshot(file: &std::fs::File, limit: u64) -> Result<Vec<u8>, String> {
    let length = file
        .metadata()
        .map_err(|_| "process output unavailable")?
        .len();
    if length > limit {
        return Err("process output exceeds limit".to_owned());
    }
    let mut bytes =
        vec![0_u8; usize::try_from(length).map_err(|_| "process output exceeds limit")?];
    let mut done = 0;
    while done < bytes.len() {
        let offset = u64::try_from(done).map_err(|_| "process output exceeds limit")?;
        let buffer = bytes
            .get_mut(done..)
            .ok_or("invalid process output offset")?;
        #[cfg(unix)]
        let read = {
            use std::os::unix::fs::FileExt;
            file.read_at(buffer, offset)
        };
        #[cfg(windows)]
        let read = {
            use std::os::windows::fs::FileExt;
            file.seek_read(buffer, offset)
        };
        #[cfg(not(any(unix, windows)))]
        let read: std::io::Result<usize> =
            Err(std::io::Error::other("process progress unavailable"));
        let read = read.map_err(|_| "cannot observe process output")?;
        if read == 0 {
            break;
        }
        done += read;
    }
    bytes.truncate(done);
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_case_inherited_rclone_flags_are_removed_without_changing_other_variables() {
        let mut command = Command::new("fictional-tool");
        clear_rclone_environment(
            &mut command,
            ["rclone_dump", "RcLoNe_CONFIG", "OTHER_CONFIG"]
                .into_iter()
                .map(OsString::from),
        );
        let removed: Vec<_> = command
            .get_envs()
            .filter_map(|(name, value)| {
                value
                    .is_none()
                    .then_some(name.to_string_lossy().into_owned())
            })
            .collect();
        assert_eq!(removed.len(), 2);
        assert!(removed.contains(&"rclone_dump".to_owned()));
        assert!(removed.contains(&"RcLoNe_CONFIG".to_owned()));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn silent_child_is_reaped_when_cooperative_control_refuses_progress() {
        let at = Instant::now();
        let result = Native.run(Request {
            environment: None,
            program: "/bin/sleep".into(),
            args: vec!["5".into()],
            input: None,
            progress: Some(std::sync::Arc::new(|_| {
                Err("fixture cancellation".to_owned())
            })),
            timeout: Duration::from_secs(6),
            limit: 128,
        });
        assert!(result.is_err());
        assert!(at.elapsed() < Duration::from_secs(2));
    }
    #[test]
    fn progress_reads_do_not_move_the_childs_shared_write_offset() {
        let mut writer = tempfile::tempfile().unwrap();
        let reader = writer.try_clone().unwrap();
        writer.write_all(b"first").unwrap();
        assert_eq!(snapshot(&reader, 100).unwrap(), b"first");
        writer.write_all(b"second").unwrap();
        assert_eq!(snapshot(&reader, 100).unwrap(), b"firstsecond");
    }
}
