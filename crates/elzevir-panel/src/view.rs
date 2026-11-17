// SPDX-License-Identifier: GPL-3.0-or-later
//! Pure rendering; terminal I/O is confined to the runtime.

use crate::{Messages, Mode, Panel, Section};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    symbols::border,
    text::Line,
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

fn body(area: Rect) -> Rect {
    Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(2),
    ])
    .split(area)
    .get(1)
    .copied()
    .unwrap_or_default()
}

fn columns(area: Rect) -> [Rect; 3] {
    let areas = Layout::horizontal([
        Constraint::Length(28),
        Constraint::Min(1),
        Constraint::Length(30),
    ])
    .split(body(area));
    std::array::from_fn(|i| areas.get(i).copied().unwrap_or_default())
}

pub(crate) fn menu_area(area: Rect, palette: bool) -> Rect {
    if palette {
        Rect::new(
            area.x + area.width.saturating_sub(48) / 2,
            area.y + area.height.saturating_sub(8) / 2,
            area.width.min(48),
            area.height.min(8),
        )
    } else {
        columns(area).first().copied().unwrap_or_default()
    }
}

pub(crate) fn block(title: String, ascii: bool) -> Block<'static> {
    let border = if ascii {
        border::Set {
            top_left: "+",
            top_right: "+",
            bottom_left: "+",
            bottom_right: "+",
            vertical_left: "|",
            vertical_right: "|",
            horizontal_top: "-",
            horizontal_bottom: "-",
        }
    } else {
        border::PLAIN
    };
    Block::default()
        .borders(Borders::ALL)
        .border_set(border)
        .title(title)
}

fn menu(frame: &mut Frame, panel: &Panel, messages: &Messages, area: Rect, ascii: bool) {
    let items = Section::ALL.into_iter().map(|section| {
        ListItem::new(format!(
            "{} {}",
            section.shortcut(),
            messages.get(section.key())
        ))
    });
    let mut selection = ListState::default().with_selected(
        Section::ALL
            .iter()
            .position(|section| *section == panel.section),
    );
    let list = List::new(items)
        .block(block(messages.get("panel-menu"), ascii))
        .highlight_symbol("> ")
        .highlight_style(Style::default().fg(Color::Black).bg(Color::Cyan));
    frame.render_stateful_widget(list, area, &mut selection);
}

/// Draws a frame without reading external state or executing operations.
pub fn draw(frame: &mut Frame, panel: &Panel, messages: &Messages, ascii: bool) {
    let area = frame.area();
    if area.width < 80 || area.height < 18 {
        frame.render_widget(
            Paragraph::new(messages.get("panel-small")).wrap(Wrap { trim: true }),
            area,
        );
        return;
    }
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(2),
    ])
    .split(area);
    frame.render_widget(
        Paragraph::new(messages.get(if matches!(&panel.mode, Mode::Demo) {
            "panel-title"
        } else {
            "panel-title-live"
        }))
        .style(Style::default().fg(Color::Cyan)),
        rows.first().copied().unwrap_or_default(),
    );
    let [navigation, work, context] = columns(area);
    menu(frame, panel, messages, navigation, ascii);
    if let Mode::Live(state) = &panel.mode {
        crate::dashboard::draw(frame, panel, state, messages, work, context, ascii);
    } else {
        let description = vec![
            Line::from(messages.get("panel-demo")),
            Line::default(),
            Line::from(messages.get(&format!("{}-body", panel.section.key()))),
            Line::default(),
            Line::from(messages.get("panel-no-actions")),
        ];
        frame.render_widget(
            Paragraph::new(description)
                .wrap(Wrap { trim: true })
                .block(block(messages.get(panel.section.key()), ascii)),
            work,
        );
        let spinner = ["|", "/", "-", "\\"]
            .get(panel.tick % 4)
            .copied()
            .unwrap_or("|");
        let context_text = vec![
            Line::from(messages.get("panel-institution")),
            Line::default(),
            Line::from(messages.get("panel-simulated")),
            Line::default(),
            Line::from(messages.get("panel-unread")),
            Line::default(),
            Line::from(format!("{spinner} {}", messages.get("panel-animation"))),
        ];
        frame.render_widget(
            Paragraph::new(context_text)
                .wrap(Wrap { trim: true })
                .block(block(messages.get("panel-context"), ascii)),
            context,
        );
    }
    frame.render_widget(
        Paragraph::new(messages.get(if matches!(&panel.mode, Mode::Demo) {
            "panel-help"
        } else {
            "panel-help-live"
        }))
        .wrap(Wrap { trim: true }),
        rows.last().copied().unwrap_or_default(),
    );
    crate::operation_view::form(frame, panel, messages, ascii);
    if panel.palette {
        let popup = menu_area(area, true);
        frame.render_widget(Clear, popup);
        menu(frame, panel, messages, popup, ascii);
    }
}
