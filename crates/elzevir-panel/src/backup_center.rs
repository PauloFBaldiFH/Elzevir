// SPDX-License-Identifier: GPL-3.0-or-later
//! Read-only catalogue selection and operator actions on a specific archive.
use crate::{
    BackupArtifact, Form, LocalBackup, Messages, Mode, Operation, Operations, Panel, Section,
    ServerStatus, Snapshot,
    access_actions::Actions,
    dashboard::text,
    ui::{self, Action, Availability, Button, Scene, Variant, theme},
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::Paragraph,
};
use std::path::PathBuf;

/// Workspace visible in the Backups section.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum BackupPage {
    /// Protection status and retained copies.
    #[default]
    Overview,
    /// Backup, export and verification requests.
    History,
    /// Progress and evidence for one backup.
    Result,
    /// Local and cloud backup schedules.
    Schedules,
    /// Searchable catalogue of installed rclone providers.
    Providers,
    /// Cloud management workspace.
    Connections,
}

/// Selection survives polling and is independent of catalogue ordering.
#[derive(Debug, Default)]
pub struct BackupCenter {
    /// Current local or cloud view.
    pub page: BackupPage,
    pub(crate) selected: Option<u128>,
    pub(crate) offset: usize,
    pub(crate) schedule: Option<crate::CloudTarget>,
    pub(crate) provider_query: String,
    pub(crate) waiting_ack: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Command {
    Page(BackupPage),
    Select(u128),
    CloudTab(usize),
    Provider(crate::provider_picker::Name),
    ClearFilter,
    SelectSchedule(u128),
    RunSchedule(u128),
    Details(u128),
    Verify(u128),
    Export(u128),
    Copy(u128),
    OpenFolder(u128),
    Cloud(u128),
    Plan(bool),
    Scroll(bool),
}

pub(super) fn sample(panel: &Panel) -> Option<&Snapshot> {
    let Mode::Live(live) = &panel.mode else {
        return None;
    };
    live.snapshot.as_ref().filter(|_| live.age_seconds <= 5)
}
fn entry(ops: &Operations, id: u128) -> Option<&BackupArtifact> {
    let name = format!("{id:032x}");
    ops.artifacts.iter().find(|a| a.id == name)
}
pub(crate) fn path(ops: &Operations, id: u128) -> Option<PathBuf> {
    entry(ops, id)?.bytes?;
    Some(
        ops.backup_folder
            .as_ref()?
            .join(format!("elzevir-{id:032x}.age")),
    )
}
pub(crate) fn selected(panel: &Panel, ops: &Operations) -> Option<u128> {
    if panel.backups.page == BackupPage::Result && panel.backups.waiting_ack {
        return None;
    }
    panel.backups.selected.or_else(|| {
        if panel.backups.page == BackupPage::Result {
            ops.manual_job.as_deref()
        } else {
            ops.artifacts.first().map(|a| a.id.as_str())
        }
        .and_then(|id| u128::from_str_radix(id, 16).ok())
    })
}
pub(crate) fn shortcut(panel: &Panel, key: char) -> Option<Operation> {
    let snapshot = sample(panel)?;
    let id = selected(panel, &snapshot.operations)?;
    path(&snapshot.operations, id)?;
    match key {
        'v' if matches!(
            snapshot.status,
            ServerStatus::Running(_) | ServerStatus::Degraded(_)
        ) =>
        {
            Some(Operation::VerifyArtifact(format!("{id:032x}")))
        }
        'e' => Some(Operation::ExportArtifact {
            id: format!("{id:032x}"),
            folder: String::new(),
        }),
        _ => None,
    }
}
pub(crate) fn scroll(panel: &mut Panel, down: bool, by: usize, limit: u16) {
    let start = panel.backups.offset.min(usize::from(limit));
    panel.backups.offset = if down {
        start.saturating_add(by).min(usize::from(limit))
    } else {
        start.saturating_sub(by)
    };
}
pub(crate) fn scrollable(panel: &Panel) -> bool {
    panel.section == Section::Backup
        && matches!(panel.mode, Mode::Live(_))
        && panel.inspection.is_none()
        && matches!(
            panel.backups.page,
            BackupPage::Overview
                | BackupPage::History
                | BackupPage::Providers
                | BackupPage::Schedules
                | BackupPage::Connections
        )
}
pub(crate) fn apply(cmd: Command, panel: &mut Panel, access: &mut Actions, limit: u16) {
    let ops = sample(panel).map(|s| &s.operations);
    match cmd {
        Command::Page(page) => {
            panel.backups.page = page;
            panel.backups.offset = 0;
            panel.access_notice = None;
        }
        Command::Provider(name) => crate::provider_picker::choose(panel, name),
        Command::ClearFilter => {
            panel.backups.provider_query.clear();
            panel.backups.offset = 0;
        }
        Command::CloudTab(tab) => {
            panel.cloud_tab = tab;
            panel.backups.offset = 0;
            panel.access_notice = None;
        }
        Command::SelectSchedule(id) => {
            panel.backups.schedule = ops
                .and_then(|o| o.cloud.as_ref())
                .and_then(|v| {
                    v.plans
                        .iter()
                        .find(|p| u128::from_str_radix(&p.id, 16) == Ok(id))
                })
                .map(crate::CloudPlan::target);
        }
        Command::RunSchedule(id) => crate::schedule_view::review(panel, id),
        Command::Scroll(down) => scroll(panel, down, 1, limit),
        Command::Select(id) | Command::Details(id) => {
            if ops.and_then(|o| entry(o, id)).is_some() {
                panel.backups.selected = Some(id);
                if matches!(cmd, Command::Details(_)) {
                    panel.backups.page = BackupPage::Result;
                }
                panel.access_notice = None;
            }
        }
        Command::Plan(forward) => {
            if let Some(count) = ops
                .and_then(|o| o.cloud.as_ref())
                .map(|v| v.plans.len())
                .filter(|n| *n > 0)
            {
                panel.cloud_plan = (panel.cloud_plan + if forward { 1 } else { count - 1 }) % count;
            }
        }
        Command::Verify(id)
        | Command::Export(id)
        | Command::Cloud(id)
        | Command::Copy(id)
        | Command::OpenFolder(id) => {
            let Some(ops) = ops else { return };
            let Some(file) = path(ops, id) else { return };
            if matches!(cmd, Command::Copy(_) | Command::OpenFolder(_)) {
                panel.access_notice = Some(if matches!(cmd, Command::Copy(_)) {
                    if file.to_str().is_some_and(|p| access.copy(p)) {
                        "address-copy-requested"
                    } else {
                        "transfer-select"
                    }
                } else {
                    access.open(file.parent().map(std::path::Path::as_os_str))
                });
                return;
            }
            if panel.diagnostics.pending {
                return;
            }
            let id = format!("{id:032x}");
            let operation = match cmd {
                Command::Verify(_) => Operation::VerifyArtifact(id),
                Command::Cloud(_) => {
                    let Some(plan) = ops
                        .cloud
                        .as_ref()
                        .and_then(|c| c.plans.get(panel.cloud_plan))
                    else {
                        return;
                    };
                    Operation::Cloud(crate::CloudOperation::Remote(
                        plan.target(),
                        crate::RemoteCommand::Send(id),
                    ))
                }
                _ => Operation::ExportArtifact {
                    id,
                    folder: String::new(),
                },
            };
            panel.form = Some(Form::new(operation));
            panel.access_notice = None;
        }
    }
}

pub(super) fn row(r: Rect, n: u16, height: u16) -> Rect {
    Rect::new(
        r.x,
        r.y + n,
        r.width,
        height.min(r.height.saturating_sub(n)),
    )
}
pub(super) fn inner(r: Rect) -> Rect {
    Rect::new(r.x + 1, r.y, r.width.saturating_sub(2), r.height)
}
pub(super) fn columns(r: Rect, count: usize) -> Vec<Rect> {
    Layout::horizontal((0..count).map(|_| Constraint::Fill(1)))
        .spacing(1)
        .split(r)
        .to_vec()
}
pub(super) fn line(f: &mut Frame, r: Rect, s: impl Into<String>, color: Color, bold: bool) {
    let st = Style::new().fg(color);
    f.render_widget(
        Paragraph::new(s.into()).style(if bold {
            st.add_modifier(Modifier::BOLD)
        } else {
            st
        }),
        row(r, 0, 1),
    );
}
pub(super) fn block(f: &mut Frame, r: Rect, title: String) {
    ui::fill(f, r, theme::surface());
    ui::fill(f, row(r, 0, 1), theme::surface().bg(theme::STEEL));
    line(f, inner(r), title, theme::TEXT, true);
}
pub(super) struct Controls<'a, 'f> {
    pub f: &'a mut Frame<'f>,
    pub scene: &'a mut Scene,
    pub m: &'a Messages,
}
impl Controls<'_, '_> {
    pub(super) fn buttons(&mut self, r: Rect, items: &[(&str, Action, Variant, bool)]) {
        for ((key, action, variant, enabled), slot) in items.iter().zip(columns(r, items.len())) {
            Button::new(self.m.get(key), *action, *variant)
                .available(if *enabled {
                    Availability::Enabled
                } else {
                    Availability::Disabled
                })
                .draw(self.f, slot, self.scene);
        }
    }
}

pub(super) fn key(c: char) -> Action {
    Action::Key(ratatui::crossterm::event::KeyCode::Char(c))
}
pub(super) fn go(page: BackupPage) -> Action {
    Action::Backup(Command::Page(page))
}
pub(super) fn bytes(size: u64) -> String {
    if size >= 1 << 30 {
        format!("{}.{:01} GiB", size >> 30, ((size % (1 << 30)) * 10) >> 30)
    } else if size >= 1 << 20 {
        format!("{}.{:01} MiB", size >> 20, ((size % (1 << 20)) * 10) >> 20)
    } else if size >= 1024 {
        format!("{} KiB", size >> 10)
    } else {
        format!("{size} B")
    }
}

pub(crate) fn draw(f: &mut Frame, panel: &Panel, m: &Messages, scene: &mut Scene) {
    let r = inner(f.area());
    let desktop = r.width >= 104 && r.height >= 35;
    let obs = sample(panel);
    let ops = obs.map(|s| &s.operations);
    let online = obs.is_some_and(|s| {
        matches!(
            s.status,
            ServerStatus::Running(_) | ServerStatus::Degraded(_)
        )
    });
    let h = if desktop { 3 } else { 1 };
    scene.scroll_limit = 0;
    let mut ui = Controls { f, scene, m };
    line(
        ui.f,
        r,
        format!("ELZEVIR   /   {}", m.get("desk-admin")),
        theme::WARNING,
        true,
    );
    navigation(&mut ui, row(r, 1, h), panel.backups.page);
    let title_y = 2 + h;
    line(
        ui.f,
        row(r, title_y, 1),
        m.get(match panel.backups.page {
            BackupPage::History => "bc-history",
            BackupPage::Result => "bc-result",
            BackupPage::Schedules => "bc-schedule",
            BackupPage::Connections => "bc-cloud",
            BackupPage::Overview => "bc-center",
            BackupPage::Providers => "cc-choose",
        }),
        theme::TEXT,
        true,
    );
    let top = title_y + 2;
    let buttons_h = if desktop { 3 } else { 2 };
    let bottom = r.height - 2 - buttons_h;
    let work = row(r, top, bottom.saturating_sub(top + 1));
    match panel.backups.page {
        BackupPage::Overview => overview(&mut ui, work, panel, obs),
        BackupPage::History => history(&mut ui, work, panel, ops),
        BackupPage::Result => result(&mut ui, work, panel, ops),
        BackupPage::Schedules => crate::schedule_view::draw(&mut ui, work, panel, ops),
        BackupPage::Providers => crate::provider_picker::draw(&mut ui, work, panel, ops),
        BackupPage::Connections => crate::cloud_view::draw(&mut ui, work, panel, obs),
    }
    footer(&mut ui, row(r, bottom, buttons_h), panel, ops, online);
    line(
        ui.f,
        row(r, r.height - 1, 1),
        m.get(
            panel
                .access_notice
                .or_else(|| {
                    ops.and_then(|o| o.result).filter(|key| {
                        panel.backups.page != BackupPage::Schedules || *key != "op-submitted"
                    })
                })
                .unwrap_or("desk-help"),
        ),
        theme::MUTED,
        false,
    );
}
fn footer(
    ui: &mut Controls<'_, '_>,
    area: Rect,
    panel: &Panel,
    ops: Option<&Operations>,
    online: bool,
) {
    let back = (
        "bc-return",
        go(BackupPage::Overview),
        Variant::Secondary,
        true,
    );
    if panel.backups.page == BackupPage::Providers {
        ui.buttons(
            area,
            &[(
                "cc-back",
                go(BackupPage::Connections),
                Variant::Secondary,
                true,
            )],
        );
    } else if panel.backups.page == BackupPage::Connections {
        ui.buttons(
            area,
            &[
                (
                    "bc-schedule",
                    go(BackupPage::Schedules),
                    Variant::Secondary,
                    true,
                ),
                back,
            ],
        );
    } else if panel.backups.page == BackupPage::Schedules {
        ui.buttons(
            area,
            &[
                (
                    "schedule-edit",
                    key('s'),
                    Variant::Primary,
                    online && ops.is_some_and(|o| o.keep.is_some()) && !panel.diagnostics.pending,
                ),
                (
                    "bc-cloud",
                    go(BackupPage::Connections),
                    Variant::Secondary,
                    true,
                ),
                back,
            ],
        );
    } else if panel.backups.page == BackupPage::Result {
        ui.buttons(
            area,
            &[
                (
                    "bc-history",
                    go(BackupPage::History),
                    Variant::Secondary,
                    true,
                ),
                back,
            ],
        );
    } else {
        let busy = panel.diagnostics.pending
            || ops.is_some_and(|o| {
                o.jobs.iter().any(|j| {
                    matches!(j.action, "op-backup-now" | "cloud-backup")
                        && matches!(j.state, "op-queued" | "op-running")
                })
            });
        ui.buttons(
            area,
            &[
                ("desk-create", key('n'), Variant::Primary, online && !busy),
                (
                    "desk-restore",
                    key('o'),
                    Variant::Secondary,
                    ops.is_some() && !panel.diagnostics.pending,
                ),
                (
                    "bc-schedule",
                    go(BackupPage::Schedules),
                    Variant::Secondary,
                    true,
                ),
                (
                    "bc-key",
                    key('k'),
                    Variant::Secondary,
                    ops.is_some() && !panel.diagnostics.pending,
                ),
            ],
        );
    }
}
fn navigation(ui: &mut Controls<'_, '_>, area: Rect, page: BackupPage) {
    let active = |candidate| {
        if page == candidate {
            Variant::Primary
        } else {
            Variant::Secondary
        }
    };
    let nav = [
        (
            "desk-overview",
            Action::Section(Section::Dashboard),
            Variant::Secondary,
            true,
        ),
        (
            "desk-backups",
            go(BackupPage::Overview),
            active(BackupPage::Overview),
            true,
        ),
        (
            "bc-history",
            go(BackupPage::History),
            active(BackupPage::History),
            true,
        ),
        (
            "bc-cloud",
            go(BackupPage::Connections),
            active(BackupPage::Connections),
            true,
        ),
    ];
    ui.buttons(area, &nav);
}
fn overview(ui: &mut Controls<'_, '_>, r: Rect, panel: &Panel, s: Option<&Snapshot>) {
    let ops = s.map(|s| &s.operations);
    summary(ui, r, ops);
    let body = row(r, 4, r.height.saturating_sub(4));
    let wide = body.width >= 100;
    let regions = if wide {
        Layout::horizontal([Constraint::Percentage(62), Constraint::Fill(1)])
            .spacing(1)
            .areas(body)
    } else {
        Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(if body.height < 12 { 3 } else { 5 }),
        ])
        .spacing(1)
        .areas(body)
    };
    let [table, detail] = regions;
    let picked = ops.and_then(|o| selected(panel, o));
    catalogue(ui, table, panel, ops, picked);
    details(ui, detail, s, picked);
}
fn summary(ui: &mut Controls<'_, '_>, r: Rect, ops: Option<&Operations>) {
    let m = ui.m;
    let tiles = columns(row(r, 0, 3), 3);
    let local = match ops.map(|o| o.local_backup).unwrap_or_default() {
        LocalBackup::Verified => ("desk-verified", theme::SUCCESS),
        LocalBackup::Empty => ("desk-empty", theme::WARNING),
        LocalBackup::Unknown => ("panel-unknown", theme::MUTED),
    };
    let remote = ops
        .and_then(|o| o.cloud.as_ref())
        .map_or("panel-unknown", |c| {
            c.transfers
                .first()
                .map_or("home-no-transfer", |(_, _, status)| status)
        });
    let policy = ops.and_then(|o| o.daily.clone()).unwrap_or_else(|| {
        m.get(if ops.is_some_and(|o| o.keep.is_some()) {
            "home-unscheduled"
        } else {
            "panel-unknown"
        })
    });
    for ((title, value, color), tile) in [
        ("bc-local-result", m.get(local.0), local.1),
        ("bc-cloud", m.get(remote), theme::TEXT),
        ("bc-schedule", policy, theme::TEXT),
    ]
    .into_iter()
    .zip(tiles)
    {
        block(ui.f, tile, m.get(title));
        line(ui.f, row(inner(tile), 1, 1), value, color, true);
    }
}

fn catalogue(
    ui: &mut Controls<'_, '_>,
    table: Rect,
    panel: &Panel,
    ops: Option<&Operations>,
    picked: Option<u128>,
) {
    let m = ui.m;
    let entries = ops.map_or(&[][..], |o| o.artifacts.as_slice());
    block(ui.f, table, m.get("bc-copies"));
    let t = inner(table);
    let page = usize::from(table.height.saturating_sub(4)).max(1);
    let start = panel.backups.offset.min(entries.len().saturating_sub(page));
    if entries.is_empty() {
        line(
            ui.f,
            row(t, 1, 1),
            m.get(
                if ops.is_some_and(|o| o.local_backup == LocalBackup::Empty) {
                    "desk-empty"
                } else {
                    "home-unknown-backup"
                },
            ),
            theme::MUTED,
            false,
        );
    } else {
        line(ui.f, row(t, 1, 1), m.get("bc-columns"), theme::MUTED, true);
    }
    for (artifact, y) in entries.iter().skip(start).take(page).zip(2u16..) {
        if y >= table.height.saturating_sub(1) {
            break;
        }
        let Ok(id) = u128::from_str_radix(&artifact.id, 16) else {
            continue;
        };
        let label = format!(
            "{:20} {:>10}   {:22} {}",
            artifact.modified.as_deref().unwrap_or("—"),
            artifact.bytes.map_or_else(|| "—".into(), bytes),
            m.get(if artifact.bytes.is_some() {
                "desk-verified"
            } else {
                "bc-missing"
            }),
            artifact.export.map_or_else(|| "—".into(), |k| m.get(k))
        );
        let slot = row(t, y, 1);
        let action = Action::Backup(Command::Select(id));
        let hover = ui.scene.add(slot, action);
        ui::fill(
            ui.f,
            slot,
            if hover || ui.scene.focus() == Some(action) || Some(id) == picked {
                theme::surface().bg(theme::STEEL)
            } else {
                theme::surface()
            },
        );
        line(
            ui.f,
            slot,
            format!("{} {label}", if Some(id) == picked { ">" } else { " " }),
            theme::TEXT,
            Some(id) == picked,
        );
    }
    if table.height > 2 {
        pager(ui, row(t, table.height - 1, 1), start, entries.len(), page);
    }
}

fn details(
    ui: &mut Controls<'_, '_>,
    detail: Rect,
    snapshot: Option<&Snapshot>,
    picked: Option<u128>,
) {
    let m = ui.m;
    let ops = snapshot.map(|s| &s.operations);
    block(ui.f, detail, m.get("bc-details"));
    let d = inner(detail);
    if let Some(a) = ops.zip(picked).and_then(|(o, id)| entry(o, id)) {
        line(
            ui.f,
            row(d, 1, 1),
            format!("elzevir-{}.age", text(&a.id)),
            theme::ACCENT,
            true,
        );
        if detail.height > 5 {
            line(
                ui.f,
                row(d, 3, 1),
                m.values(
                    "bc-size",
                    &[(
                        "value",
                        a.bytes.map_or_else(|| m.get("panel-unknown"), bytes),
                    )],
                ),
                theme::TEXT,
                false,
            );
            line(
                ui.f,
                row(d, 4, 1),
                m.values(
                    "bc-export-state",
                    &[("value", m.get(a.export.unwrap_or("bc-no-receipt")))],
                ),
                theme::TEXT,
                false,
            );
        }
    } else {
        line(
            ui.f,
            row(d, 1, 1),
            m.get("bc-empty-detail"),
            theme::MUTED,
            false,
        );
    }
    if detail.height >= 9 {
        line(
            ui.f,
            row(d, 6, 1),
            crate::dashboard::row(
                m,
                "home-retain",
                ops.and_then(|o| o.keep).map(|n| n.to_string()),
            )
            .to_string(),
            theme::TEXT,
            false,
        );
        line(
            ui.f,
            row(d, 7, 1),
            crate::dashboard::row(
                m,
                "home-space",
                snapshot.and_then(|s| s.storage.available_bytes).map(bytes),
            )
            .to_string(),
            theme::TEXT,
            false,
        );
        if let Some(folder) = ops.and_then(|o| o.backup_folder.as_ref()) {
            line(
                ui.f,
                row(d, 8, 1),
                text(&folder.to_string_lossy()),
                theme::MUTED,
                false,
            );
        }
    }
    if detail.height >= 15
        && let Some((ops, artifact)) = ops.and_then(|ops| Some((ops, entry(ops, picked?)?)))
    {
        artifact_history(ui, row(d, 10, 4), ops, artifact);
    }
    let h = if detail.height >= 10 { 3 } else { 1 };
    ui.buttons(
        row(d, detail.height.saturating_sub(h), h),
        &[(
            "bc-details",
            Action::Backup(Command::Details(picked.unwrap_or(0))),
            Variant::Secondary,
            picked.is_some(),
        )],
    );
}
fn artifact_history(
    ui: &mut Controls<'_, '_>,
    area: Rect,
    ops: &Operations,
    artifact: &BackupArtifact,
) {
    let m = ui.m;
    let cloud = ops.cloud.as_ref().map_or("panel-unknown", |cloud| {
        cloud
            .transfers
            .iter()
            .find(|(_, id, _)| *id == artifact.id)
            .map_or("home-no-transfer", |(_, _, state)| state)
    });
    for ((key, value), y) in [
        (
            "bc-attempt",
            artifact
                .export_at
                .clone()
                .unwrap_or_else(|| m.get("bc-no-receipt")),
        ),
        (
            "bc-recheck",
            m.get(artifact.recheck.unwrap_or("bc-no-receipt")),
        ),
        ("bc-remote-state", m.get(cloud)),
    ]
    .into_iter()
    .zip(0u16..)
    {
        line(
            ui.f,
            row(area, y, 1),
            m.values(key, &[("value", value)]),
            theme::TEXT,
            false,
        );
    }
}
pub(super) fn pager(ui: &mut Controls<'_, '_>, r: Rect, start: usize, count: usize, page: usize) {
    ui.scene.scroll_limit = u16::try_from(count.saturating_sub(page)).unwrap_or(u16::MAX);
    ui.buttons(
        r,
        &[
            (
                "desk-up",
                Action::Backup(Command::Scroll(false)),
                Variant::Quiet,
                start > 0,
            ),
            (
                "desk-down",
                Action::Backup(Command::Scroll(true)),
                Variant::Quiet,
                start + page < count,
            ),
        ],
    );
}
fn history(ui: &mut Controls<'_, '_>, r: Rect, panel: &Panel, ops: Option<&Operations>) {
    let m = ui.m;
    block(ui.f, r, m.get("bc-requests"));
    let t = inner(r);
    let jobs: Vec<_> = ops
        .into_iter()
        .flat_map(|o| &o.jobs)
        .filter(|j| {
            matches!(
                j.action,
                "op-backup-now" | "op-backup-export" | "op-backup-verify" | "cloud-backup"
            )
        })
        .collect();
    let page = usize::from(r.height.saturating_sub(3));
    let start = panel.backups.offset.min(jobs.len().saturating_sub(page));
    if jobs.is_empty() {
        line(
            ui.f,
            row(t, 1, 1),
            m.get("bc-no-requests"),
            theme::MUTED,
            false,
        );
    }
    for (j, y) in jobs.iter().skip(start).take(page).zip(1u16..) {
        line(
            ui.f,
            row(t, y, 1),
            format!(
                "{:20}  {:28}  {:15}  {}",
                j.at,
                m.get(match j.action {
                    "op-backup-verify" => "bc-verify",
                    "op-backup-export" => "bc-copy-out",
                    other => other,
                }),
                m.get(j.state),
                j.phase.map_or_else(String::new, |p| m.get(p))
            ),
            theme::TEXT,
            false,
        );
    }
    pager(
        ui,
        row(t, t.height.saturating_sub(1), 1),
        start,
        jobs.len(),
        page,
    );
}
fn result(ui: &mut Controls<'_, '_>, r: Rect, panel: &Panel, ops: Option<&Operations>) {
    let picked = ops.and_then(|o| selected(panel, o));
    let id = picked.unwrap_or(0);
    let artifact = ops.and_then(|o| entry(o, id));
    let awaiting = panel.backups.waiting_ack && panel.backups.selected.is_none();
    let verified = !awaiting && artifact.is_some_and(|a| a.bytes.is_some());
    let header_h = if r.height < 19 { 4 } else { 7 };
    result_header(ui, row(r, 0, header_h), panel, ops, id);
    let body = row(
        r,
        header_h + 1,
        r.height.saturating_sub(header_h + 1).min(14),
    );
    let [dest, recover] = Layout::horizontal([Constraint::Percentage(59), Constraint::Fill(1)])
        .spacing(1)
        .areas(body);
    transfer_options(ui, dest, panel, ops, id, verified);
    recovery(ui, recover, panel, id, verified);
}

fn result_header(
    ui: &mut Controls<'_, '_>,
    head: Rect,
    panel: &Panel,
    ops: Option<&Operations>,
    id: u128,
) {
    let m = ui.m;
    let artifact = ops.and_then(|o| entry(o, id));
    let awaiting = panel.backups.waiting_ack && panel.backups.selected.is_none();
    let job = ops.and_then(|o| {
        o.jobs.iter().find(|j| {
            j.id == format!("{id:032x}") && matches!(j.action, "op-backup-now" | "cloud-backup")
        })
    });
    let verified = !awaiting && artifact.is_some_and(|a| a.bytes.is_some());
    let (status, tint) = if awaiting {
        ("op-pending", theme::ACCENT)
    } else if artifact.is_some_and(|a| a.recheck == Some("op-failed")) {
        ("bc-recheck-failed", theme::WARNING)
    } else if verified {
        ("home-verified", theme::SUCCESS)
    } else if artifact.is_some() || panel.backups.selected.is_some() {
        ("bc-missing", theme::WARNING)
    } else if let Some(j) = job {
        (
            j.state,
            if matches!(j.state, "op-failed" | "op-interrupted") {
                theme::WARNING
            } else {
                theme::ACCENT
            },
        )
    } else if panel.diagnostics.pending {
        ("op-pending", theme::ACCENT)
    } else {
        (
            ops.and_then(|o| o.result).unwrap_or("panel-unknown"),
            theme::MUTED,
        )
    };
    block(ui.f, head, m.get("bc-local-result"));
    let inner_head = inner(head);
    line(ui.f, row(inner_head, 1, 1), m.get(status), tint, true);
    if !awaiting && let Some(file) = ops.and_then(|o| path(o, id)) {
        line(
            ui.f,
            row(inner_head, 2, 1),
            text(&file.to_string_lossy()),
            theme::ACCENT,
            false,
        );
        if let Some(a) = artifact {
            line(
                ui.f,
                row(inner_head, 3, 1),
                format!(
                    "{}   {}",
                    a.modified.as_deref().unwrap_or("—"),
                    a.bytes.map_or_else(|| m.get("panel-unknown"), bytes)
                ),
                theme::TEXT,
                false,
            );
        }
    } else if let Some(phase) = job.and_then(|j| j.phase) {
        line(
            ui.f,
            row(inner_head, 2, 1),
            m.get(phase),
            theme::TEXT,
            false,
        );
    }
    if verified && head.height >= 6 {
        line(
            ui.f,
            row(inner_head, 5, 1),
            m.get("bc-verification-note"),
            theme::MUTED,
            false,
        );
    }
}

fn transfer_options(
    ui: &mut Controls<'_, '_>,
    dest: Rect,
    panel: &Panel,
    ops: Option<&Operations>,
    id: u128,
    verified: bool,
) {
    let m = ui.m;
    let artifact = ops.and_then(|o| entry(o, id));
    block(ui.f, dest, m.get("bc-transfer"));
    let d = inner(dest);
    let dormant = !panel.diagnostics.pending;
    let export = if ops.is_some_and(|o| {
        o.manual_export_refused
            && artifact.is_some_and(|a| a.export.is_none() && o.manual_job.as_ref() == Some(&a.id))
    }) {
        "op-failed"
    } else {
        artifact.and_then(|a| a.export).unwrap_or("bc-no-receipt")
    };
    line(
        ui.f,
        row(d, 1, 1),
        m.values("bc-export-state", &[("value", m.get(export))]),
        theme::TEXT,
        false,
    );
    let h = if d.height >= 11 { 2 } else { 1 };
    ui.buttons(
        row(d, 2, h),
        &[
            (
                "bc-copy-path",
                Action::Backup(Command::Copy(id)),
                Variant::Secondary,
                verified,
            ),
            (
                "bc-open-folder",
                Action::Backup(Command::OpenFolder(id)),
                Variant::Secondary,
                verified && ui.scene.desktop,
            ),
        ],
    );
    ui.buttons(
        row(d, 2 + h, h),
        &[
            (
                "bc-copy-out",
                Action::Backup(Command::Export(id)),
                Variant::Primary,
                verified && dormant,
            ),
            ("bc-scp", key('z'), Variant::Secondary, verified),
        ],
    );
    let mut y = 2 + 2 * h;
    if matches!(export, "op-failed" | "op-interrupted") {
        ui.f.render_widget(
            Paragraph::new(m.get("bc-retry-help"))
                .style(Style::new().fg(theme::WARNING))
                .wrap(ratatui::widgets::Wrap { trim: true }),
            row(d, y, 2),
        );
        y += 2;
    }
    cloud_options(ui, d, y, panel, id, verified && dormant, ops);
}
fn cloud_options(
    ui: &mut Controls<'_, '_>,
    d: Rect,
    y: u16,
    panel: &Panel,
    id: u128,
    available: bool,
    ops: Option<&Operations>,
) {
    let m = ui.m;
    let plans = ops
        .and_then(|o| o.cloud.as_ref())
        .map_or(&[][..], |c| c.plans.as_slice());
    if y < d.height {
        let plan = plans.get(panel.cloud_plan);
        line(
            ui.f,
            row(d, y, 1),
            plan.map_or_else(
                || m.get("bc-no-plan"),
                |p| format!("{} · {}", text(&p.name), text(&p.folder)),
            ),
            theme::MUTED,
            false,
        );
        if d.height > y + 1 {
            ui.buttons(
                row(d, y + 1, 1),
                &[
                    (
                        "bc-prev",
                        Action::Backup(Command::Plan(false)),
                        Variant::Quiet,
                        plans.len() > 1,
                    ),
                    (
                        "bc-send",
                        Action::Backup(Command::Cloud(id)),
                        Variant::Secondary,
                        available && plan.is_some(),
                    ),
                    (
                        "bc-next",
                        Action::Backup(Command::Plan(true)),
                        Variant::Quiet,
                        plans.len() > 1,
                    ),
                ],
            );
        }
    }
}
fn recovery(ui: &mut Controls<'_, '_>, area: Rect, panel: &Panel, id: u128, verified: bool) {
    let m = ui.m;
    block(ui.f, area, m.get("bc-recovery"));
    let k = inner(area);
    let h = if k.height >= 11 { 2 } else { 1 };
    let dormant = !panel.diagnostics.pending;
    line(
        ui.f,
        row(k, 1, 1),
        m.get("bc-key-note"),
        theme::WARNING,
        true,
    );
    ui.buttons(
        row(k, 2, h),
        &[("bc-key", key('k'), Variant::Secondary, verified && dormant)],
    );
    ui.buttons(
        row(k, 2 + h, h),
        &[(
            "bc-verify",
            Action::Backup(Command::Verify(id)),
            Variant::Secondary,
            verified
                && dormant
                && sample(panel).is_some_and(|s| {
                    matches!(
                        s.status,
                        ServerStatus::Running(_) | ServerStatus::Degraded(_)
                    )
                }),
        )],
    );
    if let Some(state) = sample(panel)
        .and_then(|s| entry(&s.operations, id))
        .and_then(|a| a.recheck)
    {
        line(
            ui.f,
            row(k, 2 + 2 * h, 1),
            m.values("bc-recheck", &[("value", m.get(state))]),
            theme::TEXT,
            false,
        );
    }
    if k.height >= 9 {
        line(
            ui.f,
            row(k, 8, 1),
            m.get("bc-key-private"),
            theme::MUTED,
            false,
        );
    }
}
