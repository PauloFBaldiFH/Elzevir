// SPDX-License-Identifier: GPL-3.0-or-later
//! Interactive terminal boundary. The demo never opens institution data.

use crate::{
    Institution, Messages, Mode, ObservationRequest, Panel, PanelError, Snapshot, worker::Worker,
};
use ratatui::crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, MouseButton, MouseEventKind},
    execute,
};
use std::io::{self, IsTerminal};
use std::time::{Duration, Instant};

/// Runs the demonstration, restoring the terminal before returning any error.
///
/// # Errors
/// Fails if translations cannot load or the terminal cannot be used or restored.
pub fn run(language: &str, ascii: bool) -> Result<(), PanelError> {
    let messages = prepare(language)?;
    terminal(&messages, ascii, Panel::default(), None, || false)
}

/// Runs the read-only dashboard with one background observation source.
///
/// # Errors
/// Fails if messages, the terminal or the polling thread cannot initialize.
pub fn run_live(
    language: &str,
    ascii: bool,
    institution: Institution,
    source: impl FnMut(ObservationRequest) -> Snapshot + Send + 'static,
) -> Result<(), PanelError> {
    run_live_until(language, ascii, institution, source, || false)
}

/// Runs the connected panel until the caller requests shutdown or the operator exits.
///
/// # Errors
/// Reports failures initializing the terminal, messages or observation worker.
pub fn run_live_until(
    language: &str,
    ascii: bool,
    institution: Institution,
    source: impl FnMut(ObservationRequest) -> Snapshot + Send + 'static,
    stopping: impl Fn() -> bool,
) -> Result<(), PanelError> {
    let messages = prepare(language)?;
    let worker = Worker::start(source)?;
    terminal(
        &messages,
        ascii,
        Panel::live(institution),
        Some(&worker),
        stopping,
    )
}

pub(crate) fn prepare(language: &str) -> Result<Messages, PanelError> {
    let messages = Messages::new(language)?;
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(io::Error::other(messages.get("panel-tty")).into());
    }
    Ok(messages)
}

fn terminal(
    messages: &Messages,
    ascii: bool,
    panel: Panel,
    worker: Option<&Worker>,
    stopping: impl Fn() -> bool,
) -> Result<(), PanelError> {
    with_terminal(|terminal| interact(terminal, messages, ascii, panel, worker, stopping))
}

pub(crate) fn with_terminal<T>(
    action: impl FnOnce(ratatui::DefaultTerminal) -> io::Result<T>,
) -> Result<T, PanelError> {
    let guard = TerminalGuard { active: true };
    let terminal = ratatui::try_init()?;
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        drop(execute!(io::stdout(), DisableMouseCapture));
        previous(info);
    }));
    execute!(io::stdout(), EnableMouseCapture)?;
    let result = action(terminal);
    let restored = guard.restore();
    let value = result?;
    restored?;
    Ok(value)
}

struct TerminalGuard {
    active: bool,
}

impl TerminalGuard {
    fn restore(mut self) -> io::Result<()> {
        let mouse = execute!(io::stdout(), DisableMouseCapture);
        let terminal = ratatui::try_restore();
        self.active = false;
        mouse.and(terminal)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        // Emergency restoration also covers partial terminal initialization.
        if self.active {
            drop(execute!(io::stdout(), DisableMouseCapture));
            drop(ratatui::try_restore());
        }
    }
}

fn interact(
    mut terminal: ratatui::DefaultTerminal,
    messages: &Messages,
    ascii: bool,
    mut panel: Panel,
    worker: Option<&Worker>,
    stopping: impl Fn() -> bool,
) -> io::Result<()> {
    let mut scene = crate::ui::Scene::default();
    let mut picker: Option<crate::folder_picker::Picker> = None;
    let mut observed_at: Option<Instant> = None;
    let mut action_sequence = None;
    let tick = Duration::from_millis(150);
    let mut last_tick = Instant::now();
    while !panel.quit && !stopping() {
        if let Some(worker) = worker {
            if let Some((snapshot, at, completed)) = worker.take()
                && let Mode::Live(state) = &mut panel.mode
            {
                state.snapshot = Some(snapshot);
                observed_at = Some(at);
                if action_sequence.is_some_and(|requested| completed >= requested) {
                    panel.diagnostics.pending = false;
                    action_sequence = None;
                }
            }
            if let Mode::Live(state) = &mut panel.mode {
                state.age_seconds = observed_at.map_or(0, |at| at.elapsed().as_secs());
            }
            if let Some(request) = panel.diagnostics.requested.take() {
                action_sequence = worker.request(request);
                if action_sequence.is_none() {
                    panel.diagnostics.pending = false;
                }
            } else if panel.refresh_requested {
                worker.refresh();
            }
            panel.refresh_requested = false;
        }
        if let Some(active) = &mut picker {
            active.poll();
        }
        let drawn = terminal
            .draw(|frame| {
                if let Some(active) = &picker {
                    scene.clear();
                    active.draw(frame, messages, ascii);
                } else {
                    crate::view::interactive(frame, &panel, messages, ascii, &mut scene);
                }
            })?
            .area;
        if event::poll(tick.saturating_sub(last_tick.elapsed()))? {
            let input = event::read()?;
            if handle_picker(&input, &mut picker, &mut panel) {
                continue;
            }
            match input {
                Event::Mouse(mouse) => {
                    let point = (mouse.column, mouse.row).into();
                    scene.pointer = Some(point);
                    match mouse.kind {
                        MouseEventKind::Down(MouseButton::Left)
                            if terminal.size()?
                                == ratatui::layout::Size::new(drawn.width, drawn.height) =>
                        {
                            if let Some(action) = scene.hit(point) {
                                action.apply(&mut panel, drawn);
                            }
                        }
                        MouseEventKind::ScrollDown if panel.form.is_none() => {
                            panel.scroll = panel.scroll.saturating_add(3).min(500);
                        }
                        MouseEventKind::ScrollUp if panel.form.is_none() => {
                            panel.scroll = panel.scroll.saturating_sub(3);
                        }
                        _ => {}
                    }
                }
                Event::Resize(_, _) => scene.pointer = None,
                other => panel.handle(&other, drawn.width, drawn.height),
            }
        }
        if last_tick.elapsed() >= tick {
            panel.tick = panel.tick.wrapping_add(1) % 4;
            last_tick = Instant::now();
        }
    }
    Ok(())
}

fn handle_picker(
    input: &Event,
    picker: &mut Option<crate::folder_picker::Picker>,
    panel: &mut Panel,
) -> bool {
    if let Some(active) = picker {
        if let Event::Key(key) = input
            && key.kind == event::KeyEventKind::Press
        {
            if key.code == event::KeyCode::Esc {
                *picker = None;
            } else if let Some(mut path) = active.key(key.code) {
                if let Some(form) = &mut panel.form {
                    if matches!(form.operation, crate::Operation::ExportLatest { .. }) {
                        path.push(std::path::MAIN_SEPARATOR);
                    }
                    if let Some(value) = form.values.get_mut(form.field) {
                        *value = path;
                        form.field += 1;
                    }
                }
                *picker = None;
            }
        }
        return true;
    }
    if let Event::Key(key) = input
        && key.code == event::KeyCode::F(2)
        && key.kind == event::KeyEventKind::Press
        && let Some(form) = &mut panel.form
        && matches!(
            form.operation,
            crate::Operation::BackupNow { .. } | crate::Operation::ExportLatest { .. }
        )
        && let Mode::Live(state) = &panel.mode
        && let Some(folder) = state
            .snapshot
            .as_ref()
            .and_then(|s| s.operations.backup_folder.clone())
    {
        form.field = form.values.len().saturating_sub(1);
        *picker = Some(crate::folder_picker::Picker::new(folder));
        return true;
    }
    false
}
