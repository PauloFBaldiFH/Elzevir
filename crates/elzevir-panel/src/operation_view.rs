// SPDX-License-Identifier: GPL-3.0-or-later
//! Operational forms and job history; only recognized presentation keys are localized.
use crate::dashboard::{row, text};
use crate::ui::{Action, theme};
use crate::{Messages, Panel, Snapshot};
use ratatui::{
    Frame,
    crossterm::event::KeyCode,
    layout::Rect,
    style::{Modifier, Style},
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
        _ => vec![
            Line::from(messages.get("op-update-help")),
            row(
                messages,
                "op-latest",
                operations.and_then(|ops| ops.latest.as_deref()).map(text),
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
                "op-update" | "op-update-recover" | "op-install-cloudflared" | "op-install-rclone"
            ),
            _ => matches!(
                job.action,
                "op-backup-now"
                    | "op-backup-export"
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
            if let Some(phase) = job.phase {
                rows.push(Line::from(messages.get(phase)));
            }
            if job.action == "op-backup-now" {
                rows.push(Line::from(format!("elzevir-{}.age", text(&job.id))));
            } else {
                rows.push(Line::from(text(&job.id)));
            }
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

pub(crate) fn form(
    frame: &mut Frame,
    panel: &Panel,
    messages: &Messages,
    ascii: bool,
    scene: &mut crate::ui::Scene,
) {
    let Some(form) = &panel.form else {
        return;
    };
    let area = frame.area();
    let header = form_header(panel, form, messages);
    let field_count = u16::try_from(form.labels.len()).unwrap_or(8);
    let header_rows = u16::try_from(header.len()).unwrap_or(4);
    let height = (header_rows + 2 * field_count + 6).min(area.height);
    let popup = Rect::new(
        area.x + area.width.saturating_sub(76) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        area.width.min(76),
        height,
    );
    frame.render_widget(Clear, popup);
    let block = crate::view::block(messages.get("op-dialog"), ascii);
    let inner = block.inner(popup).inner(ratatui::layout::Margin::new(1, 0));
    frame.render_widget(block, popup);
    frame.render_widget(
        Paragraph::new(header).wrap(Wrap { trim: false }),
        Rect::new(inner.x, inner.y, inner.width, header_rows),
    );
    fields(
        frame,
        form,
        Rect::new(
            inner.x,
            inner.y + header_rows + 1,
            inner.width,
            inner.height.saturating_sub(header_rows + 4),
        ),
        messages,
        scene,
    );
    let help = Rect::new(inner.x, inner.bottom().saturating_sub(3), inner.width, 1);
    frame.render_widget(
        Paragraph::new(messages.get(if form.labels.is_empty() {
            "op-confirm"
        } else {
            "op-edit-help"
        }))
        .style(Style::new().fg(theme::MUTED)),
        help,
    );
    let buttons = Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1);
    scene.buttons(
        frame,
        buttons,
        [
            (
                crate::ui::label(messages.get("ui-save"), ascii),
                Action::Submit,
                true,
            ),
            (messages.get("ui-cancel"), Action::Key(KeyCode::Esc), false),
        ],
    );
}

fn form_header(panel: &Panel, form: &crate::Form, messages: &Messages) -> Vec<Line<'static>> {
    let mut header = vec![Line::from(messages.get(form.operation.key()))];
    if let crate::Mode::Live(state) = &panel.mode {
        header.push(Line::from(
            state
                .institution
                .name
                .as_deref()
                .map_or_else(|| messages.get("panel-no-name"), text),
        ));
        header.push(row(
            messages,
            "op-sessions",
            state
                .snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.operations.sessions)
                .map(|count| count.to_string()),
        ));
    }
    if matches!(form.operation, crate::Operation::BackupNow { .. })
        && let crate::Mode::Live(state) = &panel.mode
        && let Some(sample) = state.snapshot.as_ref()
    {
        if let Some(bytes) = sample.storage.database_bytes {
            let estimate = bytes
                .saturating_add(sample.storage.wal_bytes.unwrap_or_default())
                .saturating_mul(3);
            header.push(row(
                messages,
                "backup-estimate",
                Some(format!("{} MiB", estimate.div_ceil(1 << 20))),
            ));
        }
        if form.values.last().is_some_and(String::is_empty)
            && let Some(folder) = &sample.operations.backup_folder
        {
            header.push(Line::from(text(&folder.to_string_lossy())));
        }
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
        header.push(row(messages, "cloud-question", Some(text(&question.name))));
    }
    header
}

/// Every field at once: a label, then its input or its choices.
fn fields(
    frame: &mut Frame,
    form: &crate::Form,
    area: Rect,
    messages: &Messages,
    scene: &mut crate::ui::Scene,
) {
    let mut y = area.y;
    for (index, (key, value)) in form.labels.iter().zip(&form.values).enumerate() {
        if y + 1 >= area.bottom() {
            break;
        }
        let focused = form.field == index;
        frame.render_widget(
            Paragraph::new(field_label(messages, key)).style(if focused {
                Style::new().fg(theme::ACCENT).add_modifier(Modifier::BOLD)
            } else {
                Style::new().fg(theme::TEXT)
            }),
            Rect::new(area.x, y, area.width, 1),
        );
        let input = Rect::new(area.x, y + 1, area.width, 1);
        if let Some(choices) = crate::operations::choices(key) {
            choice_buttons(frame, input, (index, value, choices), messages, scene);
        } else {
            let hover = scene.add(input, Action::Field(index));
            let shown = field_value(key, value);
            let cursor = if focused { "▏" } else { "" };
            frame.render_widget(
                Paragraph::new(format!(" {shown}{cursor}")).style(if focused || hover {
                    Style::new().fg(theme::TEXT).bg(theme::BORDER)
                } else {
                    Style::new().fg(theme::TEXT).bg(theme::BACKGROUND)
                }),
                input,
            );
        }
        y += 2;
    }
}

/// The field's name alone: the label keys also carry the value for the
/// summary lines, which the input shows here instead.
fn field_label(messages: &Messages, key: &str) -> String {
    messages
        .values(key, &[("value", String::new())])
        .trim_end()
        .trim_end_matches(':')
        .to_owned()
}

fn choice_buttons(
    frame: &mut Frame,
    area: Rect,
    (index, value, choices): (usize, &str, &'static [(&'static str, &'static str)]),
    messages: &Messages,
    scene: &mut crate::ui::Scene,
) {
    scene.buttons(
        frame,
        area,
        choices.iter().map(|(choice, key)| {
            (
                messages.get(key),
                Action::Choose(index, choice),
                *choice == value,
            )
        }),
    );
}

fn field_value(key: &str, value: &str) -> String {
    if matches!(key, "broker-password" | "cloud-secret-answer") {
        "*".repeat(value.chars().count().min(64))
    } else {
        text(value)
    }
}
