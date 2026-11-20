// SPDX-License-Identifier: GPL-3.0-or-later
//! Linux tray over the `StatusNotifierItem` D-Bus protocol (KDE, XFCE,
//! Cinnamon, MATE, `LXQt`; GNOME with the `AppIndicator` extension).

use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use ksni::blocking::TrayMethods;

use super::{Action, Institution, State};

struct Tray {
    state: State,
    messages: super::Labels,
    chosen: Sender<Action>,
}

impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "elzevir".into()
    }

    fn title(&self) -> String {
        "Elzevir".into()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        vec![ksni::Icon {
            width: 32,
            height: 32,
            data: super::icon(self.state),
        }]
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: self.messages.get("tray-tooltip"),
            description: self.messages.get(self.state.label()),
            ..ksni::ToolTip::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        // A left click opens the panel, the most common wish. Sending fails
        // only after the loop below ended, when nothing is left to do.
        self.chosen.send(Action::OpenPanel).ok();
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        let mut items = vec![
            ksni::menu::StandardItem {
                label: self.messages.get(self.state.label()),
                enabled: false,
                ..ksni::menu::StandardItem::default()
            }
            .into(),
            ksni::MenuItem::Separator,
        ];
        for (action, key) in super::menu(self.state) {
            if action == Action::Quit {
                items.push(ksni::MenuItem::Separator);
            }
            items.push(
                ksni::menu::StandardItem {
                    label: self.messages.get(key),
                    activate: Box::new(move |tray: &mut Self| {
                        // Fails only after the loop ended; nothing is left to do.
                        tray.chosen.send(action).ok();
                    }),
                    ..ksni::menu::StandardItem::default()
                }
                .into(),
            );
        }
        items
    }
}

pub(super) fn run(institution: Institution, language: &str, poll: Duration) -> Result<(), String> {
    let messages = super::Labels::new(language)?;
    let (chosen, actions) = mpsc::channel();
    let institution = Arc::new(institution);
    let handle = Tray {
        state: institution.state(),
        messages,
        chosen,
    }
    .spawn()
    .map_err(|_| "no system tray is available in this desktop session")?;
    // A start or stop in progress shows instead of the observed state.
    let transition: Arc<Mutex<Option<State>>> = Arc::new(Mutex::new(None));
    loop {
        match actions.recv_timeout(poll) {
            Ok(Action::Quit) | Err(RecvTimeoutError::Disconnected) => break,
            Ok(action) => act(action, &institution, &transition),
            Err(RecvTimeoutError::Timeout) => {}
        }
        let shown = transition
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .unwrap_or_else(|| institution.state());
        if handle.update(|tray| tray.state = shown).is_none() {
            break;
        }
    }
    handle.shutdown().wait();
    Ok(())
}

fn act(action: Action, institution: &Arc<Institution>, transition: &Arc<Mutex<Option<State>>>) {
    match action {
        Action::OpenPanel => open_panel(),
        Action::OpenCatalog => open_url(institution.urls().0),
        Action::OpenStaff => open_url(institution.urls().1),
        Action::Start | Action::Stop => {
            let showing = if action == Action::Start {
                State::Starting
            } else {
                State::Stopping
            };
            *transition.lock().unwrap_or_else(PoisonError::into_inner) = Some(showing);
            let institution = Arc::clone(institution);
            let finished = Arc::clone(transition);
            // Starting can take up to a minute; the icon keeps answering.
            let spawned = std::thread::Builder::new()
                .name("elzevir-tray-action".into())
                .spawn(move || {
                    let result = if action == Action::Start {
                        institution.start()
                    } else {
                        institution.stop()
                    };
                    if let Err(error) = result {
                        eprintln!("elzevir: {error}");
                    }
                    *finished.lock().unwrap_or_else(PoisonError::into_inner) = None;
                });
            if spawned.is_err() {
                *transition.lock().unwrap_or_else(PoisonError::into_inner) = None;
            }
        }
        Action::Quit => {}
    }
}

/// Terminals tried, in order, when the desktop cannot launch our entry.
const TERMINALS: [(&str, &[&str]); 8] = [
    ("x-terminal-emulator", &["-e"]),
    ("gnome-terminal", &["--"]),
    ("konsole", &["-e"]),
    ("xfce4-terminal", &["-x"]),
    ("mate-terminal", &["-x"]),
    ("kitty", &[]),
    ("alacritty", &["-e"]),
    ("xterm", &["-e"]),
];

/// Opens the panel in a terminal: through the desktop's own launcher when
/// the entry is installed, so its preferred terminal is used.
fn open_panel() {
    let entry = std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".local/share")))
        .map(|data| data.join("applications/elzevir.desktop"));
    if let Some(entry) = entry.filter(|entry| entry.is_file())
        && detached(Command::new("gio").arg("launch").arg(&entry))
    {
        return;
    }
    let Ok(executable) = std::env::current_exe() else {
        return;
    };
    for (terminal, separator) in TERMINALS {
        let mut command = Command::new(terminal);
        command.args(separator).arg(&executable).arg("start");
        if detached(&mut command) {
            return;
        }
    }
    eprintln!("elzevir: no terminal found to open the panel; run elzevir");
}

fn open_url(url: Option<String>) {
    if let Some(url) = url
        && !detached(Command::new("xdg-open").arg(url))
    {
        eprintln!("elzevir: cannot open the browser");
    }
}

/// Starts a helper program without waiting for it; true when it started.
fn detached(command: &mut Command) -> bool {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        // The helper is reaped by a thread so it never lingers as a zombie.
        .map(|mut child| {
            drop(std::thread::spawn(move || child.wait()));
        })
        .is_ok()
}
