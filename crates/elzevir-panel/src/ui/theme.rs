// SPDX-License-Identifier: GPL-3.0-or-later
//! Shared panel colours.

use ratatui::style::{Color, Modifier, Style};

pub(crate) const BACKGROUND: Color = Color::Rgb(0x0b, 0x17, 0x25);
pub(crate) const SURFACE: Color = Color::Rgb(0x16, 0x29, 0x3b);
pub(crate) const BORDER: Color = Color::Rgb(0x36, 0x50, 0x67);
pub(crate) const ACCENT: Color = Color::Rgb(0x79, 0xc2, 0xfa);
pub(crate) const SUCCESS: Color = Color::Rgb(0x75, 0xda, 0xad);
pub(crate) const WARNING: Color = Color::Rgb(0xff, 0xb4, 0x57);
pub(crate) const TEXT: Color = Color::Rgb(0xe5, 0xef, 0xf8);
pub(crate) const MUTED: Color = Color::Rgb(0xa0, 0xb4, 0xc6);

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

pub(crate) const STEEL: Color = Color::Rgb(43, 74, 98);
pub(crate) const DANGER: Color = Color::Rgb(142, 61, 71);
