// SPDX-License-Identifier: GPL-3.0-or-later
//! Demo navigation and deterministic terminal rendering.
#![expect(
    clippy::unwrap_used,
    reason = "rendering helpers fail the integration test on error"
)]

use elzevir_panel::{Messages, Panel, Section, draw};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn screen(panel: &Panel, lang: &str, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    let messages = Messages::new(lang).unwrap();
    terminal
        .draw(|frame| draw(frame, panel, &messages, false))
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .chunks(usize::from(width))
        .map(|row| {
            row.iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn navigation_wraps_and_release_events_are_ignored() {
    let mut panel = Panel::default();
    panel.handle(&key(KeyCode::BackTab), 120, 24);
    assert_eq!(panel.section, Section::Administrators);
    panel.handle(&key(KeyCode::Tab), 120, 24);
    assert_eq!(panel.section, Section::Dashboard);
    panel.handle(
        &Event::Key(KeyEvent {
            kind: KeyEventKind::Release,
            ..KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)
        }),
        120,
        24,
    );
    assert_eq!(panel.section, Section::Dashboard);
    for section in Section::ALL {
        panel.handle(&key(KeyCode::Char(section.shortcut())), 120, 24);
        assert_eq!(panel.section, section);
    }
}

#[test]
fn palette_and_mouse_use_the_same_sections() {
    let mut panel = Panel::default();
    panel.handle(
        &Event::Key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL)),
        120,
        24,
    );
    assert!(panel.palette);
    panel.handle(&key(KeyCode::Esc), 120, 24);
    assert!(!panel.palette);
    panel.handle(
        &Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 3,
            row: 5,
            modifiers: KeyModifiers::NONE,
        }),
        120,
        24,
    );
    assert_eq!(panel.section, Section::Backup);
    panel.handle(
        &Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 100,
            row: 15,
            modifiers: KeyModifiers::NONE,
        }),
        120,
        24,
    );
    assert_eq!(panel.section, Section::Backup);
    panel.handle(
        &Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        120,
        24,
    );
    assert!(panel.quit);
}

#[test]
fn demo_screen_is_explicit_and_translated() {
    let panel = Panel::default();
    insta::assert_snapshot!(screen(&panel, "en", 120, 24));
    insta::assert_snapshot!(screen(&panel, "pt-BR", 120, 24));
    for section in Section::ALL {
        let panel = Panel {
            section,
            ..Panel::default()
        };
        for lang in ["en", "pt-BR"] {
            let rendered = screen(&panel, lang, 120, 24);
            assert!(
                !rendered.contains("panel-"),
                "missing Fluent message: {rendered}"
            );
        }
    }
    assert!(Messages::new("invalid").is_err());
}

#[test]
fn small_terminal_and_palette_render_without_panicking() {
    let panel = Panel {
        palette: true,
        ..Panel::default()
    };
    for (width, height) in [(1, 1), (79, 17), (80, 18), (120, 24)] {
        let rendered = screen(&panel, "en", width, height);
        if width == 79 {
            assert!(rendered.contains("80x18"));
        }
    }
}

#[test]
fn palette_click_selects_the_section_and_ascii_keeps_labels_readable() {
    let mut panel = Panel {
        palette: true,
        ..Panel::default()
    };
    panel.handle(
        &Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 38,
            row: 13,
            modifiers: KeyModifiers::NONE,
        }),
        120,
        24,
    );
    assert_eq!(panel.section, Section::Updates);
    assert!(!panel.palette);
    let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();
    let messages = Messages::new("en").unwrap();
    terminal
        .draw(|frame| draw(frame, &panel, &messages, true))
        .unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect();
    assert!(text.is_ascii());
    assert!(text.contains("b Backups and schedules"));
    assert!(text.contains("Server status: not read"));
}
