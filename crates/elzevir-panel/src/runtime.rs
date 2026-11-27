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

fn observe(
    panel: &mut Panel,
    worker: &Worker,
    observed_at: &mut Option<Instant>,
    action_sequence: &mut Option<u64>,
) {
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
        *observed_at = Some(at);
        if action_sequence.is_some_and(|requested| completed >= requested) {
            panel.diagnostics.pending = false;
            panel.backups.waiting_ack = false;
            *action_sequence = None;
        }
    }
    if let Mode::Live(state) = &mut panel.mode {
        state.age_seconds = observed_at.map_or(0, |at| at.elapsed().as_secs());
    }
    if let Some(request) = panel.diagnostics.requested.take() {
        *action_sequence = worker.request(request);
        if action_sequence.is_none() {
            panel.diagnostics.pending = false;
            panel.backups.waiting_ack = false;
        }
    } else if panel.refresh_requested {
        worker.refresh();
    }
    panel.refresh_requested = false;
}

fn poll_explorers(
    panel: &Panel,
    picker: &mut Option<crate::folder_picker::Picker>,
    manual: &mut Option<crate::manual_backup::Manual>,
) {
    if let Some(active) = picker {
        active.poll();
    }
    if crate::manual_backup::Manual::active(panel) {
        if manual.is_none() {
            *manual = crate::manual_backup::Manual::start(panel);
        }
        if let Some(editor) = manual {
            editor.poll();
        }
    } else {
        *manual = None;
    }
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
    scene.desktop = access.desktop();
    let mut input_layer = None;
    let mut transfer: Option<crate::transfer::Dialog> = None;
    let mut picker: Option<crate::folder_picker::Picker> = None;
    let mut manual: Option<crate::manual_backup::Manual> = None;
    let mut observed_at: Option<Instant> = None;
    let mut action_sequence = None;
    let mut path_probe = crate::restore_paths::Probe::default();
    let tick = Duration::from_millis(150);
    let mut last_tick = Instant::now();
    while !panel.quit && !stopping() {
        if let Some(worker) = worker {
            observe(panel, worker, &mut observed_at, &mut action_sequence);
        }
        poll_explorers(panel, &mut picker, &mut manual);
        access.poll(panel);
        path_probe.poll(panel.form.as_mut());
        let layer = (
            terminal.size()?,
            panel.section,
            panel.form.as_ref().map(|f| f.operation.key()),
            panel.palette,
            transfer.is_some(),
            picker.is_some(),
            scene.gallery,
            panel.backups.page,
            manual.is_some(),
        );
        if input_layer != Some(layer) {
            scene.reset_input();
            input_layer = Some(layer);
        }
        let drawn = terminal
            .draw(|frame| {
                if let Some(editor) = &manual
                    && !crate::view::too_small(frame.area().width, frame.area().height)
                {
                    editor.draw(frame, panel, messages, &mut scene);
                } else if let Some(active) = &picker {
                    scene.clear();
                    active.draw(frame, messages, ascii);
                } else {
                    crate::view::interactive(frame, panel, messages, ascii, &mut scene);
                    if let Some(dialog) = &transfer
                        && !crate::view::too_small(frame.area().width, frame.area().height)
                    {
                        crate::ui::dim(frame);
                        dialog.draw(frame, messages, &mut scene, ascii, access.desktop());
                    }
                }
            })?
            .area;
        scene.reconcile();
        if event::poll(tick.saturating_sub(last_tick.elapsed()))? {
            let input = event::read()?;
            let size = terminal.size()?;
            if size != ratatui::layout::Size::new(drawn.width, drawn.height) {
                scene.reset_input();
                continue;
            }
            if picker.is_none() && panel.size_warning(size.width, size.height) {
                panel.handle(&input, size.width, size.height);
                continue;
            }
            if let Some(editor) = &mut manual {
                editor.input(&input, &mut scene, panel, &mut access);
                continue;
            }
            if handle_picker(&input, &mut picker, panel) {
                continue;
            }
            let Some(input) = control_input(
                input,
                &mut scene,
                panel,
                drawn,
                &mut access,
                transfer.is_some(),
            ) else {
                continue;
            };
            if !crate::transfer::handle(&mut transfer, &input, panel, &mut access)
                && !access.handle(&input, panel)
            {
                panel.handle(&input, drawn.width, drawn.height);
            }
            if panel.section == crate::Section::Dashboard && matches!(panel.mode, Mode::Live(_)) {
                panel.scroll = panel.scroll.min(scene.scroll_limit);
            }
        }
        if last_tick.elapsed() >= tick {
            panel.tick = panel.tick.wrapping_add(1) % 4;
            last_tick = Instant::now();
        }
    }
    Ok(())
}

fn control_input(
    input: Event,
    scene: &mut crate::ui::Scene,
    panel: &mut Panel,
    drawn: ratatui::layout::Rect,
    access: &mut crate::access_actions::Actions,
    transfer_open: bool,
) -> Option<Event> {
    let controls = (!transfer_open
        && panel.section == crate::Section::Backup
        && panel.backups.page != crate::BackupPage::Connections)
        || panel.section == crate::Section::Dashboard
        || panel.form.is_some()
        || scene.gallery
        || panel.palette;
    let input = match input {
        Event::Resize(_, _) | Event::FocusLost => {
            scene.reset_input();
            return None;
        }
        Event::Mouse(mouse) => {
            let modal = panel.form.is_some() || panel.palette || transfer_open || scene.gallery;
            let action = mouse_action(mouse, scene, panel, modal)?;
            if let crate::ui::Action::Key(code) = action {
                Event::Key(event::KeyEvent::new(code, event::KeyModifiers::NONE))
            } else {
                action.apply(panel, drawn, scene, access);
                return None;
            }
        }
        Event::Key(key) if controls && key.kind == event::KeyEventKind::Press => {
            use event::KeyCode::{BackTab, Char, Enter, Esc, Tab};
            match key.code {
                event::KeyCode::PageUp | event::KeyCode::PageDown
                    if crate::backup_center::scrollable(panel)
                        && panel.form.is_none()
                        && !panel.palette
                        && !scene.gallery =>
                {
                    crate::backup_center::scroll(
                        panel,
                        key.code == event::KeyCode::PageDown,
                        usize::from(drawn.height.saturating_sub(12).max(1)),
                        scene.scroll_limit,
                    );
                    return None;
                }
                Tab | BackTab => {
                    if let Some(crate::ui::Action::Field(i) | crate::ui::Action::Choose(i, _)) =
                        scene.focus_next(key.code == Tab)
                        && let Some(form) = &mut panel.form
                    {
                        form.field = i;
                    }
                    return None;
                }
                Enter | Char(' ')
                    if scene.focus().is_some()
                        && !matches!(scene.focus(), Some(crate::ui::Action::Field(_))) =>
                {
                    if let Some(action) = scene.focus() {
                        if let crate::ui::Action::Key(code) = action {
                            return Some(Event::Key(event::KeyEvent::new(
                                code,
                                event::KeyModifiers::NONE,
                            )));
                        }
                        action.apply(panel, drawn, scene, access);
                    }
                    return None;
                }
                Esc if scene.gallery => {
                    scene.gallery = false;
                    return None;
                }
                _ if scene.gallery => return None,
                _ => Event::Key(key),
            }
        }
        Event::Key(key) if controls && key.kind != event::KeyEventKind::Press => return None,
        other => other,
    };
    Some(input)
}

fn mouse_action(
    mouse: event::MouseEvent,
    scene: &mut crate::ui::Scene,
    panel: &mut Panel,
    modal: bool,
) -> Option<crate::ui::Action> {
    let point = (mouse.column, mouse.row).into();
    scene.pointer = Some(point);
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => scene.press(point),
        MouseEventKind::Up(MouseButton::Left) => return scene.release(point),
        MouseEventKind::ScrollDown | MouseEventKind::ScrollUp if !modal => {
            scene.reset_input();
            if crate::backup_center::scrollable(panel) {
                crate::backup_center::scroll(
                    panel,
                    mouse.kind == MouseEventKind::ScrollDown,
                    3,
                    scene.scroll_limit,
                );
                return None;
            }
            let limit = if panel.section == crate::Section::Dashboard {
                scene.scroll_limit
            } else {
                500
            };
            panel.scroll = if mouse.kind == MouseEventKind::ScrollDown {
                panel.scroll.saturating_add(3).min(limit)
            } else {
                panel.scroll.saturating_sub(3)
            };
        }
        _ => {}
    }
    None
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

#[cfg(test)]
mod interaction_tests {
    use super::{control_input, mouse_action};
    use crate::{
        Messages, Mode, Panel, Section,
        access_actions::Actions,
        ui::{Action, Scene},
    };
    use ratatui::{
        Terminal,
        backend::TestBackend,
        crossterm::event::{
            Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
        },
        layout::Rect,
    };

    fn mouse(kind: MouseEventKind, x: u16, y: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        }
    }
    #[test]
    fn gallery_skips_busy_and_disabled_for_mouse_and_keyboard() {
        let mut panel = Panel::default();
        let mut scene = Scene::default();
        let messages = Messages::new("en").unwrap();
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        scene.gallery = true;
        terminal
            .draw(|f| crate::view::interactive(f, &panel, &messages, false, &mut scene))
            .unwrap();
        let screen = terminal.backend().buffer();
        for needle in ["Unavailable", "In progress"] {
            let y = (0..30)
                .find(|y| {
                    (0..120)
                        .map(|x| screen[(x, *y)].symbol())
                        .collect::<String>()
                        .contains(needle)
                })
                .unwrap();
            let line = (0..120)
                .map(|x| screen[(x, y)].symbol())
                .collect::<String>();
            let x = u16::try_from(line.find(needle).unwrap()).unwrap();
            mouse_action(
                mouse(MouseEventKind::Down(MouseButton::Left), x, y),
                &mut scene,
                &mut panel,
                true,
            );
            assert_eq!(
                mouse_action(
                    mouse(MouseEventKind::Up(MouseButton::Left), x, y),
                    &mut scene,
                    &mut panel,
                    true
                ),
                None
            );
        }
        let mut actions = Actions::default();
        let bounds = Rect::new(0, 0, 120, 30);
        for code in [KeyCode::Tab, KeyCode::Char(' ')] {
            control_input(
                Event::Key(KeyEvent::new(code, KeyModifiers::NONE)),
                &mut scene,
                &mut panel,
                bounds,
                &mut actions,
                false,
            );
        }
        assert_eq!(scene.sample, Some(true));
        control_input(
            Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            &mut scene,
            &mut panel,
            bounds,
            &mut actions,
            false,
        );
        assert!(!scene.gallery && matches!(panel.mode, Mode::Demo));
    }

    #[test]
    fn a_resize_or_a_disappearing_target_cancels_mouse_capture() {
        let mut panel = Panel::default();
        let mut scene = Scene::default();
        let rect = Rect::new(1, 2, 16, 3);
        let action = Action::Section(Section::Settings);
        scene.add(rect, action);
        let mut access = Actions::default();
        scene.press((1, 2).into());
        control_input(
            Event::Resize(80, 24),
            &mut scene,
            &mut panel,
            rect,
            &mut access,
            false,
        );
        assert_eq!(scene.release((1, 2).into()), None);
        scene.press((16, 4).into());
        scene.clear();
        scene.reconcile();
        scene.add(rect, action);
        assert_eq!(scene.release((16, 4).into()), None);
    }
}
