// SPDX-License-Identifier: GPL-3.0-or-later
//! Operational forms and job history; only recognized presentation keys are localized.
use crate::dashboard::{row, text};
use crate::{Messages, Panel, Snapshot};
use ratatui::{
    Frame,
    layout::Rect,
    text::Line,
    widgets::{Clear, Paragraph, Wrap},
};

pub(crate) fn rows(
    snapshot: Option<&Snapshot>,
    pending: bool,
    section: crate::Section,
    messages: &Messages,
) -> Vec<Line<'static>> {
    let operations = snapshot.map(|snapshot| &snapshot.operations);
    let mut rows = match section {
        crate::Section::Backup => backup_rows(operations, messages),
        crate::Section::Updates => vec![
            Line::from(messages.get("op-update-help")),
            row(
                messages,
                "op-latest",
                operations.and_then(|ops| ops.latest.as_deref()).map(text),
            ),
        ],
        _ => vec![
            Line::from(messages.get("op-restart-help")),
            row(
                messages,
                "op-sessions",
                operations
                    .and_then(|ops| ops.sessions)
                    .map(|count| count.to_string()),
            ),
        ],
    };
    if pending {
        rows.push(Line::from(messages.get("op-pending")));
    }
    if let Some(result) = operations.and_then(|ops| ops.result) {
        rows.push(Line::from(messages.get(result)));
    }
    if section == crate::Section::Updates
        && let Some(notes) = operations.and_then(|ops| ops.release_notes.as_deref())
    {
        rows.push(Line::from(text(notes)));
    }
    rows.push(Line::default());
    if let Some(operations) = operations {
        for job in operations.jobs.iter().filter(|job| match section {
            crate::Section::Updates => matches!(
                job.action,
                "op-update" | "op-install-cloudflared" | "op-install-rclone"
            ),
            crate::Section::Restart => matches!(job.action, "op-restart" | "op-reboot"),
            _ => matches!(
                job.action,
                "op-backup-now"
                    | "op-backup-verify"
                    | "op-restore"
                    | "cloud-operation"
                    | "cloud-backup"
            ),
        }) {
            rows.push(Line::from(format!(
                "{} {} {}",
                text(&job.at),
                messages.get(job.action),
                messages.get(job.state)
            )));
            rows.push(Line::from(text(&job.id)));
        }
    }
    rows
}

fn backup_rows(operations: Option<&crate::Operations>, messages: &Messages) -> Vec<Line<'static>> {
    let mut rows = vec![
        Line::from(messages.get("op-backup-help")),
        Line::from(messages.get("cloud-help")),
        Line::from(messages.get("cloud-plan-help")),
        row(
            messages,
            "op-retention",
            operations
                .and_then(|ops| ops.keep)
                .map(|keep| keep.to_string()),
        ),
        row(
            messages,
            "op-daily",
            Some(
                operations
                    .and_then(|ops| ops.daily.as_deref())
                    .map_or_else(|| messages.get("op-disabled"), text),
            ),
        ),
    ];
    if let Some(operations) = operations {
        for uri in &operations.authorization_uris {
            rows.push(row(messages, "op-authorize-uri", Some(text(uri))));
        }
        if let Some(cloud) = &operations.cloud {
            for connection in &cloud.connections {
                rows.push(Line::from(format!(
                    "{} ({}) — {}",
                    text(&connection.name),
                    text(&connection.provider),
                    messages.get(if connection.ready {
                        "cloud-ready"
                    } else {
                        "cloud-configuring"
                    })
                )));
                rows.push(Line::from(text(&connection.id)));
                rows.push(row(
                    messages,
                    "cloud-tested",
                    connection.tested_at.as_deref().map(text),
                ));
            }
            for plan in &cloud.plans {
                rows.push(Line::from(text(&plan.name)));
                rows.push(row(messages, "cloud-plan-id", Some(text(&plan.id))));
                rows.push(row(messages, "cloud-id", Some(text(&plan.connection))));
                rows.push(row(messages, "cloud-folder", Some(text(&plan.folder))));
                rows.push(row(messages, "op-retention", Some(plan.keep.to_string())));
                rows.push(row(
                    messages,
                    "op-daily",
                    Some(
                        plan.daily
                            .as_deref()
                            .map_or_else(|| messages.get("op-disabled"), text),
                    ),
                ));
                if plan.retention_pending {
                    rows.push(Line::from(messages.get("cloud-retention-pending")));
                }
            }
            if let Some(question) = &cloud.question {
                rows.push(row(messages, "cloud-question", Some(text(&question.name))));
                if question.refused {
                    rows.push(Line::from(messages.get("cloud-refused")));
                }
                rows.push(Line::from(text(&question.help)));
                for (value, help) in &question.examples {
                    rows.push(Line::from(format!("{} — {}", text(value), text(help))));
                }
                rows.push(Line::from(messages.get("cloud-answer-help")));
            }
            if cloud.connections.is_empty() {
                for (name, description) in &cloud.providers {
                    rows.push(Line::from(format!(
                        "{} — {}",
                        text(name),
                        text(description)
                    )));
                }
            }
        }
    }
    rows
}

pub(crate) fn form(frame: &mut Frame, panel: &Panel, messages: &Messages, ascii: bool) {
    let Some(form) = &panel.form else {
        return;
    };
    let area = frame.area();
    let height = if form.confirming() {
        u16::try_from(form.labels.len() + 8).unwrap_or(16).max(12)
    } else {
        12
    };
    let popup = Rect::new(
        area.x + area.width.saturating_sub(72) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        area.width.min(72),
        area.height.min(height),
    );
    let mut rows = vec![
        Line::from(messages.get(form.operation.key())),
        Line::default(),
    ];
    if let crate::Mode::Live(state) = &panel.mode {
        rows.push(Line::from(
            state
                .institution
                .name
                .as_deref()
                .map_or_else(|| messages.get("panel-no-name"), text),
        ));
        rows.push(row(
            messages,
            "op-sessions",
            state
                .snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.operations.sessions)
                .map(|count| count.to_string()),
        ));
    }
    if matches!(
        &form.operation,
        crate::Operation::Cloud(crate::CloudOperation::Answer { .. })
    ) && let crate::Mode::Live(state) = &panel.mode
        && let Some(question) = state
            .snapshot
            .as_ref()
            .and_then(|sample| sample.operations.cloud.as_ref())
            .and_then(|cloud| cloud.question.as_ref())
    {
        rows.push(row(messages, "cloud-question", Some(text(&question.name))));
    }
    if form.confirming() {
        rows.push(Line::from(messages.get("op-confirm")));
        for (key, value) in form.labels.iter().zip(&form.values) {
            rows.push(row(messages, key, Some(field_value(key, value))));
        }
    } else {
        let key = form.labels.get(form.field).copied().unwrap_or("op-output");
        rows.push(row(messages, key, Some(String::new())));
        rows.push(Line::from(format!(
            "> {}",
            form.values
                .get(form.field)
                .map_or_else(String::new, |value| field_value(key, value))
        )));
        rows.push(Line::from(messages.get("op-edit-help")));
    }
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(rows)
            .wrap(Wrap { trim: false })
            .block(crate::view::block(messages.get("op-dialog"), ascii)),
        popup,
    );
}

fn field_value(key: &str, value: &str) -> String {
    if matches!(key, "broker-password" | "cloud-secret-answer") {
        "*".repeat(value.chars().count().min(64))
    } else {
        text(value)
    }
}
