// SPDX-License-Identifier: GPL-3.0-or-later
//! Operator-initiated actions on the terminal host. Only a public catalog URL is copied.
use crate::{Mode, Panel, Section, TunnelState};
use base64::Engine as _;
use ratatui::crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use std::{
    io::Write as _,
    path::PathBuf,
    process::{Child, Command, Stdio},
};

#[derive(Default)]
pub(crate) struct Actions {
    opener: Option<PathBuf>,
    clipboard: bool,
    opening: Option<Child>,
}

impl Actions {
    pub(crate) fn detect() -> Self {
        let present = |key| std::env::var_os(key).is_some_and(|value| !value.is_empty());
        let remote = present("SSH_CONNECTION") || present("SSH_TTY") || present("SSH_CLIENT");
        let desktop =
            !remote && (cfg!(windows) || present("DISPLAY") || present("WAYLAND_DISPLAY"));
        let executable = if cfg!(windows) {
            "explorer.exe"
        } else {
            "xdg-open"
        };
        let opener = desktop
            .then(|| {
                std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                    .map(|dir| dir.join(executable))
                    .find(|path| path.is_absolute() && path.is_file())
            })
            .flatten();
        Self {
            opener,
            clipboard: std::env::var("TERM").map_or(cfg!(windows), |term| term != "dumb"),
            opening: None,
        }
    }

    pub(crate) fn help(&self) -> &'static str {
        if self.opener.is_some() {
            "address-desktop"
        } else {
            "address-terminal"
        }
    }

    pub(crate) fn poll(&mut self, panel: &mut Panel) {
        if let Some(child) = &mut self.opening {
            match child.try_wait() {
                Ok(None) => (),
                result => {
                    if !result.is_ok_and(|status| status.is_some_and(|s| s.success())) {
                        panel.access_notice = Some("address-open-failed");
                    }
                    self.opening = None;
                }
            }
        }
    }

    pub(crate) fn handle(&mut self, event: &Event, panel: &mut Panel) -> bool {
        let Event::Key(key) = event else { return false };
        if key.kind != KeyEventKind::Press
            || !matches!(key.modifiers, KeyModifiers::NONE | KeyModifiers::SHIFT)
            || panel.section != Section::Addresses
            || panel.form.is_some()
            || panel.palette
        {
            return false;
        }
        let KeyCode::Char(action @ ('Y' | 'B' | 'F')) = key.code else {
            return false;
        };
        let Mode::Live(state) = &panel.mode else {
            return false;
        };
        let ops = state
            .snapshot
            .as_ref()
            .filter(|_| state.age_seconds <= 5)
            .map(|s| &s.operations);
        let url = ops.and_then(|ops| match &ops.tunnel {
            TunnelState::Configured(tunnel) => crate::sharing::public_url(&tunnel.catalog),
            _ => None,
        });
        let folder = ops.and_then(|ops| ops.backup_folder.as_deref());
        panel.access_notice = Some(if action == 'Y' {
            if let Some(url) = url
                && self.clipboard
            {
                let content = base64::engine::general_purpose::STANDARD.encode(url);
                let mut out = std::io::stdout().lock();
                if write!(out, "\x1b]52;c;{content}\x07")
                    .and_then(|()| out.flush())
                    .is_ok()
                {
                    "address-copy-requested"
                } else {
                    "address-select-url"
                }
            } else {
                "address-select-url"
            }
        } else if let Some(program) = &self.opener {
            let target = if action == 'B' {
                url.map(std::ffi::OsStr::new)
            } else {
                folder
                    .filter(|path| path.is_absolute())
                    .map(std::path::Path::as_os_str)
            };
            if self.opening.is_some() {
                "address-opening"
            } else if let Some(target) = target {
                match Command::new(program)
                    .arg(target)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                {
                    Ok(child) => {
                        self.opening = Some(child);
                        "address-opening"
                    }
                    Err(_) => "address-open-failed",
                }
            } else {
                "address-open-failed"
            }
        } else {
            "address-terminal"
        });
        panel.scroll = 0;
        true
    }
}
