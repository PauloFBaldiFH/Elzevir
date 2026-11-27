// SPDX-License-Identifier: GPL-3.0-or-later
//! Presentation of institution-owned backup policies.
use crate::{
    CloudOperation, CloudPlan, Form, Operation, Operations, Panel, RemoteCommand, ServerStatus,
    backup_center::{Command, Controls, block, inner, line, pager, row, sample},
    dashboard::text,
    ui::{Action, Button, Variant, theme},
};
use ratatui::layout::{Constraint, Layout, Rect};

fn online(s: &crate::Snapshot) -> bool {
    matches!(
        s.status,
        ServerStatus::Running(_) | ServerStatus::Degraded(_)
    )
}

pub(super) fn policy(panel: &Panel) -> Option<Operation> {
    let ops = &sample(panel).filter(|s| online(s))?.operations;
    let keep = ops.keep?;
    let (time, zone) = match ops.daily.as_deref() {
        Some(daily) => daily.split_once(' ')?,
        None => ("", "UTC"),
    };
    Some(Operation::Policy {
        keep: keep.to_string(),
        time: time.into(),
        zone: zone.into(),
    })
}

fn selected<'a>(panel: &Panel, ops: &'a Operations) -> Option<&'a CloudPlan> {
    let target = panel.backups.schedule.as_ref()?;
    ops.cloud.as_ref()?.plans.iter().find(|p| {
        p.id == target.plan && p.connection == target.connection && p.folder == target.folder
    })
}

pub(super) fn edit(panel: &mut Panel, id: u128, pause: bool) {
    if !crate::cloud_view::worker_available(panel) {
        return;
    }
    let Some(plan) = sample(panel)
        .and_then(|s| selected(panel, &s.operations))
        .filter(|p| u128::from_str_radix(&p.id, 16) == Ok(id))
    else {
        return;
    };
    let Some(policy) = plan.policy() else { return };
    let mut form = Form::new(Operation::Cloud(policy));
    if pause
        && let Some(time) = form
            .labels
            .iter()
            .position(|label| *label == "op-time")
            .and_then(|i| form.values.get_mut(i))
    {
        time.clear();
    }
    panel.form = Some(form);
    panel.access_notice = None;
}

pub(super) fn obsolete(panel: &Panel, op: &Operation) -> Option<&'static str> {
    match op {
        Operation::Policy { .. } if policy(panel).as_ref() != Some(op) => Some("schedule-reopen"),
        Operation::Cloud(cloud @ CloudOperation::Plan { id, .. }) if !id.is_empty() => {
            let current = sample(panel)
                .filter(|s| online(s))
                .and_then(|s| s.operations.cloud.as_ref())
                .and_then(|c| c.plans.iter().find(|p| p.id == *id))
                .and_then(CloudPlan::policy);
            (current.as_ref() != Some(cloud)).then_some("schedule-plan-changed")
        }
        Operation::Cloud(CloudOperation::Plan { connection, .. }) if !connection.is_empty() => {
            crate::cloud_view::ready_connection(panel, connection)
                .is_none()
                .then_some("cc-connection-changed")
        }
        _ => None,
    }
}

pub(super) fn review(panel: &mut Panel, id: u128) {
    if panel.diagnostics.pending || !sample(panel).is_some_and(online) {
        return;
    }
    if let Some(plan) = sample(panel).and_then(|s| selected(panel, &s.operations))
        && u128::from_str_radix(&plan.id, 16) == Ok(id)
    {
        panel.form = Some(Form::new(Operation::Cloud(CloudOperation::Remote(
            plan.target(),
            RemoteCommand::Create,
        ))));
        panel.access_notice = None;
    }
}

pub(super) fn draw(ui: &mut Controls<'_, '_>, area: Rect, panel: &Panel, ops: Option<&Operations>) {
    let wide = area.width >= 104 || area.height < 12;
    let available = area;
    let area = if wide && area.height >= 18 {
        let count = ops
            .and_then(|o| o.cloud.as_ref())
            .map_or(0, |v| v.plans.len());
        row(area, 0, 11 + u16::try_from(count.min(6)).unwrap_or(0))
    } else {
        area
    };
    let (local, cloud) = if wide {
        let [a, b] = Layout::horizontal([Constraint::Percentage(39), Constraint::Fill(1)])
            .spacing(1)
            .areas(area);
        (a, b)
    } else {
        let [a, b] = Layout::vertical([Constraint::Length(4), Constraint::Fill(1)])
            .spacing(1)
            .areas(area);
        (a, b)
    };
    block(ui.f, local, ui.m.get("schedule-local"));
    let d = inner(local);
    let m = ui.m;
    if let Some((ops, keep)) = ops.and_then(|o| Some((o, o.keep?))) {
        line(
            ui.f,
            row(d, 1, 1),
            ops.daily.as_deref().map_or_else(
                || m.get("home-unscheduled"),
                |v| m.values("op-daily", &[("value", text(v))]),
            ),
            theme::TEXT,
            true,
        );
        line(
            ui.f,
            row(d, 2, 1),
            m.values("op-retention", &[("value", keep.to_string())]),
            theme::TEXT,
            false,
        );
        if let Some(path) = &ops.backup_folder {
            line(
                ui.f,
                row(d, 3, 1),
                text(&path.to_string_lossy()),
                theme::MUTED,
                false,
            );
        }
    } else {
        line(
            ui.f,
            row(d, 1, 1),
            m.get("schedule-policy-unknown"),
            theme::WARNING,
            false,
        );
    }
    if wide {
        fyi(ui, row(d, 6, 4), "schedule-local-help");
    }
    cloud_plans(ui, cloud, panel, ops);
    if available.height > area.height + 4 {
        let top = area.height + 1;
        let activity = row(available, top, available.height.saturating_sub(top).min(8));
        block(ui.f, activity, m.get("schedule-activity"));
        if let Some(ops) = ops.filter(|o| o.keep.is_some()) {
            let mut jobs = ops
                .jobs
                .iter()
                .filter(|j| matches!(j.action, "op-backup-now" | "cloud-backup"))
                .peekable();
            if jobs.peek().is_none() {
                fyi(ui, row(inner(activity), 1, 2), "schedule-no-runs");
            }
            for (i, j) in jobs
                .take(usize::from(activity.height.saturating_sub(1)))
                .enumerate()
            {
                line(
                    ui.f,
                    row(inner(activity), 1 + u16::try_from(i).unwrap_or(0), 1),
                    format!(
                        "{}  ·  {}  ·  {}",
                        text(&j.at),
                        m.get(j.action),
                        m.get(j.state)
                    ),
                    theme::TEXT,
                    false,
                );
            }
        } else {
            fyi(ui, row(inner(activity), 1, 2), "schedule-activity-unknown");
        }
    }
}

fn fyi(ui: &mut Controls<'_, '_>, r: Rect, key: &str) {
    use ratatui::widgets::{Paragraph, Wrap};
    ui.f.render_widget(
        Paragraph::new(ui.m.get(key))
            .style(theme::surface())
            .wrap(Wrap { trim: true }),
        r,
    );
}

fn cloud_plans(ui: &mut Controls<'_, '_>, area: Rect, panel: &Panel, ops: Option<&Operations>) {
    let m = ui.m;
    block(ui.f, area, m.get("schedule-cloud"));
    let r = inner(area);
    let Some(view) = ops.and_then(|o| o.cloud.as_ref()) else {
        fyi(ui, row(r, 1, 3), "schedule-cloud-unknown");
        return;
    };
    if view.plans.is_empty() {
        fyi(ui, row(r, 1, 3), "schedule-no-plans");
        return;
    }
    let plan = ops.and_then(|o| selected(panel, o));
    // Keep the selection, pager and review in the viewport at the minimum size.
    let slots = usize::from(r.height.saturating_sub(7).max(1));
    let start = panel
        .backups
        .offset
        .min(view.plans.len().saturating_sub(slots));
    for (i, candidate) in view.plans.iter().skip(start).take(slots).enumerate() {
        let Ok(id) = u128::from_str_radix(&candidate.id, 16) else {
            continue;
        };
        let current = plan.is_some_and(|p| p.id == candidate.id);
        let cadence = candidate
            .daily
            .as_deref()
            .map_or_else(|| m.get("home-unscheduled"), text);
        let label = format!(
            "{}{}  ·  {}",
            if current { "> " } else { "" },
            text(&candidate.name),
            cadence
        );
        Button::new(
            label,
            Action::Backup(Command::SelectSchedule(id)),
            if current {
                Variant::Primary
            } else {
                Variant::Secondary
            },
        )
        .draw(ui.f, row(r, 1 + u16::try_from(i).unwrap_or(0), 1), ui.scene);
    }
    let y = (1 + u16::try_from(slots.min(view.plans.len())).unwrap_or(0))
        .min(r.height.saturating_sub(1));
    if view.plans.len() > slots {
        pager(ui, row(r, y, 1), start, view.plans.len(), slots);
    }
    let d = row(r, y + u16::from(view.plans.len() > slots), 5);
    let Some(plan) = plan else {
        fyi(ui, d, "schedule-select");
        return;
    };
    plan_details(ui, d, panel, view, plan);
}

fn plan_details(
    ui: &mut Controls<'_, '_>,
    area: Rect,
    panel: &Panel,
    view: &crate::CloudView,
    plan: &CloudPlan,
) {
    let m = ui.m;
    let name = view
        .connections
        .iter()
        .find(|c| c.id == plan.connection)
        .map_or(&plan.connection, |c| &c.name);
    line(
        ui.f,
        area,
        format!("{} / {}", text(name), text(&plan.folder)),
        theme::TEXT,
        true,
    );
    line(
        ui.f,
        row(area, 1, 1),
        m.values("op-retention", &[("value", plan.keep.to_string())]),
        theme::TEXT,
        false,
    );
    let (attempt, requested) = attempt_details(sample(panel).map(|s| &s.operations), plan, m);
    line(ui.f, row(area, 2, 1), attempt, theme::TEXT, false);
    if let Some(requested) = requested {
        line(ui.f, row(area, 3, 1), requested, theme::MUTED, false);
    }
    if let Ok(id) = u128::from_str_radix(&plan.id, 16) {
        let writable = crate::cloud_view::worker_available(panel);
        let paused = plan.daily.is_none();
        ui.buttons(
            row(area, 4, 1),
            &[
                (
                    "schedule-review",
                    Action::Backup(Command::RunSchedule(id)),
                    Variant::Secondary,
                    writable,
                ),
                (
                    if paused {
                        "schedule-enable"
                    } else {
                        "schedule-edit-plan"
                    },
                    Action::Backup(Command::EditSchedule(id, false)),
                    Variant::Secondary,
                    writable,
                ),
                (
                    "schedule-pause",
                    Action::Backup(Command::EditSchedule(id, true)),
                    Variant::Quiet,
                    writable && !paused,
                ),
            ],
        );
    }
}

pub(super) fn attempt_details(
    ops: Option<&Operations>,
    plan: &CloudPlan,
    m: &crate::Messages,
) -> (String, Option<String>) {
    let Some(attempts) = ops.and_then(|o| o.plan_attempts.as_ref()) else {
        return (m.get("schedule-activity-unknown"), None);
    };
    let Some(attempt) = attempts.get(&plan.id) else {
        return (m.get("schedule-no-attempt"), None);
    };
    let status = m.values("schedule-attempt", &[("value", m.get(attempt.state))]);
    let scope = match &attempt.target {
        None => "schedule-request-unspecified",
        Some(t)
            if t.plan != plan.id || t.connection != plan.connection || t.folder != plan.folder =>
        {
            "schedule-request-moved"
        }
        Some(_) => "schedule-request-current",
    };
    (
        status,
        Some(m.values(scope, &[("value", text(&attempt.at))])),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CloudView, Component, Details, Institution, Mode, Snapshot, access_actions::Actions,
    };

    fn live(panel: &mut Panel) -> Option<&mut crate::LiveState> {
        match &mut panel.mode {
            Mode::Live(state) => Some(state),
            Mode::Demo => None,
        }
    }

    fn plans(panel: &mut Panel) -> &mut Vec<CloudPlan> {
        &mut live(panel)
            .unwrap()
            .snapshot
            .as_mut()
            .unwrap()
            .operations
            .cloud
            .as_mut()
            .unwrap()
            .plans
    }

    fn observed(cloud: CloudView) -> Panel {
        let mut panel = Panel::live(Institution::default());
        panel.section = crate::Section::Backup;
        let state = live(&mut panel).unwrap();
        state.snapshot = Some(Snapshot {
            status: ServerStatus::Running(Details {
                version: String::new(),
                pid: 42,
                uptime_seconds: 60,
                database: Component::Ok,
                indexer: Component::Ok,
                listen: None,
            }),
            operations: Operations {
                cloud: Some(cloud),
                ..Default::default()
            },
            ..Default::default()
        });
        panel
    }

    fn cloud_plan() -> CloudPlan {
        CloudPlan {
            id: format!("{:032x}", 1),
            connection: "b".repeat(32),
            folder: "daily".into(),
            name: "Library".into(),
            keep: 7,
            daily: None,
            retention_pending: false,
        }
    }

    #[test]
    fn a_connection_that_disappears_cannot_be_replaced_by_the_next_row() {
        let connection = |id: u128| crate::CloudConnection {
            id: format!("{id:032x}"),
            name: format!("Destination {id}"),
            provider: "alias".into(),
            ready: true,
            tested_at: None,
            test_failed: false,
        };
        let mut panel = observed(CloudView {
            connections: vec![connection(2), connection(1)],
            ..Default::default()
        });
        panel.backups.page = crate::BackupPage::PlanConnection;
        crate::cloud_view::choose_connection(&mut panel, 1);
        let op = CloudOperation::new_plan(format!("{:032x}", 1));
        assert_eq!(panel.form.as_ref().unwrap().operation, Operation::Cloud(op));
        let cloud = live(&mut panel)
            .unwrap()
            .snapshot
            .as_mut()
            .unwrap()
            .operations
            .cloud
            .as_mut()
            .unwrap();
        cloud.connections.retain(|c| c.id == format!("{:032x}", 2));
        panel.submit_form();
        assert_eq!(panel.access_notice, Some("cc-connection-changed"));
        assert!(panel.form.is_none() && panel.diagnostics.requested.is_none());
        crate::cloud_view::choose_connection(&mut panel, 1);
        assert!(panel.form.is_none());
    }

    #[test]
    fn a_plan_receipt_distinguishes_unknown_empty_unrecorded_and_previous_destinations() {
        let m = crate::Messages::new("en").unwrap();
        let mut plan = cloud_plan();
        let mut ops = Operations::default();
        assert_eq!(
            attempt_details(Some(&ops), &plan, &m).0,
            m.get("schedule-activity-unknown")
        );
        ops.plan_attempts = Some(std::collections::BTreeMap::new());
        assert_eq!(
            attempt_details(Some(&ops), &plan, &m).0,
            m.get("schedule-no-attempt")
        );
        let attempt = crate::PlanAttempt {
            at: "2026-01-01 12:01 UTC".into(),
            state: "op-running",
            target: None,
        };
        ops.plan_attempts
            .as_mut()
            .unwrap()
            .insert(plan.id.clone(), attempt);
        let (state, time) = attempt_details(Some(&ops), &plan, &m);
        assert!(state.contains(&m.get("op-running")));
        assert!(time.unwrap().contains("Destination unrecorded"));
        ops.plan_attempts
            .as_mut()
            .unwrap()
            .get_mut(&plan.id)
            .unwrap()
            .target = Some(plan.target());
        assert!(
            attempt_details(Some(&ops), &plan, &m)
                .1
                .unwrap()
                .contains("Requested at")
        );
        plan.folder = "moved".into();
        assert!(
            attempt_details(Some(&ops), &plan, &m)
                .1
                .unwrap()
                .contains("Previous destination")
        );
    }

    #[test]
    fn selected_run_survives_reordering_and_requires_reselection_after_a_move() {
        let plan = cloud_plan();
        let target = plan.target();
        let catalogue = vec![
            plan.clone(),
            CloudPlan {
                id: format!("{:032x}", 2),
                ..plan
            },
        ];
        let mut panel = observed(CloudView {
            plans: catalogue,
            ..Default::default()
        });
        panel.backups.page = crate::BackupPage::Schedules;
        crate::backup_center::apply(
            Command::SelectSchedule(1),
            &mut panel,
            &mut Actions::default(),
            0,
        );
        assert!(panel.form.is_none());
        assert!(panel.diagnostics.requested.is_none());
        plans(&mut panel).reverse();
        panel.cloud_navigation('u');
        assert_eq!(
            panel.form.take().unwrap().operation,
            Operation::Cloud(CloudOperation::Remote(target, RemoteCommand::Create))
        );
        plans(&mut panel)
            .iter_mut()
            .find(|p| p.id == format!("{:032x}", 1))
            .unwrap()
            .folder = "moved".into();
        panel.cloud_navigation('u');
        assert!(panel.form.is_none());
        assert!(panel.diagnostics.requested.is_none());
    }
}
