// SPDX-License-Identifier: GPL-3.0-or-later
//! Read-only installation overview used before an operational session is available.
use crate::{
    Messages, Section,
    dashboard::text,
    ui::{Action, Scene, card, theme},
};
use ratatui::{
    Frame,
    crossterm::event::KeyCode,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::Line,
};

/// Reason the normal personal startup was deferred.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallationIssue {
    /// Empty default destination suitable for the existing initializer.
    New,
    /// Existing database without configuration.
    Legacy,
    /// Configuration exists but cannot be loaded.
    InvalidConfig,
    /// A configured database is absent.
    MissingDatabase,
    /// A read under the institution lock found no administrator.
    MissingAdministrator,
    /// Inspection could not establish safe startup.
    Unavailable,
}

/// An explicit operator choice made in local inspection mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InspectionAction {
    /// Start the initializer for a new destination.
    Create,
    /// Attach to the already running server.
    Connect,
}

/// Evidence obtained before starting services or opening an institution for writing.
#[derive(Debug)]
pub struct Inspection {
    /// Local path that needs the operator's attention.
    pub location: String,
    /// Result of the installation inspection.
    pub issue: InstallationIssue,
    /// A live server was confirmed by local discovery.
    pub running: bool,
    /// Admin membership observed under the existing lock.
    pub administrator: Option<bool>,
}

impl Inspection {
    pub(crate) fn draw(
        &self,
        frame: &mut Frame,
        area: Rect,
        section: Section,
        m: &Messages,
        scene: &mut Scene,
        ascii: bool,
    ) {
        let reason = match self.issue {
            InstallationIssue::New => "inspect-new",
            InstallationIssue::MissingAdministrator => "inspect-no-admin",
            InstallationIssue::Legacy => "inspect-legacy",
            InstallationIssue::MissingDatabase => "inspect-no-db",
            InstallationIssue::InvalidConfig => "inspect-config",
            InstallationIssue::Unavailable => "inspect-unknown",
        };
        let fresh = self.issue == InstallationIssue::New;
        let tint = if fresh { theme::ACCENT } else { theme::WARNING };
        let (summary, next) = inspection_areas(area);
        let status = if self.running {
            "inspect-active"
        } else if fresh {
            "inspect-empty"
        } else {
            "inspect-review"
        };
        card(
            frame,
            summary,
            &m.get("inspect-installation"),
            vec![
                Line::from(m.get(status)).style(Style::new().fg(tint).add_modifier(Modifier::BOLD)),
                Line::from(m.get(reason)),
                Line::default(),
                Line::from(text(&self.location)),
            ],
            tint,
            ascii,
            0,
        );
        let title = if section == Section::Dashboard {
            "section-settings"
        } else {
            section.key()
        };
        let help = match section {
            Section::Dashboard | Section::Settings if fresh => "inspect-new-help".to_owned(),
            Section::Dashboard | Section::Settings
                if self.issue == InstallationIssue::MissingAdministrator =>
            {
                "inspect-admin-help".to_owned()
            }
            Section::Dashboard | Section::Settings => match self.administrator {
                Some(true) => "inspect-admin-present",
                Some(false) => "inspect-admin-help",
                None => "inspect-preserve",
            }
            .to_owned(),
            other => format!("{}-body", other.key()),
        };
        card(
            frame,
            next,
            &m.get(title),
            vec![Line::from(m.get(&help))],
            theme::ACCENT,
            ascii,
            0,
        );
        let action = match section {
            Section::Settings if fresh => Some(("inspect-create", 'c')),
            Section::Settings if self.running => Some(("inspect-connect", 'c')),
            Section::Settings => None,
            _ => Some(("section-settings", 'g')),
        };
        if next.height >= 5
            && let Some((label, shortcut)) = action
        {
            let text = format!("{} [{shortcut}]", m.get(label));
            let width = u16::try_from(text.chars().count() + 4)
                .unwrap_or(u16::MAX)
                .min(next.width.saturating_sub(4));
            scene.buttons(
                frame,
                Rect::new(next.x + 2, next.bottom() - 2, width, 1),
                [(text, Action::Key(KeyCode::Char(shortcut)), true)],
            );
        }
    }
}

fn inspection_areas(area: Rect) -> (Rect, Rect) {
    let width = area.width.saturating_sub(4).min(116);
    let content = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + 1,
        width,
        area.height.saturating_sub(1),
    );
    if width >= 98 {
        let [left, right] = Layout::horizontal([Constraint::Percentage(55), Constraint::Fill(1)])
            .spacing(1)
            .areas(content);
        (
            Rect {
                height: left.height.min(10),
                ..left
            },
            Rect {
                height: right.height.min(10),
                ..right
            },
        )
    } else {
        let [top, bottom, _] = Layout::vertical([
            Constraint::Length(7),
            Constraint::Length(7),
            Constraint::Min(0),
        ])
        .spacing(1)
        .areas(content);
        (top, bottom)
    }
}
