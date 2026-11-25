// SPDX-License-Identifier: GPL-3.0-or-later
//! Interactive geometry recorded from visible widgets.

pub(crate) mod theme;

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    widgets::{Paragraph, Widget},
};

use crate::{Messages, Mode, Panel, Section, ServerStatus};
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Copy)]
pub(crate) enum Action {
    Section(Section),
    Key(KeyCode),
    /// Focus a form field.
    Field(usize),
    /// Pick one of a form field's fixed choices.
    Choose(usize, &'static str),
    /// Submit the form, as its primary button.
    Submit,
}

#[derive(Default)]
pub(crate) struct Scene {
    targets: Vec<(Rect, Action)>,
    pub(crate) pointer: Option<Position>,
}

impl Scene {
    pub(crate) fn clear(&mut self) {
        self.targets.clear();
    }

    pub(crate) fn hit(&self, point: Position) -> Option<Action> {
        self.targets
            .iter()
            .rev()
            .find(|(rect, _)| rect.contains(point))
            .map(|(_, action)| *action)
    }

    pub(crate) fn add(&mut self, area: Rect, action: Action) -> bool {
        if area.is_empty() {
            return false;
        }
        self.targets.push((area, action));
        self.pointer.is_some_and(|point| area.contains(point))
    }

    pub(crate) fn buttons(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        buttons: impl IntoIterator<Item = (String, Action, bool)>,
    ) {
        let buttons: Vec<_> = buttons.into_iter().collect();
        let count = buttons.len();
        if count == 0 || area.is_empty() {
            return;
        }
        let areas = Layout::horizontal(
            (0..count).map(|_| Constraint::Ratio(1, u32::try_from(count).unwrap_or(u32::MAX))),
        )
        .spacing(1)
        .split(area);
        for ((label, action, selected), rect) in buttons.into_iter().zip(areas.iter().copied()) {
            let hover = self.add(rect, action);
            let style = if selected || hover {
                theme::selected()
            } else {
                theme::surface().fg(theme::ACCENT)
            };
            frame.render_widget(Paragraph::new(label).style(style).centered(), rect);
        }
    }
}

impl Action {
    pub(crate) fn apply(self, panel: &mut Panel, area: Rect) {
        match self {
            Self::Section(section) => panel.select(section),
            Self::Key(key) => panel.handle(
                &Event::Key(KeyEvent::new(key, KeyModifiers::NONE)),
                area.width,
                area.height,
            ),
            Self::Field(index) => {
                if let Some(form) = &mut panel.form {
                    form.field = index.min(form.labels.len());
                }
            }
            Self::Choose(index, value) => {
                if let Some(form) = &mut panel.form
                    && let Some(field) = form.values.get_mut(index)
                {
                    value.clone_into(field);
                    form.field = index;
                }
            }
            Self::Submit => panel.submit_form(),
        }
    }
}

pub(crate) fn header(
    frame: &mut Frame,
    area: Rect,
    panel: &Panel,
    messages: &Messages,
    ascii: bool,
) {
    let snapshot = match &panel.mode {
        Mode::Live(state) => state.snapshot.as_ref().filter(|_| state.age_seconds <= 5),
        Mode::Demo => None,
    };
    let (status, colour, details) = match snapshot.map(|sample| &sample.status) {
        Some(ServerStatus::Running(details)) => ("ui-online", theme::SUCCESS, Some(details)),
        Some(ServerStatus::Degraded(details)) => {
            ("panel-state-degraded", theme::WARNING, Some(details))
        }
        Some(ServerStatus::Stopped) => ("panel-state-stopped", theme::MUTED, None),
        _ if matches!(panel.mode, Mode::Demo) => ("panel-simulated", theme::WARNING, None),
        _ => ("panel-unknown", theme::MUTED, None),
    };
    let version = details.map_or(env!("CARGO_PKG_VERSION"), |d| d.version.as_str());
    let mut parts = vec![
        format!("{}Elzevir TUI  {version}", if ascii { "" } else { "🦀 " }),
        label(messages.get(status), ascii),
    ];
    if let Some(details) = details {
        parts.push(format!(
            "SQLite {}",
            crate::dashboard::component(details.database, messages)
        ));
        parts.push(format!(
            "Tantivy {}",
            crate::dashboard::component(details.indexer, messages)
        ));
        if area.width >= 120
            && let Some(address) = snapshot
                .and_then(|sample| sample.addresses.catalog.as_deref())
                .filter(|a| a.len() + 65 < usize::from(area.width))
        {
            parts.push(address.to_owned());
        }
    }
    if snapshot.is_some_and(|sample| sample.operations.update_available) {
        parts.push(messages.get("ui-update-available"));
    }
    let title = crate::dashboard::text(&parts.join(if ascii { "  |  " } else { "  ·  " }));
    frame.render_widget(
        Paragraph::new(title).style(theme::surface().fg(colour).add_modifier(Modifier::BOLD)),
        area,
    );
}

pub(crate) fn action_bar(
    frame: &mut Frame,
    area: Rect,
    panel: &Panel,
    messages: &Messages,
    scene: &mut Scene,
    ascii: bool,
) {
    if !matches!(panel.mode, Mode::Live(_)) {
        return;
    }
    let contextual: &[(&str, char)] = match panel.section {
        Section::Settings if panel.settings().is_some() => &[("ui-settings", 'c')],
        Section::Dashboard => &[("panel-backup", 'b'), ("section-settings", 'g')],
        Section::Diagnostics => &[("ui-verify", 'v'), ("ui-export", 'e')],
        Section::Addresses => &[("ui-test", 't'), ("ui-share", 'Q'), ("ui-config", 'm')],
        Section::Backup => &[("ui-new", 'n'), ("ui-verify", 'v'), ("ui-config", 's')],
        Section::Updates => &[("ui-search", 'c'), ("ui-install", 'i'), ("ui-recover", 'r')],
        // The service section has its own big buttons.
        Section::Settings | Section::Restart => &[],
    };
    let controls: Vec<_> = contextual
        .iter()
        .copied()
        .chain([("ui-refresh", 'f')])
        .collect();
    if controls.is_empty() {
        return;
    }
    let per_row = if area.height > 1 {
        controls.len().div_ceil(2)
    } else {
        controls.len()
    };
    for (line, row) in controls.chunks(per_row).enumerate() {
        let buttons = row
            .iter()
            .copied()
            .enumerate()
            .map(|(index, (key, shortcut))| {
                (
                    format!(
                        "{} [{}]",
                        label(messages.get(key), ascii),
                        if panel.section == Section::Addresses {
                            shortcut
                        } else {
                            shortcut.to_ascii_uppercase()
                        }
                    ),
                    Action::Key(KeyCode::Char(shortcut)),
                    line == 0 && index == 0,
                )
            });
        scene.buttons(
            frame,
            Rect::new(
                area.x,
                area.y + u16::try_from(line).unwrap_or_default(),
                area.width,
                1,
            ),
            buttons,
        );
    }
}

pub(crate) fn label(value: String, ascii: bool) -> String {
    if ascii {
        value
            .trim_start_matches(|c: char| !c.is_alphabetic())
            .to_owned()
    } else {
        value
    }
}

pub(crate) fn modal_buttons(
    frame: &mut Frame,
    popup: Rect,
    messages: &Messages,
    ascii: bool,
    scene: &mut Scene,
    primary: Option<(&str, KeyCode)>,
) {
    let buttons = Rect::new(
        popup.x + 1,
        popup.bottom().saturating_sub(2),
        popup.width.saturating_sub(2),
        1,
    );
    scene.buttons(
        frame,
        buttons,
        primary
            .into_iter()
            .map(|(key, code)| (label(messages.get(key), ascii), Action::Key(code), true))
            .chain([(messages.get("ui-cancel"), Action::Key(KeyCode::Esc), false)]),
    );
}

pub(crate) fn dim(frame: &mut Frame) {
    let area = frame.area();
    frame
        .buffer_mut()
        .set_style(area, Style::new().add_modifier(Modifier::DIM));
}

pub(crate) fn fill(frame: &mut Frame, area: Rect, style: Style) {
    Paragraph::new("")
        .style(style)
        .render(area, frame.buffer_mut());
}

/// A dashboard card with a persistent title and scrollable, wrapped contents.
pub(crate) fn card(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    rows: Vec<ratatui::text::Line<'static>>,
    accent: Color,
    ascii: bool,
    offset: u16,
) {
    let block = crate::view::block(format!(" {title} "), ascii)
        .title_style(Style::new().fg(accent).add_modifier(Modifier::BOLD))
        .border_style(Style::new().fg(theme::BORDER))
        .padding(ratatui::widgets::Padding::horizontal(1));
    frame.render_widget(
        Paragraph::new(rows)
            .block(block)
            .wrap(ratatui::widgets::Wrap { trim: false })
            .scroll((offset, 0)),
        area,
    );
}
