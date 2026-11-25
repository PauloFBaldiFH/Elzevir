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

/// Opens the installation overview without a worker, network access, or writable operations.
/// Returns the explicit action chosen by the operator, or None on exit.
///
/// # Errors
/// Reports terminal or localization failures.
pub fn inspect_installation(
    language: &str,
    ascii: bool,
    inspection: crate::Inspection,
) -> Result<Option<crate::InspectionAction>, PanelError> {
    let messages = prepare(language)?;
    let mut panel = Panel::live(Institution::default());
    panel.inspection = Some(inspection);
    with_terminal(|terminal| interact(terminal, &messages, ascii, &mut panel, None, || false))?;
    Ok(panel.inspection_action)
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
    mut panel: Panel,
    worker: Option<&Worker>,
    stopping: impl Fn() -> bool,
) -> Result<(), PanelError> {
    with_terminal(|terminal| interact(terminal, messages, ascii, &mut panel, worker, stopping))
}

pub(crate) fn with_terminal<T>(
    action: impl FnOnce(ratatui::DefaultTerminal) -> io::Result<T>,
) -> Result<T, PanelError> {
    let guard = TerminalGuard { active: true };
    let terminal = ratatui::try_init()?;
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        drop(execute!(
            io::stdout(),
            event::DisableBracketedPaste,
            DisableMouseCapture
        ));
        previous(info);
    }));
    execute!(
        io::stdout(),
        event::EnableBracketedPaste,
        EnableMouseCapture
    )?;
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
        let mouse = execute!(
            io::stdout(),
            event::DisableBracketedPaste,
            DisableMouseCapture
        );
        let terminal = ratatui::try_restore();
        self.active = false;
        mouse.and(terminal)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        // Emergency restoration also covers partial terminal initialization.
        if self.active {
            drop(execute!(
                io::stdout(),
                event::DisableBracketedPaste,
                DisableMouseCapture
            ));
            drop(ratatui::try_restore());
        }
    }
}

fn new_restore_review(
    previous: Option<&Snapshot>,
    current: &Snapshot,
) -> Option<crate::RestoreReview> {
    let review = current.operations.review.as_ref()?;
    if previous
        .and_then(|s| s.operations.review.as_ref())
        .is_some_and(|r| r.token == review.token)
    {
        return None;
    }
    Some(review.clone())
}

fn interact(
    mut terminal: ratatui::DefaultTerminal,
    messages: &Messages,
    ascii: bool,
    panel: &mut Panel,
    worker: Option<&Worker>,
    stopping: impl Fn() -> bool,
) -> io::Result<()> {
    let mut access = crate::access_actions::Actions::detect();
    panel.access_help = Some(access.help());
    let mut scene = crate::ui::Scene::default();
    let mut picker: Option<crate::folder_picker::Picker> = None;
    let mut observed_at: Option<Instant> = None;
    let mut action_sequence = None;
    let mut path_probe = crate::restore_paths::Probe::default();
    let tick = Duration::from_millis(150);
    let mut last_tick = Instant::now();
    while !panel.quit && !stopping() {
        if let Some(worker) = worker {
            if let Some((snapshot, at, completed)) = worker.take()
                && let Mode::Live(state) = &mut panel.mode
            {
                let review = new_restore_review(state.snapshot.as_ref(), &snapshot);
                state.snapshot = Some(snapshot);
                if let Some(review) = review
                    && panel.form.is_none()
                    && panel.section == crate::Section::Backup
                {
                    panel.form = Some(crate::Form::new(crate::Operation::ApplyRestore(review)));
                }
                panel.initialize_cloud_file();
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
        access.poll(panel);
        path_probe.poll(panel.form.as_mut());
        let drawn = terminal
            .draw(|frame| {
                if let Some(active) = &picker {
                    scene.clear();
                    active.draw(frame, messages, ascii);
                } else {
                    crate::view::interactive(frame, panel, messages, ascii, &mut scene);
                }
            })?
            .area;
        if event::poll(tick.saturating_sub(last_tick.elapsed()))? {
            let input = event::read()?;
            let size = terminal.size()?;
            if size != ratatui::layout::Size::new(drawn.width, drawn.height) {
                continue;
            }
            if picker.is_none() && panel.size_warning(size.width, size.height) {
                panel.handle(&input, size.width, size.height);
                continue;
            }
            if handle_picker(&input, &mut picker, panel) || access.handle(&input, panel) {
                continue;
            }
            match input {
                Event::Mouse(mouse) => handle_mouse(mouse, &mut scene, panel, drawn),
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

fn handle_mouse(
    mouse: event::MouseEvent,
    scene: &mut crate::ui::Scene,
    panel: &mut Panel,
    drawn: ratatui::layout::Rect,
) {
    let point = (mouse.column, mouse.row).into();
    scene.pointer = Some(point);
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if let Some(action) = scene.hit(point) {
                action.apply(panel, drawn);
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

fn handle_picker(
    input: &Event,
    picker: &mut Option<crate::folder_picker::Picker>,
    panel: &mut Panel,
) -> bool {
    if let Some(active) = picker {
        if let Event::Paste(path) = input {
            active.paste(path);
            return true;
        }
        if let Event::Key(key) = input
            && key.kind == event::KeyEventKind::Press
        {
            if key.code == event::KeyCode::F(6)
                && panel.form.as_ref().is_some_and(|f| {
                    matches!(f.operation, crate::Operation::Restore { .. }) && f.field == 0
                })
            {
                *picker = None;
                panel.form = None;
                panel.cloud_tab = 2;
                panel.scroll = 0;
            } else if key.code == event::KeyCode::Esc {
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
            crate::Operation::BackupNow { .. }
                | crate::Operation::ExportLatest { .. }
                | crate::Operation::Restore { .. }
        )
        && let Mode::Live(state) = &panel.mode
        && let Some(folder) = state
            .snapshot
            .as_ref()
            .and_then(|s| s.operations.backup_folder.clone())
    {
        if matches!(form.operation, crate::Operation::Restore { .. }) {
            if form.field < 2
                && let Some(s) = &state.snapshot
            {
                *picker = Some(crate::folder_picker::Picker::files(
                    folder,
                    form.field == 0,
                    &s.operations.managed_ids,
                ));
            }
            return true;
        }
        form.field = form.values.len().saturating_sub(1);
        *picker = Some(crate::folder_picker::Picker::new(folder));
        return true;
    }
    false
}
