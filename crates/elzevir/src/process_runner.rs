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
        if let Some(environment) = &request.environment {
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
            for (name, _) in std::env::vars_os() {
                if name.to_string_lossy().starts_with("RCLONE_") {
                    command.env_remove(name);
                }
            }
            for (name, value) in environment {
                command.env(name, value.as_str());
            }
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
                        drop(child.kill());
                        drop(child.wait());
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
                    drop(child.kill());
                    drop(child.wait());
                    return Err("optional tool failed, exceeded its limit or timed out".to_owned());
                }
            }
        };
        finish_output(stdout, &stderr, status.success(), request.limit)
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
    fn progress_reads_do_not_move_the_childs_shared_write_offset() {
        let mut writer = tempfile::tempfile().unwrap();
        let reader = writer.try_clone().unwrap();
        writer.write_all(b"first").unwrap();
        assert_eq!(snapshot(&reader, 100).unwrap(), b"first");
        writer.write_all(b"second").unwrap();
        assert_eq!(snapshot(&reader, 100).unwrap(), b"firstsecond");
    }
}
