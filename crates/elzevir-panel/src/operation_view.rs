// SPDX-License-Identifier: GPL-3.0-or-later
//! Operational forms and job history; only recognized presentation keys are localized.
use crate::dashboard::{row, text};
use crate::ui::{Action, Button, Variant, theme};
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
    panel: &Panel,
    messages: &Messages,
) -> Vec<Line<'static>> {
    let section = panel.section;
    let operations = snapshot.map(|snapshot| &snapshot.operations);
    let mut rows = match section {
        crate::Section::Backup => backup_rows(operations, panel, messages),
        _ => vec![
            Line::from(messages.get("op-update-help")),
            row(
                messages,
                "op-latest",
                operations.and_then(|ops| ops.latest.as_deref()).map(text),
            ),
        ],
    };
    if panel.diagnostics.pending {
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
            crate::Section::Backup if panel.cloud_tab != 3 && operations.cloud.is_some() => {
                matches!(job.state, "op-queued" | "op-running")
                    && matches!(
                        job.action,
                        "op-backup-now"
                            | "op-backup-export"
                            | "op-backup-verify"
                            | "op-restore"
                            | "cloud-operation"
                            | "cloud-backup"
                    )
            }
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

fn backup_rows(
    operations: Option<&crate::Operations>,
    panel: &Panel,
    m: &Messages,
) -> Vec<Line<'static>> {
    let mut rows = Vec::new();
    if operations.is_some_and(|ops| ops.review.is_some()) {
        rows.push(Line::from(m.get("restore-ready")));
    }
    let Some(view) = operations.and_then(|o| o.cloud.as_ref()) else {
        rows.push(Line::from(m.get("schedule-cloud-unknown")));
        return rows;
    };
    let plan = view
        .plans
        .get(panel.cloud_plan)
        .or_else(|| view.plans.first());
    if let Some(plan) = plan
        && panel.cloud_tab != 0
    {
        rows.push(Line::from(format!(
            "{} · {} / {}",
            text(&plan.name),
            connection_name(view, &plan.connection),
            text(&plan.folder)
        )));
    }
    if panel.cloud_tab == 1
        && let Some(plan) = plan
        && panel.cloud_targets.contains(&plan.target())
    {
        rows.push(Line::from(m.get("cloud-marked")));
    }
    match panel.cloud_tab {
        0 => connection_rows(&mut rows, view, operations, m),
        1 => {
            rows.push(Line::from(m.get("cloud-plan-help")));
            rows.push(Line::from(m.get("cloud-send-keys")));
            if let Some(plan) = plan {
                rows.push(row(m, "cloud-plan-id", Some(text(&plan.id))));
                rows.push(row(m, "op-retention", Some(plan.keep.to_string())));
                rows.push(row(
                    m,
                    "op-daily",
                    Some(plan.daily.clone().unwrap_or_else(|| m.get("op-disabled"))),
                ));
                if plan.retention_pending {
                    rows.push(Line::from(m.get("cloud-retention-pending")));
                }
            } else {
                rows.push(Line::from(m.get("cloud-no-plans")));
            }
        }
        2 => file_rows(&mut rows, view, plan, panel.cloud_file.as_ref(), m),
        _ => {
            rows.push(Line::from(m.get("cloud-send-keys")));
            for (target, id, state) in &view.transfers {
                rows.push(Line::from(format!(
                    "{} · {} / {}",
                    m.get(state),
                    connection_name(view, &target.connection),
                    text(&target.folder)
                )));
                rows.push(Line::from(text(id)));
            }
        }
    }
    rows
}
fn connection_rows(
    rows: &mut Vec<Line<'static>>,
    view: &crate::CloudView,
    operations: Option<&crate::Operations>,
    m: &Messages,
) {
    rows.push(Line::from(m.get("cloud-help")));
    if view.connections.is_empty() {
        rows.push(Line::from(m.get("cloud-no-connections")));
    }
    for c in &view.connections {
        rows.push(Line::from(format!(
            "{} ({}) · {}",
            text(&c.name),
            text(&c.provider),
            m.get(if c.test_failed {
                "cloud-test-failed"
            } else if c.ready {
                "cloud-ready"
            } else {
                "cloud-auth-pending"
            })
        )));
        rows.push(Line::from(text(&c.id)));
        rows.push(row(m, "cloud-tested", c.tested_at.as_deref().map(text)));
    }
    if let Some(q) = &view.question {
        rows.push(row(m, "cloud-question", Some(text(&q.name))));
        if q.refused {
            rows.push(Line::from(m.get("cloud-refused")));
        }
        rows.push(Line::from(text(&q.help)));
        for (v, h) in &q.examples {
            rows.push(Line::from(format!("{} · {}", text(v), text(h))));
        }
        rows.push(Line::from(m.get("cloud-answer-help")));
    }
    for uri in operations.into_iter().flat_map(|o| &o.authorization_uris) {
        rows.push(row(m, "op-authorize-uri", Some(text(uri))));
    }
    if view.connections.is_empty() {
        for (name, desc) in &view.providers {
            rows.push(Line::from(format!("{} · {}", text(name), text(desc))));
        }
    }
}
fn file_rows(
    rows: &mut Vec<Line<'static>>,
    view: &crate::CloudView,
    plan: Option<&crate::CloudPlan>,
    selected: Option<&(crate::CloudTarget, String)>,
    m: &Messages,
) {
    rows.push(Line::from(m.get("cloud-file-keys")));
    if let Some((target, key)) = &view.notice
        && plan.is_some_and(|p| p.target() == *target)
    {
        rows.push(Line::from(m.get(key)));
    }
    if let Some((target, path)) = &view.download
        && plan.is_some_and(|p| p.target() == *target)
    {
        rows.push(Line::from(format!("O · {}", text(&path.to_string_lossy()))));
    }
    if let Some(r) = view
        .remote
        .as_ref()
        .filter(|r| plan.is_some_and(|p| p.target() == r.target))
    {
        rows.push(row(
            m,
            "cloud-page",
            Some(format!("{} / {}", r.page, r.pages)),
        ));
        rows.push(row(
            m,
            "cloud-quota",
            Some(
                r.free
                    .map_or_else(|| m.get("cloud-unavailable"), |size| format!("{size} B")),
            ),
        ));
        if r.files.is_empty() {
            rows.push(Line::from(m.get("cloud-folder-empty")));
        }
        let index = r.selected_index(selected);
        if selected.is_some() && index.is_none() {
            rows.push(Line::from(m.get("cloud-reselect")));
        }
        for (i, (id, size)) in r.files.iter().enumerate() {
            let marker = if Some(i) == index { ">" } else { " " };
            rows.push(Line::from(format!(
                "{marker} elzevir-{}.age   {size} B",
                text(id)
            )));
        }
    } else {
        rows.push(Line::from(m.get("cloud-list-first")));
    }
}
fn connection_name(view: &crate::CloudView, id: &str) -> String {
    view.connections
        .iter()
        .find(|c| c.id == id)
        .map_or_else(|| text(id), |c| text(&c.name))
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
    let header_rows = u16::try_from(header.len())
        .unwrap_or(4)
        .min(area.height.saturating_sub(8));
    let button_height = if area.height >= 24 { 3 } else { 1 };
    let height = (header_rows + 2 * field_count + 6 + button_height - 1).min(area.height);
    let popup = Rect::new(
        area.x + area.width.saturating_sub(76) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        area.width.min(76),
        height,
    );
    frame.render_widget(Clear, popup);
    let block = crate::view::block(messages.get("op-dialog"), ascii)
        .title_style(Style::new().fg(theme::TEXT).add_modifier(Modifier::BOLD));
    let inner = block.inner(popup).inner(ratatui::layout::Margin::new(1, 0));
    frame.render_widget(block, popup);
    // Keep every review item on screen, including scope and outage, even for a long filename.
    let summary = Paragraph::new(header);
    let summary = if matches!(form.operation, crate::Operation::ApplyRestore(_)) {
        summary
    } else {
        summary.wrap(Wrap { trim: false })
    };
    frame.render_widget(
        summary,
        Rect::new(inner.x, inner.y, inner.width, header_rows),
    );
    fields(
        frame,
        form,
        Rect::new(
            inner.x,
            inner.y + header_rows + 1,
            inner.width,
            inner.height.saturating_sub(header_rows + 3 + button_height),
        ),
        messages,
        scene,
    );
    let help = Rect::new(
        inner.x,
        inner.bottom().saturating_sub(2 + button_height),
        inner.width,
        1,
    );
    frame.render_widget(
        Paragraph::new(messages.get(if form.labels.is_empty() {
            "op-confirm"
        } else {
            "op-edit-help"
        }))
        .style(Style::new().fg(theme::MUTED)),
        help,
    );
    let buttons = Rect::new(
        inner.x,
        inner.bottom().saturating_sub(button_height),
        inner.width,
        button_height,
    );
    let [confirm, cancel] =
        ratatui::layout::Layout::horizontal([ratatui::layout::Constraint::Fill(1); 2])
            .spacing(1)
            .areas(buttons);
    let (key, variant) = match form.operation {
        crate::Operation::Restore { .. } => ("restore-validate", Variant::Primary),
        crate::Operation::ApplyRestore(_) => ("restore-confirm", Variant::Danger),
        crate::Operation::Server(crate::ServerAction::Stop) => ("desk-stop", Variant::Danger),
        crate::Operation::BackupNow { .. } => ("desk-create", Variant::Primary),
        _ => ("desk-confirm", Variant::Primary),
    };
    Button::new(messages.get(key), Action::Submit, variant).draw(frame, confirm, scene);
    Button::new(
        messages.get("desk-cancel"),
        Action::Key(KeyCode::Esc),
        Variant::Secondary,
    )
    .draw(frame, cancel, scene);
}

fn destination_header(
    panel: &Panel,
    form: &crate::Form,
    messages: &Messages,
    header: &mut Vec<Line<'static>>,
) {
    let target = match &form.operation {
        crate::Operation::Cloud(crate::CloudOperation::SendGroup(targets, _)) => {
            header.push(row(
                messages,
                "cloud-target-page",
                Some(format!("{} / {}", form.target_page + 1, targets.len())),
            ));
            targets.get(form.target_page)
        }
        crate::Operation::Cloud(crate::CloudOperation::Remote(target, cmd)) => {
            if let crate::RemoteCommand::Download(id) = cmd {
                header.push(Line::from(format!("elzevir-{}.age", text(id))));
            }
            Some(target)
        }
        _ => None,
    };
    if let Some(target) = target {
        header.push(row(
            messages,
            "cloud-id",
            Some(panel.cloud_view().map_or_else(
                || text(&target.connection),
                |v| connection_name(v, &target.connection),
            )),
        ));
        let folder = messages.values("cloud-folder", &[("value", text(&target.folder))]);
        for line in folder.chars().collect::<Vec<_>>().chunks(60) {
            header.push(Line::from(line.iter().collect::<String>()));
        }
    }
}

fn address_header(
    panel: &Panel,
    form: &crate::Form,
    messages: &Messages,
    header: &mut Vec<Line<'static>>,
) {
    if panel.access_help == Some("address-terminal")
        && matches!(
            form.operation,
            crate::Operation::Address(
                crate::AddressOperation::Recover { .. } | crate::AddressOperation::Export { .. }
            )
        )
    {
        header.push(Line::from(messages.get("address-sftp")));
    }
    if matches!(
        form.operation,
        crate::Operation::Address(crate::AddressOperation::Enroll { .. })
    ) {
        if let Some(name) = form.values.get(2).filter(|s| !s.is_empty()) {
            header.push(Line::from(messages.get("address-preview")));
            header.push(Line::from(format!("https://{}.elzevir.app/", text(name))));
        }
        if let crate::Mode::Live(state) = &panel.mode {
            let origin = state.institution.configured_listen.and_then(|mut addr| {
                if addr.ip().is_unspecified() {
                    addr.set_ip(if addr.is_ipv4() {
                        std::net::Ipv4Addr::LOCALHOST.into()
                    } else {
                        std::net::Ipv6Addr::LOCALHOST.into()
                    });
                }
                (addr.ip().is_loopback() && addr.port() != 0).then(|| format!("http://{addr}"))
            });
            header.push(row(messages, "address-origin", origin));
        }
        header.push(Line::from(messages.get("address-staff-setup")));
    }
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
    address_header(panel, form, messages, &mut header);
    destination_header(panel, form, messages, &mut header);
    if matches!(form.operation, crate::Operation::Restore { .. }) {
        header.push(Line::from(messages.get("restore-keys")));
        for (i, check) in form.checks.iter().enumerate() {
            if let (Some(value), Some(label)) = (form.values.get(i), form.labels.get(i)) {
                header.push(row(
                    messages,
                    label,
                    Some(messages.get(check.status(value))),
                ));
            }
        }
        if let Some(notice) = form.notice {
            header.push(Line::from(messages.get(notice)));
        }
    }
    if let crate::Operation::ApplyRestore(review) = &form.operation {
        header.push(Line::from(text(&review.file)));
        header.push(Line::from(format!(
            "{}   {}",
            text(&review.id),
            text(&review.created)
        )));
        header.push(row(
            messages,
            "restore-schema",
            Some(review.schema.to_string()),
        ));
        header.push(Line::from(messages.get(if review.managed {
            "restore-managed"
        } else {
            "restore-unknown"
        })));
        header.push(Line::from(messages.get(match review.scope {
            crate::BackupScope::Database => "restore-db-impact",
            crate::BackupScope::Full => "restore-full-impact",
        })));
        header.push(Line::from(messages.get("restore-outage")));
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

/// Scroll the field list just enough to keep the focused label and input together.
fn fields(
    frame: &mut Frame,
    form: &crate::Form,
    area: Rect,
    messages: &Messages,
    scene: &mut crate::ui::Scene,
) {
    let mut y = area.y;
    let visible = usize::from(area.height / 2);
    let focus = form.field.min(form.labels.len().saturating_sub(1));
    let first = (focus + 1).saturating_sub(visible);
    for (index, (key, value)) in form.labels.iter().zip(&form.values).enumerate().skip(first) {
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
            let hover = scene.add(Rect::new(area.x, y, area.width, 2), Action::Field(index));
            let shown = field_value(key, value);
            let cursor = if focused { "▏" } else { "" };
            frame.render_widget(
                Paragraph::new(format!(" {shown}{cursor}"))
                    .scroll((
                        0,
                        if focused {
                            u16::try_from(Line::from(shown.as_str()).width() + 2)
                                .unwrap_or(u16::MAX)
                                .saturating_sub(input.width)
                        } else {
                            0
                        },
                    ))
                    .style(if focused || hover {
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
    let slots = ratatui::layout::Layout::horizontal(
        choices.iter().map(|_| ratatui::layout::Constraint::Fill(1)),
    )
    .spacing(1)
    .split(area);
    for ((choice, key), slot) in choices.iter().zip(slots.iter().copied()) {
        let selected = *choice == value;
        let mark = if selected { "(*)" } else { "( )" };
        Button::new(
            format!("{mark} {}", messages.get(key)),
            Action::Choose(index, choice),
            if selected {
                Variant::Secondary
            } else {
                Variant::Quiet
            },
        )
        .draw(frame, slot, scene);
    }
}

fn field_value(key: &str, value: &str) -> String {
    if matches!(key, "broker-password" | "cloud-secret-answer") {
        "*".repeat(value.chars().count().min(64))
    } else {
        text(value)
    }
}
