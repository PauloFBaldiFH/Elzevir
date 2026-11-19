// SPDX-License-Identifier: GPL-3.0-or-later
//! Terminal administration interface.

mod address_operations;
mod address_view;
pub use address_operations::AddressOperation;
mod addresses;
mod cloud;
mod dashboard;
mod domain;
pub use cloud::{CloudConnection, CloudOperation, CloudPlan, CloudQuestion, CloudView};
pub use domain::{ConnectorState, DomainPhase, DomainStatus};
mod diagnostic_view;
mod diagnostics;
mod live;
mod messages;
mod operation_view;
mod operations;
mod runtime;
mod setup;
pub use setup::{Setup, SetupProfile, setup};
mod sharing;
mod tunnel_view;
mod ui;
mod view;
mod worker;

pub use addresses::{
    AddressBook, PublicCheck, RemoteAddress, StaffAccess, Tunnel, TunnelLog, TunnelState, TunnelTab,
};
pub use diagnostics::{
    CheckResult, DiagnosticAction, Diagnostics, ExportState, LogEntry, LogEvent, LogFeed, LogState,
    ObservationRequest,
};
pub use live::{
    Component, Details, Host, Institution, LiveState, Mode, ServerStatus, Snapshot, Storage,
};
pub use messages::Messages;
pub use operations::{Form, Job, Operation, Operations};
pub use runtime::{run, run_live, run_live_until};
pub use view::draw;

use ratatui::crossterm::event::{
    Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};

/// A failure loading messages or operating the terminal.
#[derive(Debug, thiserror::Error)]
pub enum PanelError {
    /// Invalid or unsupported embedded translations.
    #[error("panel messages: {0}")]
    Messages(String),
    /// The terminal could not be opened, read, drawn or restored.
    #[error("panel terminal: {0}")]
    Terminal(#[from] std::io::Error),
}

/// The six administration sections, shared by keyboard, mouse and rendering.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Section {
    /// Service overview.
    #[default]
    Dashboard,
    /// Health checks and logs.
    Diagnostics,
    /// Public and local addresses.
    Addresses,
    /// Backups and schedules.
    Backup,
    /// Release management.
    Updates,
    /// Service and machine restart.
    Restart,
}

impl Section {
    /// Navigation order.
    pub const ALL: [Self; 6] = [
        Self::Dashboard,
        Self::Diagnostics,
        Self::Addresses,
        Self::Backup,
        Self::Updates,
        Self::Restart,
    ];

    /// Fluent message key for this section.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Dashboard => "panel-dashboard",
            Self::Diagnostics => "panel-diagnostics",
            Self::Addresses => "panel-addresses",
            Self::Backup => "panel-backup",
            Self::Updates => "panel-updates",
            Self::Restart => "panel-restart",
        }
    }

    /// Direct keyboard shortcut, identical in each language.
    #[must_use]
    pub const fn shortcut(self) -> char {
        match self {
            Self::Dashboard => 'd',
            Self::Diagnostics => 'x',
            Self::Addresses => 'a',
            Self::Backup => 'b',
            Self::Updates => 'u',
            Self::Restart => 'r',
        }
    }

    fn adjacent(self, forward: bool) -> Self {
        let index = Self::ALL
            .iter()
            .position(|section| *section == self)
            .unwrap_or_default();
        let step = if forward { 1 } else { Self::ALL.len() - 1 };
        Self::ALL
            .get((index + step) % Self::ALL.len())
            .copied()
            .unwrap_or(self)
    }
}

/// Pure presentation state. It owns no database, filesystem or network handles.
#[derive(Debug, Default)]
pub struct Panel {
    /// Demo or a selected institution's observations.
    pub mode: Mode,
    /// Coalesced manual refresh request.
    pub refresh_requested: bool,
    /// Coalesced explicit diagnostic work and its presentation state.
    pub diagnostics: DiagnosticAction,
    /// Bounded operation form and explicit confirmation.
    pub form: Option<Form>,
    /// Selected public tunnel detail tab.
    pub tunnel_tab: TunnelTab,
    /// Vertical dashboard scroll offset.
    pub scroll: u16,
    /// Currently selected section.
    pub section: Section,
    /// Whether the section palette is open.
    pub palette: bool,
    /// Whether the user requested exit.
    pub quit: bool,
    /// Animation phase, advanced by the event loop.
    pub tick: usize,
}

impl Panel {
    /// Builds a real view with no fabricated initial observations.
    #[must_use]
    pub fn live(institution: Institution) -> Self {
        Self {
            mode: Mode::Live(Box::new(LiveState {
                institution,
                snapshot: None,
                age_seconds: 0,
            })),
            ..Self::default()
        }
    }
    /// Applies an input event using the same geometry as the renderer.
    pub fn handle(&mut self, event: &Event, width: u16, height: u16) {
        if self.form.is_some() && self.handle_form(event) {
            return;
        }
        let previous = self.section;
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                if key.code == KeyCode::Char('q')
                    || (key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL))
                {
                    self.quit = true;
                } else if key.code == KeyCode::Char('p')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                {
                    self.palette = !self.palette;
                } else {
                    match key.code {
                        KeyCode::Char(c) if self.begin_operation(c) => {}
                        KeyCode::Char('f') if matches!(&self.mode, Mode::Live(_)) => {
                            self.refresh_requested = true;
                        }
                        KeyCode::Char('v') if self.live_diagnostics() || self.live_addresses() => {
                            if !self.diagnostics.pending {
                                self.diagnostics.requested =
                                    Some(if self.section == Section::Addresses {
                                        ObservationRequest::TestAddresses
                                    } else {
                                        ObservationRequest::Verify
                                    });
                            }
                            self.diagnostics.pending = true;
                        }
                        KeyCode::Char('e') if self.live_diagnostics() => {
                            self.diagnostics.requested = Some(ObservationRequest::Export);
                            self.diagnostics.pending = true;
                        }
                        KeyCode::PageDown => self.scroll = self.scroll.saturating_add(5).min(500),
                        KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(5),
                        KeyCode::Home => self.scroll = 0,
                        KeyCode::Esc | KeyCode::Enter => self.palette = false,
                        KeyCode::Tab | KeyCode::Down | KeyCode::Right => {
                            self.section = self.section.adjacent(true);
                        }
                        KeyCode::BackTab | KeyCode::Up | KeyCode::Left => {
                            self.section = self.section.adjacent(false);
                        }
                        KeyCode::Char(c @ '1'..='9') => {
                            if let Some(section) = c
                                .to_digit(10)
                                .and_then(|n| Section::ALL.get(n as usize - 1))
                            {
                                self.select(*section);
                            }
                        }
                        KeyCode::Char(c) => {
                            if let Some(section) =
                                Section::ALL.into_iter().find(|s| s.shortcut() == c)
                            {
                                self.section = section;
                            }
                        }
                        _ => {}
                    }
                }
            }
            Event::Mouse(mouse) if mouse.kind == MouseEventKind::Down(MouseButton::Left) => {
                self.handle_legacy_click(mouse.column, mouse.row, width, height);
            }
            _ => {}
        }
        if previous != self.section {
            self.enter_section();
        }
    }

    fn handle_legacy_click(&mut self, column: u16, row: u16, width: u16, height: u16) {
        if width < 80 || height < 18 {
            return;
        }
        let area = ratatui::layout::Rect::new(0, 0, width, height);
        let menu = view::menu_area(area, self.palette).inner(ratatui::layout::Margin::new(1, 1));
        if menu.contains((column, row).into())
            && let Some(section) = Section::ALL.get(usize::from(row - menu.y))
        {
            self.section = *section;
            self.palette = false;
        }
    }

    pub(crate) fn select(&mut self, section: Section) {
        if self.section != section {
            self.section = section;
            self.enter_section();
        }
        self.palette = false;
    }

    fn enter_section(&mut self) {
        self.scroll = 0;
    }

    fn live_diagnostics(&self) -> bool {
        matches!(&self.mode, Mode::Live(_)) && self.section == Section::Diagnostics && !self.palette
    }

    fn live_addresses(&self) -> bool {
        matches!(&self.mode, Mode::Live(_)) && self.section == Section::Addresses && !self.palette
    }
}

impl Panel {
    fn cloud_view(&self) -> Option<&CloudView> {
        if let Mode::Live(state) = &self.mode {
            state.snapshot.as_ref()?.operations.cloud.as_ref()
        } else {
            None
        }
    }
    fn begin_operation(&mut self, key: char) -> bool {
        if !matches!(&self.mode, Mode::Live(_)) || self.palette || self.diagnostics.pending {
            return false;
        }
        let operation = match (self.section, key) {
            (Section::Addresses, 'Q') => {
                self.tunnel_tab = if self.tunnel_tab == TunnelTab::Share {
                    TunnelTab::Test
                } else {
                    TunnelTab::Share
                };
                self.scroll = 0;
                return true;
            }
            (Section::Addresses, 'h') => {
                self.tunnel_tab = TunnelTab::Log;
                self.scroll = 0;
                return true;
            }
            (Section::Addresses, 't') => {
                self.tunnel_tab = TunnelTab::Test;
                Operation::TestTunnel
            }
            (Section::Addresses, key) if address_operations::shortcut(key).is_some() => {
                Operation::Address(
                    address_operations::shortcut(key).unwrap_or(AddressOperation::Refresh),
                )
            }
            (Section::Addresses, 'l') => Operation::CloudflareLogin,
            (Section::Addresses, 's') => Operation::CloudflareSetup {
                catalog: String::new(),
                staff: String::new(),
                name: String::new(),
                service: "elzevir".to_owned(),
                protocol: "auto".to_owned(),
            },
            (Section::Backup, key) if cloud::shortcut(key, self.cloud_view()).is_some() => {
                Operation::Cloud(
                    cloud::shortcut(key, self.cloud_view()).unwrap_or(CloudOperation::Providers),
                )
            }
            (Section::Backup, 'n' | 'v' | 'e' | 'k' | 'o' | 's') => backup_operation(
                key,
                if let Mode::Live(state) = &self.mode {
                    state.snapshot.as_ref().map(|snapshot| &snapshot.operations)
                } else {
                    None
                },
            ),
            (Section::Restart, 't') => Operation::Restart {
                name: "elzevir".to_owned(),
            },
            (Section::Restart, 'm') => Operation::Reboot,
            (Section::Updates, 'c') => Operation::CheckUpdates,
            (Section::Updates, 'r') => Operation::RecoverUpdate {
                executable: String::new(),
            },
            (Section::Updates, 'g') => Operation::InstallCloudflared,
            (Section::Updates, 'l') => Operation::InstallRclone,
            (Section::Updates, 'i') => Operation::Update {
                version: "latest".to_owned(),
                name: "elzevir".to_owned(),
            },
            _ => return false,
        };
        if matches!(operation, Operation::TestTunnel) {
            self.diagnostics.requested = Some(ObservationRequest::Operate(operation));
            self.diagnostics.pending = true;
        } else {
            self.form = Some(Form::new(operation));
        }
        true
    }

    fn handle_form(&mut self, event: &Event) -> bool {
        let Event::Key(key) = event else {
            return true;
        };
        if key.kind == KeyEventKind::Release {
            return true;
        }
        if key.code == KeyCode::Esc
            || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
        {
            self.form = None;
            return true;
        }
        let Some(form) = &mut self.form else {
            return false;
        };
        if form.confirming() {
            if key.code == KeyCode::Char('y') {
                self.diagnostics.requested = self
                    .form
                    .take()
                    .and_then(Form::finish)
                    .map(ObservationRequest::Operate);
                self.diagnostics.pending = self.diagnostics.requested.is_some();
            }
            return true;
        }
        match key.code {
            KeyCode::Enter | KeyCode::Tab => form.field += 1,
            KeyCode::BackTab => form.field = form.field.saturating_sub(1),
            KeyCode::Backspace => {
                if let Some(value) = form.values.get_mut(form.field) {
                    value.pop();
                }
            }
            KeyCode::Char(c)
                if !c.is_control()
                    && !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                if let Some(value) = form.values.get_mut(form.field)
                    && value.chars().count() < 512
                {
                    value.push(c);
                }
            }
            _ => {}
        }
        true
    }
}

fn backup_operation(key: char, observed: Option<&Operations>) -> Operation {
    match key {
        'n' => Operation::BackupNow,
        'v' => Operation::VerifyLatest,
        'e' => Operation::ExportLatest {
            output: String::new(),
        },
        'k' => Operation::ExportKey {
            output: String::new(),
        },
        'o' => Operation::Restore {
            input: String::new(),
            key: String::new(),
        },
        _ => Operation::Policy {
            keep: observed.and_then(|ops| ops.keep).unwrap_or(7).to_string(),
            time: observed
                .and_then(|ops| ops.daily.as_deref())
                .and_then(|daily| daily.split_once(' '))
                .map_or_else(String::new, |(time, _)| time.to_owned()),
            zone: observed
                .and_then(|ops| ops.daily.as_deref())
                .and_then(|daily| daily.split_once(' '))
                .map_or_else(|| "UTC".to_owned(), |(_, zone)| zone.to_owned()),
        },
    }
}
