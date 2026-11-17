// SPDX-License-Identifier: GPL-3.0-or-later
//! Bounded argument-based child execution shared by optional operational tools.
use std::{
    ffi::OsString,
    io::{Read, Seek, SeekFrom, Write},
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub(crate) struct Request {
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
        let mut stdout = tempfile::tempfile().map_err(|_| "cannot capture process output")?;
        let stderr = tempfile::tempfile().map_err(|_| "cannot capture process errors")?;
        let mut child = Command::new(&request.program)
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
        let status = loop {
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
        if stdout
            .metadata()
            .map_err(|_| "process output unavailable")?
            .len()
            > request.limit
            || stderr
                .metadata()
                .map_err(|_| "process errors unavailable")?
                .len()
                > request.limit
        {
            return Err("optional tool output exceeds limit".to_owned());
        }
        stdout
            .seek(SeekFrom::Start(0))
            .map_err(|_| "cannot rewind process output")?;
        let mut bytes = Vec::new();
        stdout
            .take(request.limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "process output unavailable")?;
        Ok(Output {
            success: status.success(),
            stdout: bytes,
        })
    }
}
