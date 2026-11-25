// SPDX-License-Identifier: GPL-3.0-or-later
//! First-run form. The initializer validates and persists its result.

use crate::{
    Messages, PanelError, runtime,
    ui::{self, Action, Scene, theme},
};
use ratatui::{
    Frame,
    crossterm::event::{
        self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
    },
    layout::{Constraint, Rect},
    widgets::{Clear, Paragraph, Wrap},
};
use zeroize::Zeroizing;

/// The optional institutional settings requested by the operator.
#[derive(Clone, Copy, Debug, Default)]
pub enum SetupProfile {
    /// Leave the built-in catalog empty.
    #[default]
    Empty,
    /// Start with a public library's basic registries.
    Public,
    /// Start with a school's basic registries.
    School,
    /// Start with a university's basic registries.
    University,
}

/// Input for the first administrator and institution.
pub struct Setup {
    /// Institution name.
    pub name: String,
    /// Administrator password, zeroed on drop.
    pub password: Zeroizing<String>,
    /// Optional starting profile.
    pub profile: SetupProfile,
}

#[derive(Clone, Copy)]
enum Step {
    Name,
    Password,
    Confirm,
    Profile,
}

struct Wizard {
    step: Step,
    purpose: Purpose,
    invalid: bool,
    name: String,
    password: Zeroizing<String>,
    confirm: Zeroizing<String>,
    profile: usize,
    mismatch: bool,
}

/// Collects first-use choices in a terminal dialog with hidden password fields.
///
/// # Errors
/// Reports terminal, I/O and translation failures.
pub fn setup(language: &str) -> Result<Option<Setup>, PanelError> {
    form(language, Purpose::Initialize)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Purpose {
    Initialize,
    Institution,
    Account,
}

/// Collect only institution settings. Staff accounts are managed in the panel.
///
/// # Errors
/// Terminal or translation failure.
pub fn institution_setup(language: &str) -> Result<Option<Setup>, PanelError> {
    form(language, Purpose::Institution)
}
fn form(language: &str, purpose: Purpose) -> Result<Option<Setup>, PanelError> {
    let messages = runtime::prepare(language)?;
    runtime::with_terminal(|mut terminal| dialog(&mut terminal, &messages, purpose, None))
}

pub(crate) fn dialog(
    terminal: &mut ratatui::DefaultTerminal,
    messages: &Messages,
    purpose: Purpose,
    backdrop: Option<(&crate::Panel, bool)>,
) -> std::io::Result<Option<Setup>> {
    let mut wizard = Wizard {
        step: Step::Name,
        purpose,
        invalid: false,
        name: String::new(),
        password: Zeroizing::new(String::new()),
        confirm: Zeroizing::new(String::new()),
        profile: 0,
        mismatch: false,
    };
    let mut scene = Scene::default();
    loop {
        terminal.draw(|frame| {
            if let Some((panel, ascii)) = backdrop {
                crate::draw(frame, panel, messages, ascii);
                ui::dim(frame);
            } else {
                ui::fill(frame, frame.area(), theme::muted());
            }
            wizard.draw(frame, messages, &mut scene);
        })?;
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
        if let Some(key) = key {
            if key.code == KeyCode::Esc
                || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
            {
                return Ok(None);
            }
            if wizard.handle(key) {
                return Ok(Some(Setup {
                    name: wizard.name,
                    password: wizard.password,
                    profile: [
                        SetupProfile::Empty,
                        SetupProfile::Public,
                        SetupProfile::School,
                        SetupProfile::University,
                    ]
                    .get(wizard.profile)
                    .copied()
                    .unwrap_or_default(),
                }));
            }
        }
    }
}

impl Wizard {
    fn handle(&mut self, key: KeyEvent) -> bool {
        if key.code == KeyCode::Enter {
            self.invalid = match self.step {
                Step::Name if self.purpose == Purpose::Account => {
                    self.name.is_empty()
                        || self.name.len() > 64
                        || !self
                            .name
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-' | b'_'))
                }
                Step::Name => self.name.trim().is_empty(),
                Step::Password => self.password.chars().count() < 8,
                _ => false,
            };
            if self.invalid {
                return false;
            }
            self.step = match self.step {
                Step::Name if self.purpose == Purpose::Institution => Step::Profile,
                Step::Name => Step::Password,
                Step::Password => Step::Confirm,
                Step::Confirm
                    if self.purpose == Purpose::Account
                        && self.password.as_str() == self.confirm.as_str() =>
                {
                    return true;
                }
                Step::Confirm if self.password.as_str() == self.confirm.as_str() => Step::Profile,
                Step::Confirm => {
                    self.confirm = Zeroizing::new(String::new());
                    self.password = Zeroizing::new(String::new());
                    self.mismatch = true;
                    Step::Password
                }
                Step::Profile => return true,
            };
            return false;
        }
        if matches!(self.step, Step::Profile) {
            match key.code {
                KeyCode::Left | KeyCode::Up => self.profile = (self.profile + 3) % 4,
                KeyCode::Right | KeyCode::Down => self.profile = (self.profile + 1) % 4,
                KeyCode::Char(c @ '0'..='3') => {
                    self.profile = c.to_digit(10).unwrap_or_default() as usize;
                }
                _ => {}
            }
            return false;
        }
        let field = match self.step {
            Step::Name => &mut self.name,
            Step::Password => &mut *self.password,
            Step::Confirm => &mut *self.confirm,
            Step::Profile => return false,
        };
        match key.code {
            KeyCode::Backspace => {
                field.pop();
            }
            KeyCode::Char(c)
                if !c.is_control()
                    && !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                    && field.len() + c.len_utf8() <= 1024 =>
            {
                field.push(c);
            }
            _ => {}
        }
        false
    }

    fn field(&self) -> (&'static str, String) {
        match self.step {
            Step::Name if self.purpose == Purpose::Account => {
                ("account-login", crate::dashboard::text(&self.name))
            }
            Step::Name if self.purpose == Purpose::Institution => {
                ("institution-name", crate::dashboard::text(&self.name))
            }
            Step::Name => ("setup-name", crate::dashboard::text(&self.name)),
            Step::Password => (
                "setup-password",
                "•".repeat(self.password.chars().count().min(50)),
            ),
            Step::Confirm => (
                "setup-repeat",
                "•".repeat(self.confirm.chars().count().min(50)),
            ),
            Step::Profile if self.purpose == Purpose::Institution => {
                ("institution-profile", String::new())
            }
            Step::Profile => ("setup-profile", String::new()),
        }
    }

    fn draw(&self, frame: &mut Frame, messages: &Messages, scene: &mut Scene) {
        scene.clear();
        let screen = frame.area();
        if screen.width < 38 || screen.height < 10 {
            frame.render_widget(
                Paragraph::new(messages.get("setup-small")).wrap(Wrap { trim: true }),
                screen,
            );
            return;
        }
        let area = screen.centered(Constraint::Length(76), Constraint::Length(12));
        frame.render_widget(Clear, area);
        let (key, value) = self.field();
        let warning = if self.invalid {
            messages.get(match self.step {
                Step::Password => "account-short",
                Step::Name if self.purpose == Purpose::Account => "account-login-rule",
                _ => "account-empty",
            })
        } else if self.mismatch && matches!(self.step, Step::Password) {
            messages.get("setup-mismatch")
        } else {
            String::new()
        };
        let body = format!("{}\n\n{value}\n{warning}", messages.get(key));
        frame.render_widget(
            Paragraph::new(body)
                .wrap(Wrap { trim: false })
                .block(crate::view::block(
                    messages.get(if self.purpose == Purpose::Account {
                        "account-title"
                    } else {
                        "setup-title"
                    }),
                    false,
                )),
            area,
        );
        if matches!(self.step, Step::Profile) && area.height >= 8 {
            let choices = [
                "setup-empty",
                "setup-public",
                "setup-school",
                "setup-university",
            ];
            scene.buttons(
                frame,
                Rect::new(
                    area.x + 1,
                    area.bottom() - 4,
                    area.width.saturating_sub(2),
                    1,
                ),
                choices
                    .into_iter()
                    .zip('0'..='3')
                    .enumerate()
                    .map(|(index, (label, key))| {
                        (
                            messages.get(label),
                            Action::Key(KeyCode::Char(key)),
                            self.profile == index,
                        )
                    }),
            );
        }
        ui::modal_buttons(
            frame,
            area,
            messages,
            false,
            scene,
            Some((
                if self.purpose == Purpose::Account && matches!(self.step, Step::Confirm) {
                    "account-save"
                } else if matches!(self.step, Step::Profile) {
                    "setup-finish"
                } else {
                    "ui-next"
                },
                KeyCode::Enter,
            )),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_institution_requires_a_valid_login_and_confirmed_password() {
        let mut wizard = Wizard {
            step: Step::Name,
            purpose: Purpose::Account,
            invalid: false,
            name: "bad login".into(),
            password: Zeroizing::default(),
            confirm: Zeroizing::default(),
            profile: 0,
            mismatch: false,
        };
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        assert!(!wizard.handle(enter) && wizard.invalid);
        wizard.name = "library.admin".into();
        assert!(!wizard.handle(enter) && matches!(wizard.step, Step::Password));
        assert!(!wizard.handle(enter) && wizard.invalid);
        wizard.password.push_str("fixture-password");
        assert!(!wizard.handle(enter) && matches!(wizard.step, Step::Confirm));
        assert!(!wizard.handle(enter) && wizard.mismatch && wizard.password.is_empty());
        wizard.password.push_str("fixture-password");
        assert!(!wizard.handle(enter));
        wizard.confirm.push_str("fixture-password");
        assert!(wizard.handle(enter));
    }
}
