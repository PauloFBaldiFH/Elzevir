// SPDX-License-Identifier: GPL-3.0-or-later
//! System tray: shows whether the server is running and offers the daily
//! shortcuts (panel, catalog, staff, start and stop) without a terminal.
//!
//! The state and the menu are decided here, platform-free and tested; each
//! platform only draws them and reports which entry was chosen.

#[cfg(target_os = "linux")]
mod linux;

use std::path::PathBuf;
use std::time::Duration;

use crate::server_config::ServeArgs;

/// How often the tray looks at the server.
#[cfg(target_os = "linux")]
const POLL: Duration = Duration::from_secs(3);

/// Arguments of `elzevir tray`.
#[derive(clap::Args)]
pub(crate) struct Args {
    #[command(flatten)]
    serve: ServeArgs,
}

/// What the tray shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum State {
    /// No institution yet: the panel's first use creates it.
    NotConfigured,
    Starting,
    Running,
    /// Answering, with a failed health check.
    Degraded,
    Stopping,
    Stopped,
    /// Locked by a process that does not answer.
    Unavailable,
}

/// One menu entry the operator can choose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    OpenPanel,
    OpenCatalog,
    OpenStaff,
    Start,
    Stop,
    Quit,
}

impl State {
    fn observed(state: &str) -> Self {
        match state {
            "running" => Self::Running,
            "degraded" => Self::Degraded,
            "stopped" => Self::Stopped,
            _ => Self::Unavailable,
        }
    }

    /// Fluent key of the status line.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::NotConfigured => "tray-not-configured",
            Self::Starting => "tray-starting",
            Self::Running => "tray-running",
            Self::Degraded => "tray-degraded",
            Self::Stopping => "tray-stopping",
            Self::Stopped => "tray-stopped",
            Self::Unavailable => "tray-unavailable",
        }
    }

    /// Status dot colour, RGB.
    fn colour(self) -> [u8; 3] {
        match self {
            Self::Running => [0x2E, 0x9E, 0x5B],
            Self::Degraded | Self::Starting | Self::Stopping => [0xE0, 0xA1, 0x1B],
            Self::Unavailable => [0xC6, 0x3B, 0x32],
            Self::Stopped | Self::NotConfigured => [0x9C, 0xA5, 0xB3],
        }
    }
}

/// The entries offered in `state`, with their Fluent keys; disabled entries
/// are left out so the menu never offers what cannot happen.
pub(crate) fn menu(state: State) -> Vec<(Action, &'static str)> {
    let online = matches!(state, State::Running | State::Degraded);
    let mut entries = vec![(Action::OpenPanel, "tray-open-panel")];
    if online {
        entries.push((Action::OpenCatalog, "tray-open-catalog"));
        entries.push((Action::OpenStaff, "tray-open-staff"));
        entries.push((Action::Stop, "tray-stop"));
    }
    if state == State::Stopped {
        entries.push((Action::Start, "tray-start"));
    }
    entries.push((Action::Quit, "tray-quit"));
    entries
}

/// A 32×32 ARGB32 (network byte order) icon: a slate book with a status dot.
pub(crate) fn icon(state: State) -> Vec<u8> {
    const SIZE: i32 = 32;
    let cover = [0x52, 0x5A, 0x66];
    let paper = [0xF2, 0xEE, 0xE4];
    let dot = state.colour();
    let mut pixels = Vec::with_capacity(4 * 32 * 32);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (dx, dy) = (x - 23, y - 23);
            let distance = dx * dx + dy * dy;
            let colour = if distance <= 36 {
                Some(dot)
            } else if distance <= 64 {
                Some(paper)
            } else if (3..=26).contains(&x) && (2..=29).contains(&y) {
                // Cover, with three page lines.
                Some(if (8..=21).contains(&x) && matches!(y, 9 | 14 | 19) {
                    paper
                } else {
                    cover
                })
            } else {
                None
            };
            match colour {
                Some([red, green, blue]) => pixels.extend([0xFF, red, green, blue]),
                None => pixels.extend([0, 0, 0, 0]),
            }
        }
    }
    pixels
}

/// Every state and menu text in one language, read once: the Fluent bundle
/// itself cannot move to the tray's own thread.
pub(crate) struct Labels(std::collections::BTreeMap<&'static str, String>);

impl Labels {
    pub(crate) fn new(language: &str) -> Result<Self, String> {
        let messages = elzevir_panel::Messages::new(language).map_err(|error| error.to_string())?;
        let states = [
            State::NotConfigured,
            State::Starting,
            State::Running,
            State::Degraded,
            State::Stopping,
            State::Stopped,
            State::Unavailable,
        ];
        let keys = states
            .into_iter()
            .flat_map(|state| {
                std::iter::once(state.label()).chain(menu(state).into_iter().map(|(_, key)| key))
            })
            .chain(["tray-tooltip"]);
        Ok(Self(keys.map(|key| (key, messages.get(key))).collect()))
    }

    pub(crate) fn get(&self, key: &str) -> String {
        self.0.get(key).cloned().unwrap_or_else(|| key.to_owned())
    }
}

/// What the platform tray needs from the institution.
pub(crate) struct Institution {
    serve: Option<(crate::server_config::Settings, Option<PathBuf>)>,
}

impl Institution {
    fn resolve(args: ServeArgs) -> Self {
        // Before the first use there is nothing to serve; the tray still
        // offers the panel, whose wizard creates the institution.
        Self {
            serve: crate::first_use::resolve(args).ok(),
        }
    }

    /// The current state, read from the server itself.
    pub(crate) fn state(&self) -> State {
        match &self.serve {
            None => State::NotConfigured,
            Some((settings, _)) => {
                State::observed(crate::status::inspect(&settings.data_dir).state)
            }
        }
    }

    /// Catalog and staff addresses of the running server.
    pub(crate) fn urls(&self) -> (Option<String>, Option<String>) {
        self.serve.as_ref().map_or((None, None), |(settings, _)| {
            crate::addresses::local_urls(&crate::status::inspect(&settings.data_dir))
        })
    }

    pub(crate) fn start(&self) -> Result<(), String> {
        let (settings, config) = self.serve.as_ref().ok_or("institution not configured")?;
        crate::background::launch(settings, config.as_deref())
    }

    pub(crate) fn stop(&self) -> Result<(), String> {
        let (settings, _) = self.serve.as_ref().ok_or("institution not configured")?;
        crate::background::stop(&settings.data_dir).map(drop)
    }
}

/// `elzevir tray`: started at login; also starts the server.
pub(crate) fn run(args: Args) -> Result<(), String> {
    let institution = Institution::resolve(args.serve);
    if institution.state() == State::Stopped {
        // At login the server starts even where no tray can be shown.
        if let Err(error) = institution.start() {
            eprintln!("elzevir: {error}");
        }
    }
    let language = crate::first_use::language(args_language(&institution)).to_owned();
    platform(institution, &language)
}

fn args_language(institution: &Institution) -> Option<&str> {
    institution
        .serve
        .as_ref()
        .map(|(settings, _)| settings.lang.as_str())
}

#[cfg(target_os = "linux")]
fn platform(institution: Institution, language: &str) -> Result<(), String> {
    linux::run(institution, language, POLL)
}

#[cfg(not(target_os = "linux"))]
fn platform(_: Institution, _: &str) -> Result<(), String> {
    // The server was started above; the icon itself comes in a later change.
    Err("the system tray is not available on this system yet".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_only_offers_what_the_state_allows() {
        let actions = |state| {
            menu(state)
                .into_iter()
                .map(|(action, _)| action)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            actions(State::Running),
            [
                Action::OpenPanel,
                Action::OpenCatalog,
                Action::OpenStaff,
                Action::Stop,
                Action::Quit
            ]
        );
        assert_eq!(
            actions(State::Stopped),
            [Action::OpenPanel, Action::Start, Action::Quit]
        );
        assert_eq!(
            actions(State::NotConfigured),
            [Action::OpenPanel, Action::Quit]
        );
        assert_eq!(actions(State::Starting), [Action::OpenPanel, Action::Quit]);
        assert_eq!(
            actions(State::Unavailable),
            [Action::OpenPanel, Action::Quit]
        );
    }

    #[test]
    fn observed_states_map_to_tray_states() {
        assert_eq!(State::observed("running"), State::Running);
        assert_eq!(State::observed("degraded"), State::Degraded);
        assert_eq!(State::observed("stopped"), State::Stopped);
        assert_eq!(State::observed("unavailable"), State::Unavailable);
    }

    #[test]
    fn every_label_exists_in_both_languages() {
        for language in ["en", "pt-BR"] {
            let messages = Labels::new(language).unwrap();
            let states = [
                State::NotConfigured,
                State::Starting,
                State::Running,
                State::Degraded,
                State::Stopping,
                State::Stopped,
                State::Unavailable,
            ];
            for state in states {
                assert_ne!(messages.get(state.label()), state.label());
                for (_, key) in menu(state) {
                    assert_ne!(messages.get(key), key);
                }
            }
            assert_ne!(messages.get("tray-tooltip"), "tray-tooltip");
        }
    }

    #[test]
    fn the_icon_is_a_full_square_with_the_state_colour() {
        let pixels = icon(State::Running);
        assert_eq!(pixels.len(), 4 * 32 * 32);
        let centre = 4 * (23 * 32 + 23);
        assert_eq!(pixels[centre..centre + 4], [0xFF, 0x2E, 0x9E, 0x5B]);
        assert_eq!(pixels[..4], [0, 0, 0, 0], "corners stay transparent");
    }
}
