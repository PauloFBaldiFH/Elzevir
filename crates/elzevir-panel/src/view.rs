// SPDX-License-Identifier: GPL-3.0-or-later
//! Pure rendering; terminal I/O is confined to the runtime.

use crate::{
    Messages, Mode, Panel, Section,
    ui::{self, Action, Scene, theme},
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Margin, Rect},
    style::Style,
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
        Constraint::Length(if area.width < 110 { 22 } else { 28 }),
        Constraint::Min(1),
        Constraint::Length(if area.width < 83 { 0 } else { 30 }),
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
            area.height.min(9),
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
        border::ROUNDED
    };
    Block::default()
        .borders(Borders::ALL)
        .border_set(border)
        .style(theme::surface())
        .border_style(Style::new().fg(theme::BORDER))
        .title(title)
}

fn menu(
    frame: &mut Frame,
    panel: &Panel,
    messages: &Messages,
    area: Rect,
    ascii: bool,
    scene: &mut Scene,
) {
    let inner = area.inner(Margin::new(1, 1));
    let items = Section::ALL
        .into_iter()
        .enumerate()
        .map(|(index, section)| {
            let visible = u16::try_from(index)
                .ok()
                .filter(|index| *index < inner.height);
            let hover = visible.is_some_and(|index| {
                scene.add(
                    Rect::new(inner.x, inner.y + index, inner.width, 1),
                    Action::Section(section),
                )
            });
            let style = if hover {
                theme::selected()
            } else if index % 2 == 0 {
                theme::surface()
            } else {
                Style::new().fg(theme::TEXT).bg(theme::BACKGROUND)
            };
            ListItem::new(format!(
                "{} {}",
                section.shortcut(),
                messages.get(section.key())
            ))
            .style(style)
        });
    let mut selection = ListState::default().with_selected(
        Section::ALL
            .iter()
            .position(|section| *section == panel.section),
    );
    let list = List::new(items)
        .block(block(messages.get("panel-menu"), ascii))
        .highlight_symbol(if ascii { "> " } else { "▌ " })
        .highlight_style(theme::selected());
    frame.render_stateful_widget(list, area, &mut selection);
}

/// Draws a frame without reading external state or executing operations.
pub fn draw(frame: &mut Frame, panel: &Panel, messages: &Messages, ascii: bool) {
    interactive(frame, panel, messages, ascii, &mut Scene::default());
}

pub(crate) fn too_small(width: u16, height: u16) -> bool {
    width < 80 || height < 18
}

pub(crate) fn interactive(
    frame: &mut Frame,
    panel: &Panel,
    messages: &Messages,
    ascii: bool,
    scene: &mut Scene,
) {
    scene.clear();
    let area = frame.area();
    ui::fill(frame, area, theme::muted());
    if let Mode::Live(state) = &panel.mode
        && panel.sharing_catalog()
    {
        crate::sharing::draw(
            frame,
            state.snapshot.as_ref().filter(|_| state.age_seconds <= 5),
            messages,
            ascii || area.width < 80,
            panel.access_notice,
            scene,
        );
        return;
    }
    if too_small(area.width, area.height) {
        frame.render_widget(
            Paragraph::new(messages.get(if panel.form.is_some() {
                "panel-small-form"
            } else {
                "panel-small"
            }))
            .wrap(Wrap { trim: true }),
            area,
        );
        return;
    }
    if scene.gallery && matches!(panel.mode, Mode::Demo) {
        crate::overview::gallery(frame, messages, scene);
        return;
    }
    if panel.section == Section::Dashboard && matches!(panel.mode, Mode::Live(_)) {
        crate::overview::draw(frame, panel, messages, ascii, scene);
    } else if panel.section == Section::Addresses
        && panel.inspection.is_none()
        && matches!(panel.mode, Mode::Live(_))
    {
        crate::access_center::draw(frame, panel, messages, scene, ascii);
    } else if panel.section == Section::Backup
        && panel.inspection.is_none()
        && matches!(panel.mode, Mode::Live(_))
    {
        crate::backup_center::draw(frame, panel, messages, scene);
    } else {
        section_body(frame, panel, messages, ascii, scene);
    }
    if panel.form.is_some() {
        scene.clear();
        ui::dim(frame);
        crate::operation_view::form(frame, panel, messages, ascii, scene);
    } else if panel.palette {
        scene.clear();
        ui::dim(frame);
        let popup = menu_area(area, true);
        frame.render_widget(Clear, popup);
        menu(frame, panel, messages, popup, ascii, scene);
    }
}

fn section_body(
    frame: &mut Frame,
    panel: &Panel,
    messages: &Messages,
    ascii: bool,
    scene: &mut Scene,
) {
    let area = frame.area();
    let header = Rect::new(area.x, area.y, area.width, 1);
    ui::header(frame, header, panel, messages, ascii);
    let [mut navigation, mut work, mut context] = columns(area);
    if matches!(panel.mode, Mode::Live(_))
        && (area.width < 120 || panel.section == Section::Dashboard || panel.inspection.is_some())
    {
        navigation = Rect::default();
        work = body(area);
        context = Rect::new(work.right(), work.y, 0, work.height);
    }
    let tabs = Rect::new(work.x, work.y, work.width + context.width, 1);
    draw_tabs(frame, tabs, panel, messages, scene);
    let action_rows = if matches!(&panel.mode, Mode::Live(_)) && area.width < 115 {
        2
    } else {
        1
    };
    let work = Rect::new(
        work.x,
        work.y + 1,
        work.width,
        work.height.saturating_sub(1 + action_rows),
    );
    let context = Rect::new(context.x, work.y, context.width, work.height);
    let actions = Rect::new(
        work.x,
        work.bottom(),
        work.width + context.width,
        action_rows,
    );
    if !navigation.is_empty() {
        menu(frame, panel, messages, navigation, ascii, scene);
    }
    if let Some(inspection) = &panel.inspection {
        inspection.draw(
            frame,
            work.union(context),
            panel.section,
            messages,
            scene,
            ascii,
        );
    } else if let Mode::Live(state) = &panel.mode {
        crate::dashboard::draw(frame, panel, state, messages, work, context, ascii);
        if panel.section == Section::Restart {
            crate::service_view::draw(frame, panel, state, messages, work, (ascii, scene));
        }
    } else {
        draw_demo(frame, panel, messages, ascii, work, context);
    }
    ui::action_bar(frame, actions, panel, messages, scene, ascii);
    draw_footer(frame, area, panel, messages);
    if matches!(panel.mode, Mode::Demo) && panel.section == Section::Dashboard {
        ui::Button::new(
            messages.get("desk-gallery"),
            Action::Gallery,
            ui::Variant::Primary,
        )
        .draw(
            frame,
            Rect::new(
                work.x + 2,
                work.bottom().saturating_sub(4),
                work.width.saturating_sub(4),
                3,
            ),
            scene,
        );
    }
}

pub(crate) fn draw_tabs(
    frame: &mut Frame,
    tabs: Rect,
    panel: &Panel,
    messages: &Messages,
    scene: &mut Scene,
) {
    scene.buttons(
        frame,
        tabs,
        Section::ALL
            .into_iter()
            .enumerate()
            .map(|(index, section)| {
                (
                    {
                        let names = [
                            "tab-overview",
                            "tab-health",
                            "tab-access",
                            "tab-backup",
                            "tab-updates",
                            "tab-restart",
                            "tab-settings",
                        ];
                        let label = names
                            .get(index)
                            .map_or_else(String::new, |name| messages.get(name));
                        if tabs.width < 100 {
                            label
                        } else {
                            format!("{} {label}", index + 1)
                        }
                    },
                    Action::Section(section),
                    panel.section == section,
                )
            }),
    );
}

fn draw_demo(
    frame: &mut Frame,
    panel: &Panel,
    messages: &Messages,
    ascii: bool,
    work: Rect,
    context: Rect,
) {
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

fn draw_footer(frame: &mut Frame, area: Rect, panel: &Panel, messages: &Messages) {
    let footer = Rect::new(area.x, area.bottom().saturating_sub(2), area.width, 2);
    let mut help = messages.get(if panel.inspection.is_some() {
        "inspect-help"
    } else if matches!(panel.mode, Mode::Demo) {
        "panel-help"
    } else {
        "panel-help-live"
    });
    if let Mode::Live(state) = &panel.mode
        && let Some(result) = state
            .snapshot
            .as_ref()
            .and_then(|sample| sample.operations.result)
    {
        help = format!("{}  |  {help}", messages.get(result));
    }
    frame.render_widget(Paragraph::new(help).wrap(Wrap { trim: true }), footer);
}
