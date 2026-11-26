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
    layout::{Constraint, Rect, Size},
    style::{Modifier, Style},
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
    /// Login chosen for the initial administrator.
    pub login: String,
    /// Administrator password, zeroed on drop.
    pub password: Zeroizing<String>,
    /// Optional starting profile.
    pub profile: SetupProfile,
}

#[derive(Clone, Copy)]
enum Step {
    Name,
    Login,
    Password,
    Confirm,
    Profile,
}

struct Wizard {
    login: String,
    step: Step,
    name: String,
    password: Zeroizing<String>,
    confirm: Zeroizing<String>,
    profile: usize,
    error: Option<&'static str>,
}

/// Collects first-use choices in a terminal dialog with hidden password fields.
///
/// # Errors
/// Reports terminal, I/O and translation failures.
pub fn setup(language: &str) -> Result<Option<Setup>, PanelError> {
    setup_with_login(language, "admin", false)
}

/// Collects the initial account, using the suggested login and terminal border mode.
///
/// # Errors
/// Reports terminal and localization failures.
pub fn setup_with_login(
    language: &str,
    login: &str,
    ascii: bool,
) -> Result<Option<Setup>, PanelError> {
    let messages = runtime::prepare(language)?;
    runtime::with_terminal(|mut terminal| {
        let mut wizard = Wizard {
            step: Step::Name,
            login: login.to_owned(),
            name: String::new(),
            password: Zeroizing::new(String::new()),
            confirm: Zeroizing::new(String::new()),
            profile: 0,
            error: None,
        };
        let mut scene = Scene::default();
        loop {
            let drawn = terminal
                .draw(|frame| wizard.draw(frame, &messages, &mut scene, ascii))?
                .area;
            let key = match event::read()? {
                Event::Key(key) if key.kind != KeyEventKind::Release => Some(key),
                Event::Mouse(mouse) => {
                    let point = (mouse.column, mouse.row).into();
                    scene.pointer = Some(point);
                    match mouse.kind {
                        MouseEventKind::Down(MouseButton::Left) => {
                            scene.press(point);
                            None
                        }
                        MouseEventKind::Up(MouseButton::Left) => match scene.release(point) {
                            Some(Action::Key(key)) => Some(KeyEvent::new(key, KeyModifiers::NONE)),
                            _ => None,
                        },
                        _ => None,
                    }
                }
                Event::Resize(_, _) => {
                    scene.reset_input();
                    None
                }
                _ => None,
            };
            if let Some(key) = key {
                if key.code == KeyCode::Esc
                    || (key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL))
                {
                    return Ok(None);
                }
                let size = terminal.size()?;
                if size.width == drawn.width
                    && size.height == drawn.height
                    && wizard.handle(key, size)
                {
                    return Ok(Some(Setup {
                        name: wizard.name,
                        login: wizard.login,
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
    })
}

impl Wizard {
    fn handle(&mut self, key: KeyEvent, size: Size) -> bool {
        if !fits(size) {
            return false;
        }
        if key.code == KeyCode::Enter {
            self.step = match self.step {
                Step::Name if self.name.trim().is_empty() || self.name.chars().count() > 200 => {
                    self.error = Some("setup-invalid-name");
                    return false;
                }
                Step::Name => Step::Login,
                Step::Login
                    if self.login.is_empty()
                        || self.login.len() > 64
                        || !self.login.bytes().all(|b| {
                            b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-')
                        }) =>
                {
                    self.error = Some("setup-invalid-login");
                    return false;
                }
                Step::Login => Step::Password,
                Step::Password if self.password.chars().count() < 8 => {
                    self.error = Some("setup-short");
                    return false;
                }
                Step::Password => Step::Confirm,
                Step::Confirm if self.password.as_str() == self.confirm.as_str() => Step::Profile,
                Step::Confirm => {
                    self.confirm = Zeroizing::new(String::new());
                    self.password = Zeroizing::new(String::new());
                    self.error = Some("setup-mismatch");
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
        self.error = None;
        let field = match self.step {
            Step::Name => &mut self.name,
            Step::Login => &mut self.login,
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

    fn draw(&self, frame: &mut Frame, messages: &Messages, scene: &mut Scene, ascii: bool) {
        scene.clear();
        let screen = frame.area();
        ui::fill(frame, screen, theme::muted());
        if !fits(Size::new(screen.width, screen.height)) {
            frame.render_widget(
                Paragraph::new(messages.get("setup-small")).wrap(Wrap { trim: true }),
                screen,
            );
            return;
        }
        let area = screen.centered(Constraint::Length(76), Constraint::Length(12));
        frame.render_widget(Clear, area);
        let (key, value) = match self.step {
            Step::Name => ("setup-name", crate::dashboard::text(&self.name)),
            Step::Login => ("setup-login", crate::dashboard::text(&self.login)),
            Step::Password => (
                "setup-password-step",
                (if ascii { "*" } else { "•" }).repeat(self.password.chars().count().min(50)),
            ),
            Step::Confirm => (
                "setup-repeat-step",
                (if ascii { "*" } else { "•" }).repeat(self.confirm.chars().count().min(50)),
            ),
            Step::Profile => ("setup-profile", String::new()),
        };
        let title = format!(" {} ", messages.get("setup-title"));
        frame.render_widget(crate::view::block(title, ascii), area);
        let inner = |offset, height| {
            Rect::new(
                area.x + 2,
                area.y + offset,
                area.width.saturating_sub(4),
                height,
            )
        };
        frame.render_widget(
            Paragraph::new(messages.get(key)).style(
                theme::surface()
                    .fg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD),
            ),
            inner(1, 1),
        );
        let hint = match self.step {
            Step::Name => "setup-name-help",
            Step::Login => "setup-login-help",
            Step::Password | Step::Confirm => "setup-password-help",
            Step::Profile => "setup-profile-help",
        };
        frame.render_widget(
            Paragraph::new(messages.get(hint)).wrap(Wrap { trim: true }),
            inner(2, 2),
        );
        if !matches!(self.step, Step::Profile) {
            let cursor = if ascii { "_" } else { "▏" };
            let field = inner(4, 3);
            let line = ratatui::text::Line::from(format!("{value}{cursor}"));
            let width = line.width();
            let value = Paragraph::new(line).block(
                crate::view::block(String::new(), ascii)
                    .border_style(Style::new().fg(theme::ACCENT)),
            );
            // Keep the end of a long value and its insertion point visible.
            let offset = u16::try_from(width)
                .unwrap_or(u16::MAX)
                .saturating_sub(field.width.saturating_sub(2));
            frame.render_widget(value.scroll((0, offset)), field);
        }
        if let Some(key) = self.error {
            frame.render_widget(
                Paragraph::new(messages.get(key))
                    .style(theme::surface().fg(theme::WARNING))
                    .wrap(Wrap { trim: true }),
                inner(7, 2),
            );
        }
        self.profile_choices(frame, scene, area, messages);
        ui::modal_buttons(
            frame,
            area,
            messages,
            ascii,
            scene,
            Some((
                if matches!(self.step, Step::Profile) {
                    "setup-finish"
                } else {
                    "ui-next"
                },
                KeyCode::Enter,
            )),
        );
    }
    fn profile_choices(
        &self,
        frame: &mut Frame,
        scene: &mut Scene,
        area: Rect,
        messages: &Messages,
    ) {
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
    }
}

fn fits(size: Size) -> bool {
    size.width >= 38 && size.height >= 10
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_hidden_confirmation_cannot_create_an_institution() {
        let mut wizard = Wizard {
            login: "admin".into(),
            step: Step::Profile,
            name: "Library".into(),
            password: Zeroizing::new(String::new()),
            confirm: Zeroizing::new(String::new()),
            profile: 0,
            error: None,
        };
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        assert!(!wizard.handle(enter, Size::new(37, 10)));
        assert!(!wizard.handle(enter, Size::new(80, 9)));
        assert!(wizard.handle(enter, Size::new(38, 10)));
    }
}
