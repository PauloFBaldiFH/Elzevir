// SPDX-License-Identifier: GPL-3.0-or-later
//! Filled terminal button. Drawing and hit testing share one rectangle.
use super::{Action, Scene, fill, theme};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    widgets::Paragraph,
};

#[derive(Clone, Copy)]
pub(crate) enum Variant {
    Primary,
    Secondary,
    Quiet,
    Danger,
    Success,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Availability {
    Enabled,
    Disabled,
    Busy,
}

pub(crate) struct Button {
    label: String,
    action: Action,
    variant: Variant,
    availability: Availability,
}

impl Button {
    pub(crate) fn new(label: String, action: Action, variant: Variant) -> Self {
        Self {
            label,
            action,
            variant,
            availability: Availability::Enabled,
        }
    }

    pub(crate) const fn available(mut self, state: Availability) -> Self {
        self.availability = state;
        self
    }

    pub(crate) fn draw(self, frame: &mut Frame, rect: Rect, scene: &mut Scene) {
        if rect.is_empty() {
            return;
        }
        let active = self.availability == Availability::Enabled;
        let hover = active && scene.add(rect, self.action);
        let pressed = hover && scene.pressed == Some((rect, self.action));
        let focus = active && scene.focused == Some((rect, self.action));
        let (fg, bg) = match self.variant {
            Variant::Primary => (theme::BACKGROUND, theme::WARNING),
            Variant::Secondary => (theme::TEXT, theme::STEEL),
            Variant::Quiet => (theme::ACCENT, theme::SURFACE),
            Variant::Danger => (theme::TEXT, theme::DANGER),
            Variant::Success => (theme::BACKGROUND, theme::SUCCESS),
        };
        let (fg, bg, prefix) = if !active {
            (
                theme::MUTED,
                theme::SURFACE,
                if self.availability == Availability::Busy {
                    ".. "
                } else {
                    "- "
                },
            )
        } else if pressed {
            (theme::BACKGROUND, theme::TEXT, "v ")
        } else if hover {
            (theme::BACKGROUND, theme::ACCENT, "")
        } else if focus {
            (theme::BACKGROUND, theme::ACCENT, "> ")
        } else {
            (fg, bg, "")
        };
        let style = Style::new().fg(fg).bg(bg);
        fill(frame, rect, style);
        frame.render_widget(
            Paragraph::new(format!("{prefix}{}", self.label))
                .centered()
                .style(if active {
                    style.add_modifier(Modifier::BOLD)
                } else {
                    style
                }),
            Rect::new(rect.x, rect.y + (rect.height - 1) / 2, rect.width, 1),
        );
    }
}
