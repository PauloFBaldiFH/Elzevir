// SPDX-License-Identifier: GPL-3.0-or-later
//! Shared panel colours.

use ratatui::style::{Color, Modifier, Style};

pub(crate) const BACKGROUND: Color = Color::Rgb(0x16, 0x18, 0x21);
pub(crate) const SURFACE: Color = Color::Rgb(0x1e, 0x21, 0x30);
pub(crate) const BORDER: Color = Color::Rgb(0x3b, 0x42, 0x61);
pub(crate) const ACCENT: Color = Color::Rgb(0x7a, 0xa2, 0xf7);
pub(crate) const SUCCESS: Color = Color::Rgb(0x9e, 0xce, 0x6a);
pub(crate) const WARNING: Color = Color::Rgb(0xe0, 0xaf, 0x68);
pub(crate) const ERROR: Color = Color::Rgb(0xf7, 0x76, 0x8e);
pub(crate) const TEXT: Color = Color::Rgb(0xc0, 0xca, 0xf5);
pub(crate) const MUTED: Color = Color::Rgb(0x78, 0x7c, 0x99);

pub(crate) const fn surface() -> Style {
    Style::new().fg(TEXT).bg(SURFACE)
}

pub(crate) const fn selected() -> Style {
    Style::new()
        .fg(BACKGROUND)
        .bg(ACCENT)
        .add_modifier(Modifier::BOLD)
}

pub(crate) const fn muted() -> Style {
    Style::new().fg(MUTED).bg(BACKGROUND)
}
