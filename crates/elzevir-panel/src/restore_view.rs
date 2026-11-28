// SPDX-License-Identifier: GPL-3.0-or-later
//! Review and durable receipts for restoration on the selected institution.
use crate::{
    Form, Operation, Panel, ServerStatus, Snapshot,
    backup_center::{Command, Controls, block, inner, key, line, pager, row, sample},
    dashboard::text,
    ui::{Action, Variant, theme},
};
use ratatui::layout::Rect;

pub(super) fn downloaded(panel: &Panel) -> Option<(u128, u128, &str)> {
    let view = sample(panel)?.operations.cloud.as_ref()?;
    let plan = view
        .plans
        .get(panel.cloud_plan)
        .or_else(|| view.plans.first())?;
    let (target, path) = view.download.as_ref()?;
    if *target != plan.target() {
        return None;
    }
    let id = path
        .file_name()?
        .to_str()?
        .strip_prefix("elzevir-")?
        .strip_suffix(".age")?;
    Some((
        u128::from_str_radix(&plan.id, 16).ok()?,
        u128::from_str_radix(id, 16).ok()?,
        path.to_str()?,
    ))
}

pub(super) fn open_download(panel: &mut Panel, plan: u128, file: u128) {
    if !panel.diagnostics.pending
        && let Some((p, f, path)) = downloaded(panel)
        && (p, f) == (plan, file)
    {
        panel.form = Some(Form::new(Operation::Restore {
            input: path.into(),
            key: String::new(),
        }));
    }
}

pub(super) fn review(panel: &mut Panel, token: u128) {
    if panel.diagnostics.pending {
        return;
    }
    if let Some(review) = sample(panel)
        .and_then(|s| s.operations.review.as_ref())
        .filter(|r| u128::from_str_radix(&r.token, 16) == Ok(token))
    {
        panel.form = Some(Form::new(Operation::ApplyRestore(review.clone())));
    }
}

pub(super) fn service_state(obs: Option<&Snapshot>) -> &'static str {
    match obs.map(|s| &s.status) {
        Some(ServerStatus::Running(_)) => "panel-state-running",
        Some(ServerStatus::Degraded(_)) => "panel-state-degraded",
        Some(ServerStatus::Stopped) => "panel-state-stopped",
        _ => "panel-unknown",
    }
}

pub(super) fn draw(ui: &mut Controls<'_, '_>, area: Rect, panel: &Panel, obs: Option<&Snapshot>) {
    let m = ui.m;
    let ops = obs.map(|s| &s.operations);
    let review = ops
        .and_then(|o| o.review.as_ref())
        .filter(|_| !panel.diagnostics.pending);
    let token = review.and_then(|r| u128::from_str_radix(&r.token, 16).ok());
    let r = inner(area);
    block(ui.f, area, m.get("restore-source"));
    line(
        ui.f,
        row(r, 1, 1),
        m.values(
            "restore-target-service",
            &[("value", m.get(service_state(obs)))],
        ),
        theme::TEXT,
        false,
    );
    let status = review.map_or_else(
        || {
            m.get(if panel.diagnostics.pending {
                "op-pending"
            } else if ops.and_then(|o| o.result) == Some("restore-invalid") {
                "restore-invalid"
            } else {
                "restore-select-source"
            })
        },
        |v| m.values("restore-authenticated", &[("value", text(&v.file))]),
    );
    line(ui.f, row(r, 2, 1), status, theme::TEXT, true);
    ui.buttons(
        row(r, 3, 1),
        &[
            (
                "restore-select",
                key('o'),
                Variant::Secondary,
                obs.is_some() && !panel.diagnostics.pending,
            ),
            (
                "restore-review",
                Action::Backup(Command::ReviewRestore(token.unwrap_or_default())),
                Variant::Primary,
                token.is_some(),
            ),
        ],
    );
    let history = row(area, 5, area.height.saturating_sub(5));
    block(ui.f, history, m.get("restore-history"));
    let jobs: Vec<_> = ops
        .into_iter()
        .flat_map(|o| &o.jobs)
        .filter(|j| j.action == "op-restore")
        .collect();
    if jobs.is_empty() {
        line(
            ui.f,
            row(inner(history), 1, 1),
            m.get("restore-no-receipt"),
            theme::MUTED,
            false,
        );
    }
    let capacity = usize::from(history.height.saturating_sub(2));
    let start = panel
        .backups
        .offset
        .min(jobs.len().saturating_sub(capacity));
    if jobs.len() > capacity {
        pager(
            ui,
            row(inner(history), history.height.saturating_sub(1), 1),
            start,
            jobs.len(),
            capacity,
        );
    }
    for (job, y) in jobs.iter().skip(start).take(capacity).zip(1u16..) {
        line(
            ui.f,
            row(inner(history), y, 1),
            format!("{}  ·  {}", text(&job.at), m.get(job.state)),
            theme::TEXT,
            false,
        );
    }
}
