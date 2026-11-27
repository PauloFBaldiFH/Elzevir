// SPDX-License-Identifier: GPL-3.0-or-later
//! Mouse and keyboard access to the existing cloud forms and observations.
use crate::{
    BackupPage, CloudView, Panel, Section, ServerStatus, Snapshot,
    backup_center::{Command, Controls, block, go, inner, key, line, pager, row, sample},
    ui::{Action, Button, Variant, theme},
};
use ratatui::{
    layout::Rect,
    widgets::{Paragraph, Wrap},
};

const TABS: [&str; 4] = [
    "cloud-connections-tab",
    "cloud-plans-tab",
    "cloud-files-tab",
    "cloud-history-tab",
];

pub(super) fn draw(
    ui: &mut Controls<'_, '_>,
    area: Rect,
    panel: &Panel,
    snapshot: Option<&Snapshot>,
) {
    let h = if area.height >= 20 { 3 } else { 1 };
    let tab = panel.cloud_tab;
    let tabs = TABS
        .iter()
        .enumerate()
        .map(|(index, label)| {
            (
                *label,
                Action::Backup(Command::CloudTab(index)),
                if tab == index {
                    Variant::Primary
                } else {
                    Variant::Secondary
                },
                true,
            )
        })
        .collect::<Vec<_>>();
    ui.buttons(row(area, 0, h), &tabs);
    let ops = snapshot.map(|s| &s.operations);
    let view = ops.and_then(|o| o.cloud.as_ref());
    let writable = worker_available(panel);
    toolbar(ui, row(area, h + 1, h), panel, view, writable);
    let body = row(area, 2 * h + 2, area.height.saturating_sub(2 * h + 2));
    block(ui.f, body, ui.m.get(TABS.get(tab).unwrap_or(&TABS[0])));
    let content = inner(body);
    let mut rows = crate::operation_view::rows(snapshot, panel, ui.m);
    if tab == 0 && view.is_some_and(|v| v.providers.is_empty()) {
        rows.insert(0, ui.m.get("cc-discover-help").into());
    }
    let body = row(content, 1, content.height.saturating_sub(3));
    let paragraph = Paragraph::new(rows).wrap(Wrap { trim: false });
    let count = paragraph.line_count(body.width);
    let capacity = usize::from(body.height);
    let start = panel.backups.offset.min(count.saturating_sub(capacity));
    ui.f.render_widget(
        paragraph.scroll((u16::try_from(start).unwrap_or(u16::MAX), 0)),
        body,
    );
    if tab == 0 && view.is_some_and(|v| v.providers.is_empty()) {
        ui.buttons(
            row(content, content.height.saturating_sub(2), 1),
            &[(
                "cc-tools",
                Action::Section(Section::Updates),
                Variant::Quiet,
                true,
            )],
        );
    }
    if count > capacity {
        pager(
            ui,
            row(content, content.height.saturating_sub(1), 1),
            start,
            count,
            capacity,
        );
    }
}

fn toolbar(
    ui: &mut Controls<'_, '_>,
    toolbar: Rect,
    panel: &Panel,
    view: Option<&CloudView>,
    writable: bool,
) {
    let has_plan = view.is_some_and(|v| !v.plans.is_empty());
    match panel.cloud_tab {
        0 => ui.buttons(
            toolbar,
            &[
                ("cc-providers", key('p'), Variant::Primary, writable),
                (
                    "cc-new",
                    go(BackupPage::Providers),
                    Variant::Secondary,
                    writable && view.is_some_and(|v| !v.providers.is_empty()),
                ),
                (
                    "cc-answer",
                    key('a'),
                    Variant::Secondary,
                    writable && view.is_some_and(|v| v.question.is_some()),
                ),
            ],
        ),
        1 => ui.buttons(
            toolbar,
            &[
                (
                    "cc-new-plan",
                    key('g'),
                    Variant::Secondary,
                    writable && view.is_some_and(|v| v.connections.iter().any(|c| c.ready)),
                ),
                (
                    "bc-prev",
                    key('<'),
                    Variant::Quiet,
                    view.is_some_and(|v| v.plans.len() > 1),
                ),
                (
                    "bc-next",
                    key('>'),
                    Variant::Quiet,
                    view.is_some_and(|v| v.plans.len() > 1),
                ),
                (
                    "schedule-review",
                    key('u'),
                    Variant::Primary,
                    writable && has_plan,
                ),
            ],
        ),
        2 => ui.buttons(
            toolbar,
            &[
                ("cc-list", key('l'), Variant::Primary, writable && has_plan),
                (
                    "bc-prev",
                    key(','),
                    Variant::Quiet,
                    panel.remote_page().is_some_and(|r| r.files.len() > 1),
                ),
                (
                    "bc-next",
                    key('.'),
                    Variant::Quiet,
                    panel.remote_page().is_some_and(|r| r.files.len() > 1),
                ),
                (
                    "cc-download",
                    key('D'),
                    Variant::Secondary,
                    writable
                        && panel
                            .remote_page()
                            .is_some_and(|r| r.selected_index(panel.cloud_file.as_ref()).is_some()),
                ),
            ],
        ),
        _ => ui.buttons(
            toolbar,
            &[(
                "bc-history",
                Action::Backup(Command::Page(BackupPage::History)),
                Variant::Secondary,
                true,
            )],
        ),
    }
}

pub(super) fn worker_available(panel: &Panel) -> bool {
    sample(panel).is_some_and(|s| {
        matches!(
            s.status,
            ServerStatus::Running(_) | ServerStatus::Degraded(_)
        ) && s.operations.cloud.is_some()
    }) && !panel.diagnostics.pending
}

pub(super) fn ready_connection<'a>(
    panel: &'a Panel,
    id: &str,
) -> Option<&'a crate::CloudConnection> {
    if !worker_available(panel) {
        return None;
    }
    sample(panel)?
        .operations
        .cloud
        .as_ref()?
        .connections
        .iter()
        .find(|c| c.ready && c.id == id)
}

pub(super) fn choose_connection(panel: &mut Panel, id: u128) {
    let Some(connection) = ready_connection(panel, &format!("{id:032x}")) else {
        panel.access_notice = Some("cc-connection-changed");
        return;
    };
    let op = crate::CloudOperation::new_plan(connection.id.clone());
    panel.form = Some(crate::Form::new(crate::Operation::Cloud(op)));
    panel.backups.page = BackupPage::Connections;
    panel.backups.offset = 0;
    panel.access_notice = None;
}

pub(super) fn connections_for_plan(ui: &mut Controls<'_, '_>, area: Rect, panel: &Panel) {
    block(ui.f, area, ui.m.get("cc-plan-ready"));
    let r = inner(area);
    if !worker_available(panel) {
        line(
            ui.f,
            row(r, 1, 1),
            ui.m.get("cc-offline"),
            theme::WARNING,
            false,
        );
        return;
    }
    let Some(view) = panel.cloud_view() else {
        return;
    };
    let ready: Vec<_> = view.connections.iter().filter(|c| c.ready).collect();
    if ready.is_empty() {
        line(
            ui.f,
            row(r, 1, 1),
            ui.m.get("cc-no-ready"),
            theme::MUTED,
            false,
        );
        return;
    }
    let slots = usize::from(r.height.saturating_sub(2));
    let start = panel.backups.offset.min(ready.len().saturating_sub(slots));
    for (connection, y) in ready.iter().skip(start).take(slots).zip(1u16..) {
        if let Ok(id) = u128::from_str_radix(&connection.id, 16) {
            Button::new(
                format!(
                    "{}  ·  {}",
                    crate::dashboard::text(&connection.name),
                    crate::dashboard::text(&connection.provider)
                ),
                Action::Backup(Command::NewPlan(id)),
                Variant::Secondary,
            )
            .draw(ui.f, row(r, y, 1), ui.scene);
        }
    }
    if ready.len() > slots {
        pager(
            ui,
            row(r, r.height.saturating_sub(1), 1),
            start,
            ready.len(),
            slots,
        );
    }
}

pub(super) fn handle_guard(panel: &mut Panel, key: char) -> bool {
    if panel.backups.page == BackupPage::Schedules && key == 'u' {
        if let Some(id) = panel
            .backups
            .schedule
            .as_ref()
            .and_then(|t| u128::from_str_radix(&t.plan, 16).ok())
        {
            crate::schedule_view::review(panel, id);
        }
        return true;
    }
    if unavailable(panel, key) {
        panel.access_notice = Some("cc-offline");
        return true;
    }
    if key == 'g' && panel.backups.page == BackupPage::Connections {
        panel.cloud_tab = 1;
        panel.backups.page = BackupPage::PlanConnection;
        panel.backups.offset = 0;
        panel.access_notice = None;
        return true;
    }
    false
}

fn unavailable(panel: &Panel, key: char) -> bool {
    panel.section == Section::Backup
        && panel.backups.page == BackupPage::Connections
        && matches!(
            key,
            'p' | 'c' | 'a' | 't' | 'y' | 'g' | 'u' | 'h' | 'G' | 'C' | 'l' | 'D' | 'U' | 'M'
        )
        && !worker_available(panel)
}
