// SPDX-License-Identifier: GPL-3.0-or-later
//! Interactive terminal boundary. The demo never opens institution data.

use crate::{
    Institution, Messages, Mode, ObservationRequest, Panel, PanelError, Snapshot, draw,
    worker::Worker,
};
use ratatui::crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture},
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
    terminal(&messages, ascii, Panel::default(), None)
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
    let messages = prepare(language)?;
    let worker = Worker::start(source)?;
    terminal(&messages, ascii, Panel::live(institution), Some(&worker))
}

fn prepare(language: &str) -> Result<Messages, PanelError> {
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
) -> Result<(), PanelError> {
    // Also restore partial initialization if entering the alternate screen fails.
    let result = interact(messages, ascii, panel, worker);
    let mouse = execute!(io::stdout(), DisableMouseCapture);
    let terminal = ratatui::try_restore();
    result?;
    mouse?;
    terminal?;
    Ok(())
}

fn interact(
    messages: &Messages,
    ascii: bool,
    mut panel: Panel,
    worker: Option<&Worker>,
) -> io::Result<()> {
    let mut terminal = ratatui::try_init()?;
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Best effort during panic; ratatui's previous hook restores raw mode and screen.
        drop(execute!(io::stdout(), DisableMouseCapture));
        previous(info);
    }));
    execute!(io::stdout(), EnableMouseCapture)?;
    let mut observed_at: Option<Instant> = None;
    let mut action_sequence = None;
    let tick = Duration::from_millis(150);
    let mut last_tick = Instant::now();
    while !panel.quit {
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
        terminal.draw(|frame| draw(frame, &panel, messages, ascii))?;
        if event::poll(tick.saturating_sub(last_tick.elapsed()))? {
            let size = terminal.size()?;
            panel.handle(&event::read()?, size.width, size.height);
        }
        if last_tick.elapsed() >= tick {
            panel.tick = panel.tick.wrapping_add(1) % 4;
            last_tick = Instant::now();
        }
    }
    Ok(())
}
