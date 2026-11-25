// SPDX-License-Identifier: GPL-3.0-or-later
//! Terminal administration interface.

mod access_actions;
mod address_operations;
mod address_view;
pub use address_operations::AddressOperation;
mod addresses;
mod backup_options;
mod cloud;
mod dashboard;
mod domain;
pub use backup_options::{BackupOptions, BackupProfile, BackupScope};
pub use cloud::{
    CloudConnection, CloudOperation, CloudPlan, CloudQuestion, CloudTarget, CloudView,
    RemoteCommand, RemotePage,
};
pub use domain::{ConnectorState, DomainPhase, DomainStatus};
mod diagnostic_view;
mod diagnostics;
mod live;
mod messages;
mod operation_view;
mod operations;
mod overview;
mod port;
pub use port::{MIN_PORT, PortForm, PortStep, Refusal, choose_port};
mod folder_picker;
mod restore_paths;
mod runtime;
mod service_view;
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
pub use operations::{Form, Job, LocalBackup, Operation, Operations, RestoreReview, ServerAction};
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
    /// Institution preferences and initial setup.
    Settings,
}

impl Section {
    /// Navigation order.
    pub const ALL: [Self; 7] = [
        Self::Dashboard,
        Self::Diagnostics,
        Self::Addresses,
        Self::Backup,
        Self::Updates,
        Self::Restart,
        Self::Settings,
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
            Self::Settings => "section-settings",
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
            Self::Settings => 'g',
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
    /// Cloud backup tab (connections, plans, files or history).
    pub cloud_tab: usize,
    /// Index of the plan whose connection and folder are displayed.
    pub cloud_plan: usize,
    /// Identity and destination of the selected remote backup.
    pub cloud_file: Option<(CloudTarget, String)>,
    /// Reviewed destinations marked for a single upload.
    pub cloud_targets: Vec<CloudTarget>,
    /// Most recent desktop or clipboard action feedback.
    pub access_notice: Option<&'static str>,
    /// Available terminal-host actions.
    pub access_help: Option<&'static str>,
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
    fn sharing_catalog(&self) -> bool {
        self.section == Section::Addresses
            && self.tunnel_tab == TunnelTab::Share
            && self.form.is_none()
            && matches!(self.mode, Mode::Live(_))
    }

    fn size_warning(&self, width: u16, height: u16) -> bool {
        view::too_small(width, height) && !self.sharing_catalog()
    }

    fn handle_size_warning(&mut self, event: &Event) {
        if let Event::Key(key) = event
            && key.kind == KeyEventKind::Press
        {
            match key.code {
                KeyCode::Char('q') => self.quit = true,
                KeyCode::Char('Q') if self.live_addresses() => {
                    self.tunnel_tab = TunnelTab::Share;
                }
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.quit = true;
                }
                _ => {}
            }
        }
    }

    /// Applies an input event using the same geometry as the renderer.
    pub fn handle(&mut self, event: &Event, width: u16, height: u16) {
        if self.size_warning(width, height) {
            if self.form.is_none() {
                self.handle_size_warning(event);
            }
            return;
        }
        if self.form.is_some() && self.handle_form(event) {
            return;
        }
        if self.section == Section::Addresses && self.tunnel_tab == TunnelTab::Share {
            if let Event::Key(key) = event
                && key.kind != KeyEventKind::Release
            {
                match key.code {
                    KeyCode::Char('Q') | KeyCode::Esc => self.tunnel_tab = TunnelTab::Addresses,
                    KeyCode::Char('q') => self.quit = true,
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        self.quit = true;
                    }
                    _ => {}
                }
            }
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
    /// Whether the selected institution runs under a service manager.
    #[must_use]
    pub fn managed(&self) -> bool {
        matches!(&self.mode, Mode::Live(state) if state.institution.managed)
    }

    /// The settings form, filled with the values read when the panel opened;
    /// only an institution with a configuration file has any.
    fn settings(&self) -> Option<Operation> {
        let Mode::Live(state) = &self.mode else {
            return None;
        };
        let institution = &state.institution;
        Some(Operation::Settings {
            name: institution.name.clone().unwrap_or_default(),
            listen: institution.configured_listen?.to_string(),
            language: institution.language.clone().unwrap_or_default(),
        })
    }

    fn remote_page(&self) -> Option<&RemotePage> {
        let view = self.cloud_view()?;
        let plan = view
            .plans
            .get(self.cloud_plan)
            .or_else(|| view.plans.first())?;
        view.remote.as_ref().filter(|r| r.target == plan.target())
    }

    fn initialize_cloud_file(&mut self) {
        if self.cloud_file.is_none()
            && let Some(r) = self.remote_page()
            && let Some((id, _)) = r.files.first()
        {
            self.cloud_file = Some((r.target.clone(), id.clone()));
        }
    }

    fn step_cloud_file(&mut self, forward: bool) {
        let Some(r) = self.remote_page() else { return };
        let current = r.selected_index(self.cloud_file.as_ref());
        let index = current.map_or_else(
            || {
                if forward {
                    0
                } else {
                    r.files.len().saturating_sub(1)
                }
            },
            |i| cycle(i, r.files.len(), forward),
        );
        if let Some((id, _)) = r.files.get(index) {
            self.cloud_file = Some((r.target.clone(), id.clone()));
        }
    }

    fn cloud_navigation(&mut self, key: char) -> bool {
        self.initialize_cloud_file();
        match key {
            '[' | ']' => {
                self.cloud_tab = (self.cloud_tab + if key == ']' { 1 } else { 3 }) % 4;
            }
            ',' | '.' => {
                self.step_cloud_file(key == '.');
            }
            '<' | '>' => {
                self.cloud_file = None;
                let count = self.cloud_view().map_or(0, |v| v.plans.len()).max(1);
                self.cloud_plan = cycle(self.cloud_plan, count, key == '>');
            }
            'l' | 'D' | 'U' | 'O' | 'u' | '+' | 'M' => {
                let Mode::Live(state) = &self.mode else {
                    return false;
                };
                let Some(snapshot) = &state.snapshot else {
                    return false;
                };
                let Some(view) = &snapshot.operations.cloud else {
                    return true;
                };
                let Some(plan) = view
                    .plans
                    .get(self.cloud_plan)
                    .or_else(|| view.plans.first())
                else {
                    return true;
                };
                let target = plan.target();
                if key == 'O' {
                    if let Some((download, path)) = &view.download
                        && download == &target
                    {
                        self.form = Some(Form::new(Operation::Restore {
                            input: path.to_string_lossy().into_owned(),
                            key: String::new(),
                        }));
                    }
                    return true;
                }
                if key == '+' {
                    if self.cloud_targets.contains(&target) {
                        self.cloud_targets.retain(|t| t != &target);
                    } else if self.cloud_targets.len() < 16 {
                        self.cloud_targets.push(target);
                    }
                    return true;
                }
                if key == 'M' {
                    if !self.cloud_targets.is_empty() {
                        self.form = Some(Form::new(Operation::Cloud(CloudOperation::SendGroup(
                            self.cloud_targets.clone(),
                            snapshot
                                .operations
                                .managed_ids
                                .last()
                                .cloned()
                                .unwrap_or_default(),
                        ))));
                    }
                    return true;
                }
                let listing = view.remote.as_ref().filter(|r| r.target == target);
                let cmd = match key {
                    'l' => RemoteCommand::Browse(listing.map_or(1, |r| r.page)),
                    'D' => {
                        let Some((id, _)) = listing
                            .and_then(|r| r.files.get(r.selected_index(self.cloud_file.as_ref())?))
                        else {
                            self.cloud_tab = 2;
                            self.scroll = 0;
                            return true;
                        };
                        RemoteCommand::Download(id.clone())
                    }
                    'u' => RemoteCommand::Create,
                    _ => RemoteCommand::Send(
                        view.transfers
                            .iter()
                            .find(|(t, _, s)| *t == target && s != "cloud-verified")
                            .map(|(_, id, _)| id)
                            .or_else(|| snapshot.operations.managed_ids.last())
                            .cloned()
                            .unwrap_or_default(),
                    ),
                };
                self.form = Some(Form::new(Operation::Cloud(CloudOperation::Remote(
                    target, cmd,
                ))));
            }
            _ => return false,
        }
        self.scroll = 0;
        true
    }
    fn connector_action_available(&self, key: char) -> bool {
        if !matches!(key, 'P' | 'S') {
            return true;
        }
        let Mode::Live(state) = &self.mode else {
            return false;
        };
        let observed = state
            .snapshot
            .as_ref()
            .filter(|_| state.age_seconds <= 5)
            .and_then(|s| s.operations.domain.as_ref())
            .map(|d| d.connector);
        matches!(
            (key, observed),
            ('P', Some(ConnectorState::Running)) | ('S', Some(ConnectorState::Stopped))
        )
    }
    fn address_navigation(&mut self, key: char) -> bool {
        match key {
            key @ ('[' | ']') => {
                use TunnelTab::{Addresses, Log, Manage, Test};
                let tabs = [Addresses, Manage, Test, Log];
                let i = tabs
                    .iter()
                    .position(|tab| *tab == self.tunnel_tab)
                    .unwrap_or(0);
                self.tunnel_tab = tabs
                    .get(cycle(i, tabs.len(), key == ']'))
                    .copied()
                    .unwrap_or(Addresses);
            }
            'Q' => {
                self.access_notice = None;
                self.tunnel_tab = if self.tunnel_tab == TunnelTab::Share {
                    TunnelTab::Addresses
                } else {
                    TunnelTab::Share
                };
            }
            'm' => self.tunnel_tab = TunnelTab::Manage,
            'h' => {
                self.tunnel_tab = TunnelTab::Log;
            }
            _ => return false,
        }
        self.scroll = 0;
        true
    }
    fn begin_operation(&mut self, key: char) -> bool {
        if !matches!(&self.mode, Mode::Live(_)) || self.palette || self.diagnostics.pending {
            return false;
        }
        if (self.section == Section::Addresses && self.address_navigation(key))
            || (self.section == Section::Backup && self.cloud_navigation(key))
        {
            return true;
        }
        let operation = match (self.section, key) {
            (Section::Addresses, 'T') => {
                self.tunnel_tab = TunnelTab::Test;
                Operation::TestStaff
            }
            (Section::Addresses, 't') => {
                self.tunnel_tab = TunnelTab::Test;
                Operation::TestTunnel
            }
            (Section::Addresses, key) if address_operations::shortcut(key).is_some() => {
                if !self.connector_action_available(key) {
                    self.access_notice = Some("address-connector-unavailable");
                    self.scroll = 0;
                    return true;
                }
                self.tunnel_tab = TunnelTab::Manage;
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
            (Section::Backup, 'R') => {
                let Mode::Live(state) = &self.mode else {
                    return false;
                };
                let Some(review) = state
                    .snapshot
                    .as_ref()
                    .and_then(|s| s.operations.review.clone())
                else {
                    return false;
                };
                Operation::ApplyRestore(review)
            }
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
            (Section::Restart | Section::Settings, 'c') => match self.settings() {
                Some(operation) => operation,
                None => return false,
            },
            (Section::Restart, 'i') if !self.managed() => Operation::Server(ServerAction::Start),
            (Section::Restart, 'p') if !self.managed() => Operation::Server(ServerAction::Stop),
            (Section::Restart, 't') if !self.managed() => Operation::Server(ServerAction::Restart),
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
        if matches!(operation, Operation::TestTunnel | Operation::TestStaff) {
            self.diagnostics.requested = Some(ObservationRequest::Operate(operation));
            self.diagnostics.pending = true;
        } else {
            self.form = Some(Form::new(operation));
        }
        true
    }

    /// Confirms the current form using the same validation as its primary button.
    pub fn submit_form(&mut self) {
        if let Some(Operation::Cloud(CloudOperation::Remote(target, RemoteCommand::Download(id)))) =
            self.form.as_ref().map(|form| &form.operation)
            && !self
                .remote_page()
                .is_some_and(|r| r.target == *target && r.files.iter().any(|(file, _)| file == id))
        {
            self.form = None;
            self.cloud_tab = 2;
            self.scroll = 0;
            return;
        }
        if let Some(form) = &mut self.form {
            if matches!(form.operation, Operation::Restore { .. })
                && form
                    .values
                    .iter()
                    .zip(&form.checks)
                    .any(|(v, c)| c.resolved(v).is_none())
            {
                form.notice = Some("restore-check-paths");
                return;
            }
            form.field = form.labels.len();
        }
        self.diagnostics.requested = self
            .form
            .take()
            .and_then(Form::finish)
            .map(ObservationRequest::Operate);
        self.diagnostics.pending = self.diagnostics.requested.is_some();
    }

    fn handle_form(&mut self, event: &Event) -> bool {
        if let Event::Paste(value) = event {
            if let Some(form) = &mut self.form {
                form.paste(value);
            }
            return true;
        }
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
        if key.code == KeyCode::F(2) && form.open_broker_settings() {
            return true;
        }
        if let Operation::Cloud(CloudOperation::SendGroup(targets, _)) = &form.operation
            && matches!(key.code, KeyCode::PageUp | KeyCode::PageDown)
        {
            let count = targets.len().max(1);
            form.target_page = cycle(form.target_page, count, key.code == KeyCode::PageDown);
            return true;
        }
        if matches!(form.operation, Operation::ApplyRestore(_)) && key.code == KeyCode::Enter {
            self.submit_form();
            return true;
        }
        if matches!(form.operation, Operation::Restore { .. })
            && let Some(check) = form.checks.get_mut(form.field)
        {
            if matches!(key.code, KeyCode::F(7 | 8)) {
                check.choose(key.code == KeyCode::F(8));
                return true;
            }
            form.notice = None;
        }
        if form.confirming() {
            if key.code == KeyCode::Char('y') {
                self.submit_form();
            } else if key.code == KeyCode::BackTab {
                form.field = form.field.saturating_sub(1);
            }
            return true;
        }
        let label = form.labels.get(form.field).copied().unwrap_or_default();
        if let Some(choices) = operations::choices(label) {
            // A choice field changes with the arrows; typing never edits it.
            if let (KeyCode::Left | KeyCode::Right, Some(value)) =
                (key.code, form.values.get_mut(form.field))
            {
                let current = choices.iter().position(|(choice, _)| choice == value);
                let next = match (key.code, current) {
                    (KeyCode::Left, Some(index)) => {
                        index.checked_sub(1).unwrap_or(choices.len() - 1)
                    }
                    (_, Some(index)) => (index + 1) % choices.len(),
                    (_, None) => 0,
                };
                if let Some((choice, _)) = choices.get(next) {
                    (*choice).clone_into(value);
                }
            } else if matches!(key.code, KeyCode::Enter | KeyCode::Tab) {
                form.field += 1;
            } else if key.code == KeyCode::BackTab {
                form.field = form.field.saturating_sub(1);
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

fn cycle(current: usize, count: usize, forward: bool) -> usize {
    (current % count + if forward { 1 } else { count - 1 }) % count
}

fn backup_operation(key: char, observed: Option<&Operations>) -> Operation {
    match key {
        'n' => Operation::BackupNow {
            options: BackupOptions::default(),
            folder: String::new(),
        },
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
