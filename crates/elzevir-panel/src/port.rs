// SPDX-License-Identifier: GPL-3.0-or-later
//! Port-conflict dialog: the operator picks another port instead of editing files.

use crate::{
    Messages, PanelError, runtime,
    ui::{self, Action, Scene, theme},
};
use ratatui::{
    Frame,
    crossterm::event::{
        self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
    },
    layout::Constraint,
    widgets::{Clear, Paragraph, Wrap},
};

/// Lowest port offered: below it most systems require administrator rights.
pub const MIN_PORT: u16 = 1024;

/// Editable port field with the reason the last value was refused.
#[derive(Debug)]
pub struct PortForm {
    busy: u16,
    value: String,
    refused: Option<Refusal>,
}

/// Why a typed port cannot be used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Not a number between [`MIN_PORT`] and 65535.
    OutOfRange,
    /// The same port that is already taken.
    StillBusy,
}

/// What a key press did to the form.
#[derive(Debug, PartialEq, Eq)]
pub enum PortStep {
    /// Keep asking.
    Editing,
    /// The operator confirmed a valid port.
    Chosen(u16),
}

impl PortForm {
    /// Starts with the suggested free port already typed, so Enter accepts it.
    #[must_use]
    pub fn new(busy: u16, suggestion: Option<u16>) -> Self {
        Self {
            busy,
            value: suggestion.map(|port| port.to_string()).unwrap_or_default(),
            refused: None,
        }
    }

    /// Marks a confirmed port as taken after the caller failed to reserve it.
    pub fn reject(&mut self, port: u16) {
        self.busy = port;
        self.refused = Some(Refusal::StillBusy);
    }

    /// Applies one key; digits edit the field and Enter validates it.
    pub fn handle(&mut self, key: KeyEvent) -> PortStep {
        match key.code {
            KeyCode::Enter => match self.value.parse::<u16>() {
                Ok(port) if port == self.busy => self.refused = Some(Refusal::StillBusy),
                Ok(port) if port >= MIN_PORT => return PortStep::Chosen(port),
                _ => self.refused = Some(Refusal::OutOfRange),
            },
            KeyCode::Backspace => {
                self.value.pop();
            }
            KeyCode::Char(digit @ '0'..='9') if self.value.len() < 5 => {
                self.value.push(digit);
                self.refused = None;
            }
            _ => {}
        }
        PortStep::Editing
    }

    fn draw(&self, frame: &mut Frame, messages: &Messages, scene: &mut Scene) {
        scene.clear();
        let screen = frame.area();
        ui::fill(frame, screen, theme::muted());
        if screen.width < 38 || screen.height < 10 {
            frame.render_widget(
                Paragraph::new(messages.get("setup-small")).wrap(Wrap { trim: true }),
                screen,
            );
            return;
        }
        let area = screen.centered(Constraint::Length(72), Constraint::Length(11));
        frame.render_widget(Clear, area);
        let warning = match self.refused {
            Some(Refusal::OutOfRange) => messages.get("port-invalid"),
            Some(Refusal::StillBusy) => {
                messages.values("port-still-busy", &[("port", self.busy.to_string())])
            }
            None => String::new(),
        };
        let body = format!(
            "{}\n\n{}  {}\n{warning}",
            messages.values("port-busy", &[("port", self.busy.to_string())]),
            messages.get("port-field"),
            self.value,
        );
        frame.render_widget(
            Paragraph::new(body)
                .wrap(Wrap { trim: false })
                .block(crate::view::block(messages.get("port-title"), false)),
            area,
        );
        ui::modal_buttons(
            frame,
            area,
            messages,
            false,
            scene,
            Some(("port-use", KeyCode::Enter)),
        );
    }
}

/// Asks for another port until `reserve` accepts one; `None` when cancelled.
///
/// `reserve` tries the chosen port and returns whether it is free, so a port
/// taken while the dialog was open is refused in place instead of aborting.
///
/// # Errors
/// Reports terminal, I/O and translation failures.
pub fn choose_port(
    language: &str,
    busy: u16,
    suggestion: Option<u16>,
    mut reserve: impl FnMut(u16) -> bool,
) -> Result<Option<u16>, PanelError> {
    let messages = runtime::prepare(language)?;
    runtime::with_terminal(|mut terminal| {
        let mut form = PortForm::new(busy, suggestion);
        let mut scene = Scene::default();
        loop {
            terminal.draw(|frame| form.draw(frame, &messages, &mut scene))?;
            let key = match event::read()? {
                Event::Key(key) if key.kind != KeyEventKind::Release => Some(key),
                Event::Mouse(mouse) => {
                    let point = (mouse.column, mouse.row).into();
                    scene.pointer = Some(point);
                    match (mouse.kind, scene.hit(point)) {
                        (MouseEventKind::Down(MouseButton::Left), Some(Action::Key(key))) => {
                            Some(KeyEvent::new(key, KeyModifiers::NONE))
                        }
                        _ => None,
                    }
                }
                _ => None,
            };
            let Some(key) = key else { continue };
            if key.code == KeyCode::Esc
                || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
            {
                return Ok(None);
            }
            if let PortStep::Chosen(port) = form.handle(key) {
                if reserve(port) {
                    return Ok(Some(port));
                }
                form.reject(port);
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(form: &mut PortForm, code: KeyCode) -> PortStep {
        form.handle(KeyEvent::new(code, KeyModifiers::NONE))
    }

    #[test]
    fn the_suggestion_is_accepted_with_enter() {
        let mut form = PortForm::new(7390, Some(7391));
        assert_eq!(press(&mut form, KeyCode::Enter), PortStep::Chosen(7391));
    }

    #[test]
    fn typed_ports_are_validated_before_use() {
        let mut form = PortForm::new(7390, None);
        assert_eq!(press(&mut form, KeyCode::Enter), PortStep::Editing);
        assert_eq!(form.refused, Some(Refusal::OutOfRange));
        for digit in "80".chars() {
            press(&mut form, KeyCode::Char(digit));
        }
        assert_eq!(press(&mut form, KeyCode::Enter), PortStep::Editing);
        assert_eq!(form.refused, Some(Refusal::OutOfRange));
        press(&mut form, KeyCode::Backspace);
        press(&mut form, KeyCode::Backspace);
        for digit in "7390".chars() {
            press(&mut form, KeyCode::Char(digit));
        }
        assert_eq!(press(&mut form, KeyCode::Enter), PortStep::Editing);
        assert_eq!(form.refused, Some(Refusal::StillBusy));
        press(&mut form, KeyCode::Backspace);
        press(&mut form, KeyCode::Char('5'));
        press(&mut form, KeyCode::Char('9'));
        assert_eq!(form.value, "73959");
        press(&mut form, KeyCode::Char('1'));
        assert_eq!(form.value, "73959", "a port has at most five digits");
        assert_eq!(press(&mut form, KeyCode::Enter), PortStep::Editing);
        assert_eq!(form.refused, Some(Refusal::OutOfRange));
    }

    #[test]
    fn a_port_taken_meanwhile_is_refused_in_place() {
        let mut form = PortForm::new(7390, Some(7391));
        form.reject(7391);
        assert_eq!(form.refused, Some(Refusal::StillBusy));
        assert_eq!(press(&mut form, KeyCode::Enter), PortStep::Editing);
    }
}
