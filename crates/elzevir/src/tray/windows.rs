// SPDX-License-Identifier: GPL-3.0-or-later
//! Windows notification-area icon. `elzevir-tray.exe` starts `elzevir tray`
//! without a console window; here the icon and its menu run on winit's
//! message loop, which the Win32 tray needs on the same thread.

use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use winit::application::ApplicationHandler;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::WindowId;

use super::{Action, Controller, Labels, State};

/// The panel is a console program: it gets a console of its own, since the
/// tray's is hidden.
const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;

/// What reaches the message loop from the tray's own threads.
#[derive(Debug)]
enum Signal {
    Chosen(MenuId),
    Clicked,
    Redraw,
}

struct App<'a> {
    controller: &'a Controller,
    labels: Labels,
    proxy: EventLoopProxy<Signal>,
    poll: Duration,
    icon: Option<TrayIcon>,
    /// The state drawn and the menu entries offered for it.
    shown: Option<State>,
    entries: Vec<(MenuId, Action)>,
    failure: Option<String>,
}

impl App<'_> {
    fn draw(&mut self) -> Result<(), String> {
        let state = self.controller.shown();
        if self.shown == Some(state) {
            return Ok(());
        }
        let picture = picture(state)?;
        let tooltip = format!(
            "{}\n{}",
            self.labels.get("tray-tooltip"),
            self.labels.get(state.label())
        );
        let menu = Menu::new();
        let status = MenuItem::new(self.labels.get(state.label()), false, None);
        let failed = |_| "cannot build the tray menu".to_owned();
        menu.append(&status).map_err(failed)?;
        menu.append(&PredefinedMenuItem::separator())
            .map_err(failed)?;
        self.entries.clear();
        for (action, key) in super::menu(state) {
            if action == Action::Quit {
                menu.append(&PredefinedMenuItem::separator())
                    .map_err(failed)?;
            }
            let item = MenuItem::with_id(key, self.labels.get(key), true, None);
            menu.append(&item).map_err(failed)?;
            self.entries.push((item.id().clone(), action));
        }
        if let Some(icon) = &self.icon {
            icon.set_icon(Some(picture))
                .map_err(|_| "cannot redraw the tray icon")?;
            icon.set_tooltip(Some(tooltip))
                .map_err(|_| "cannot redraw the tray icon")?;
            icon.set_menu(Some(Box::new(menu)));
        } else {
            let icon = TrayIconBuilder::new()
                .with_icon(picture)
                .with_tooltip(tooltip)
                .with_menu(Box::new(menu))
                // A left click opens the panel; the menu is on the right.
                .with_menu_on_left_click(false)
                .build()
                .map_err(|_| "no notification area is available")?;
            self.icon = Some(icon);
        }
        self.shown = Some(state);
        Ok(())
    }

    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        if let Err(error) = self.draw() {
            self.failure = Some(error);
            event_loop.exit();
        }
    }

    fn act(&mut self, event_loop: &ActiveEventLoop, action: Action) {
        if action == Action::Quit {
            event_loop.exit();
            return;
        }
        let proxy = Mutex::new(self.proxy.clone());
        self.controller.act(action, move || {
            // Fails only once the loop has ended; nothing is left to redraw.
            proxy
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .send_event(Signal::Redraw)
                .ok();
        });
        self.redraw(event_loop);
    }
}

impl ApplicationHandler<Signal> for App<'_> {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        if matches!(
            cause,
            StartCause::Init | StartCause::ResumeTimeReached { .. }
        ) {
            self.redraw(event_loop);
            event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + self.poll));
        }
    }

    fn resumed(&mut self, _: &ActiveEventLoop) {}

    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}

    fn user_event(&mut self, event_loop: &ActiveEventLoop, signal: Signal) {
        match signal {
            Signal::Chosen(id) => {
                let chosen = self
                    .entries
                    .iter()
                    .find(|(entry, _)| *entry == id)
                    .map(|(_, action)| *action);
                if let Some(action) = chosen {
                    self.act(event_loop, action);
                }
            }
            Signal::Clicked => self.act(event_loop, Action::OpenPanel),
            Signal::Redraw => self.redraw(event_loop),
        }
    }
}

pub(super) fn run(controller: &Controller, language: &str, poll: Duration) -> Result<(), String> {
    let labels = Labels::new(language)?;
    let event_loop = EventLoop::<Signal>::with_user_event()
        .build()
        .map_err(|_| "cannot start the tray's message loop")?;
    let menu_proxy = Mutex::new(event_loop.create_proxy());
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        // Fails only once the loop has ended.
        menu_proxy
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .send_event(Signal::Chosen(event.id))
            .ok();
    }));
    let click_proxy = Mutex::new(event_loop.create_proxy());
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } = event
        {
            // Fails only once the loop has ended.
            click_proxy
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .send_event(Signal::Clicked)
                .ok();
        }
    }));
    let mut app = App {
        controller,
        labels,
        proxy: event_loop.create_proxy(),
        poll,
        icon: None,
        shown: None,
        entries: Vec::new(),
        failure: None,
    };
    event_loop
        .run_app(&mut app)
        .map_err(|_| "the tray's message loop failed")?;
    app.failure.map_or(Ok(()), Err)
}

/// The icon in the RGBA order Windows takes.
fn picture(state: State) -> Result<tray_icon::Icon, String> {
    let rgba = super::icon(state)
        .chunks_exact(4)
        .flat_map(|pixel| match pixel {
            [alpha, red, green, blue] => [*red, *green, *blue, *alpha],
            _ => [0; 4],
        })
        .collect();
    tray_icon::Icon::from_rgba(rgba, 32, 32).map_err(|_| "invalid tray icon".to_owned())
}

/// Opens the panel in a new console window (Windows Terminal when it is the
/// default terminal).
pub(super) fn open_panel() {
    let opened = std::env::current_exe().and_then(|executable| {
        Command::new(executable)
            .arg("start")
            .creation_flags(CREATE_NEW_CONSOLE)
            .spawn()
    });
    if opened.is_err() {
        eprintln!("elzevir: cannot open the panel; run elzevir");
    }
}

/// Opens an address in the default browser.
pub(super) fn open_url(url: Option<String>) {
    if let Some(url) = url
        && Command::new("explorer.exe")
            .arg(url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .is_err()
    {
        eprintln!("elzevir: cannot open the browser");
    }
}
