// SPDX-License-Identifier: GPL-3.0-or-later
//! `elzevir-tray.exe`: starts `elzevir tray` without a console window.
//!
//! Windows opens a console for every console program, so a sign-in shortcut
//! straight to `elzevir.exe tray` would leave a black window behind. This
//! program is built for the Windows subsystem, opens no console itself and
//! starts `elzevir.exe` next to it with a hidden one. It holds no logic, so
//! it seldom needs updating.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::process::{Command, ExitCode};

/// Gives the child a console without a window.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn main() -> ExitCode {
    let program = if cfg!(windows) {
        "elzevir.exe"
    } else {
        "elzevir"
    };
    let Ok(launcher) = std::env::current_exe() else {
        return ExitCode::FAILURE;
    };
    let mut command = Command::new(launcher.with_file_name(program));
    command.arg("tray").args(std::env::args_os().skip(1));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    // There is no console to report to; the exit code says whether it started.
    match command.spawn() {
        Ok(_) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}
