// SPDX-License-Identifier: GPL-3.0-or-later
//! Service section: the server's state in large type and big clickable
//! buttons for what can be done with it now.

use crate::ui::{Action, Scene, label, theme};
use crate::{LiveState, Messages, Panel, ServerStatus};
use ratatui::{
    Frame,
    crossterm::event::KeyCode,
    layout::{Constraint, Layout, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};

/// One big button: its label, the key it presses and whether it applies now.
struct Button {
    label: &'static str,
    key: char,
    enabled: bool,
}

pub(crate) fn draw(
    frame: &mut Frame,
    panel: &Panel,
    state: &LiveState,
    messages: &Messages,
    work: Rect,
    (ascii, scene): (bool, &mut Scene),
) {
    let inner = work.inner(Margin::new(2, 1));
    if inner.height < 12 {
        return;
    }
    let snapshot = state.snapshot.as_ref().filter(|_| state.age_seconds <= 5);
    let [status, _, first, second, _, result] = Layout::vertical([
        Constraint::Length(4),
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Min(1),
    ])
    .areas(inner);
    frame.render_widget(
        Paragraph::new(status_lines(snapshot, messages, ascii)).wrap(Wrap { trim: true }),
        status,
    );
    let online = matches!(
        snapshot.map(|sample| &sample.status),
        Some(ServerStatus::Running(_) | ServerStatus::Degraded(_))
    );
    let stopped = matches!(
        snapshot.map(|sample| &sample.status),
        Some(ServerStatus::Stopped)
    );
    let idle = !panel.diagnostics.pending;
    let server = if panel.managed() {
        vec![Button {
            label: "ui-restart-service",
            key: 't',
            enabled: idle,
        }]
    } else {
        vec![
            Button {
                label: "ui-start",
                key: 'i',
                enabled: idle && stopped,
            },
            Button {
                label: "ui-stop",
                key: 'p',
                enabled: idle && online,
            },
            Button {
                label: "ui-restart",
                key: 't',
                enabled: idle && online,
            },
        ]
    };
    buttons(frame, first, &server, messages, (ascii, &mut *scene));
    let machine = [
        Button {
            label: "ui-settings",
            key: 'c',
            enabled: idle && state.institution.configured_listen.is_some(),
        },
        Button {
            label: "ui-reboot",
            key: 'm',
            enabled: idle,
        },
    ];
    buttons(frame, second, &machine, messages, (ascii, scene));
    let outcome = if panel.diagnostics.pending {
        Some(("op-pending", theme::ACCENT))
    } else {
        snapshot
            .and_then(|sample| sample.operations.result)
            .map(|key| {
                let colour = if matches!(key, "op-server-done" | "settings-saved") {
                    theme::SUCCESS
                } else {
                    theme::WARNING
                };
                (key, colour)
            })
    };
    if let Some((key, colour)) = outcome {
        frame.render_widget(
            Paragraph::new(messages.get(key))
                .style(Style::new().fg(colour).add_modifier(Modifier::BOLD))
                .wrap(Wrap { trim: true }),
            result,
        );
    }
}

fn status_lines(
    snapshot: Option<&crate::Snapshot>,
    messages: &Messages,
    ascii: bool,
) -> Vec<Line<'static>> {
    let (key, colour, details) = match snapshot.map(|sample| &sample.status) {
        Some(ServerStatus::Running(details)) => {
            ("panel-state-running", theme::SUCCESS, Some(details))
        }
        Some(ServerStatus::Degraded(details)) => {
            ("panel-state-degraded", theme::WARNING, Some(details))
        }
        Some(ServerStatus::Stopped) => ("panel-state-stopped", theme::MUTED, None),
        Some(ServerStatus::Unavailable) => ("panel-state-unavailable", theme::WARNING, None),
        None => ("panel-state-loading", theme::MUTED, None),
    };
    let dot = if ascii { "*" } else { "●" };
    let mut lines = vec![Line::from(vec![
        Span::styled(format!("{dot} "), Style::new().fg(colour)),
        Span::styled(
            messages.get(key),
            Style::new().fg(colour).add_modifier(Modifier::BOLD),
        ),
    ])];
    if let Some(details) = details {
        let mut facts = vec![messages.values(
            "service-uptime",
            &[("value", uptime(details.uptime_seconds))],
        )];
        if let Some(address) = snapshot
            .and_then(|sample| sample.addresses.catalog.clone())
            .or_else(|| details.listen.map(|listen| format!("http://{listen}/")))
        {
            facts.push(crate::dashboard::text(&address));
        }
        lines.push(Line::from(facts.join(if ascii {
            "  |  "
        } else {
            "  ·  "
        })));
        if let Some(sessions) = snapshot.and_then(|sample| sample.operations.sessions) {
            lines.push(Line::from(
                messages.values("service-sessions", &[("value", sessions.to_string())]),
            ));
        }
    } else if key == "panel-state-stopped" {
        lines.push(
            Line::from(messages.get("service-stopped-hint")).style(Style::new().fg(theme::MUTED)),
        );
    }
    lines
}

fn buttons(
    frame: &mut Frame,
    area: Rect,
    buttons: &[Button],
    messages: &Messages,
    (ascii, scene): (bool, &mut Scene),
) {
    let count = u32::try_from(buttons.len()).unwrap_or(1).max(1);
    let areas = Layout::horizontal((0..count).map(|_| Constraint::Ratio(1, count)))
        .spacing(2)
        .split(area);
    for (button, rect) in buttons.iter().zip(areas.iter().copied()) {
        let text = label(messages.get(button.label), ascii);
        let hover = button.enabled && scene.add(rect, Action::Key(KeyCode::Char(button.key)));
        let (border, inside) = if !button.enabled {
            (
                theme::BORDER,
                Style::new().fg(theme::MUTED).bg(theme::SURFACE),
            )
        } else if hover {
            (theme::ACCENT, theme::selected())
        } else {
            (
                theme::ACCENT,
                Style::new()
                    .fg(theme::TEXT)
                    .bg(theme::SURFACE)
                    .add_modifier(Modifier::BOLD),
            )
        };
        // A framed block, so the whole of it is one big target.
        frame.render_widget(
            Paragraph::new(Line::from(text))
                .centered()
                .style(inside)
                .block(
                    crate::view::block(String::new(), ascii)
                        .style(inside)
                        .border_style(Style::new().fg(border)),
                ),
            rect,
        );
    }
}

/// Uptime in the largest two units, e.g. `2 h 5 min`.
fn uptime(seconds: u64) -> String {
    let (days, hours, minutes) = (seconds / 86_400, seconds / 3_600 % 24, seconds / 60 % 60);
    match (days, hours) {
        (0, 0) => format!("{minutes} min"),
        (0, _) => format!("{hours} h {minutes} min"),
        _ => format!("{days} d {hours} h"),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn uptime_uses_the_two_largest_units() {
        assert_eq!(super::uptime(59), "0 min");
        assert_eq!(super::uptime(7_500), "2 h 5 min");
        assert_eq!(super::uptime(3 * 86_400 + 4 * 3_600 + 60), "3 d 4 h");
    }
}
