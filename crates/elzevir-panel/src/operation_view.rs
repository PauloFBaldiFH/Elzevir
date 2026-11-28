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
    if section != crate::Section::Backup
        && let Some(result) = operations.and_then(|ops| ops.result)
    {
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
        0 => connection_rows(&mut rows, view, operations, panel, m),
        1 => {
            if !view.connections.iter().any(|c| c.ready) {
                rows.push(Line::from(m.get("cc-no-ready")));
            }
            rows.push(Line::from(m.get("cloud-plan-help")));
            rows.push(Line::from(m.get("cloud-send-keys")));
            if let Some(plan) = plan {
                let (attempt, requested) =
                    crate::schedule_view::attempt_details(operations, plan, m);
                rows.push(Line::from(attempt));
                rows.extend(requested.map(Line::from));
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
    panel: &Panel,
    m: &Messages,
) {
    if let Some(uri) = operations.and_then(|o| o.cloud_authorization_uri.as_deref()) {
        let guide = if panel.access_help == Some("address-desktop") {
            "cc-oauth-desktop"
        } else {
            "cc-oauth-terminal"
        };
        rows.push(Line::from(m.get(guide)));
        rows.push(Line::from(crate::dashboard::safe_text(uri, 1024)));
    }
    if !view.connections.is_empty() {
        rows.push(Line::from(m.get("cc-advanced")));
    }
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
    let title = if matches!(form.operation, crate::Operation::ApplyRestore(_)) && !panel.managed() {
        "restore-review"
    } else {
        "op-dialog"
    };
    let block = crate::view::block(messages.get(title), ascii)
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
    if matches!(form.operation, crate::Operation::ApplyRestore(_)) && !panel.managed() {
        Button::new(
            messages.get("restore-back"),
            Action::Key(KeyCode::Esc),
            Variant::Secondary,
        )
        .draw(
            frame,
            Rect::new(
                inner.x,
                inner.bottom().saturating_sub(button_height),
                inner.width,
                button_height,
            ),
            scene,
        );
        return;
    }
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
    form_buttons(frame, form, messages, scene, buttons);
}

fn form_buttons(
    frame: &mut Frame,
    form: &crate::Form,
    messages: &Messages,
    scene: &mut crate::ui::Scene,
    buttons: Rect,
) {
    let enroll = matches!(
        form.operation,
        crate::Operation::Address(crate::AddressOperation::Enroll { .. })
    );
    let (confirm, cancel, settings) = if enroll {
        let [confirm, cancel, settings] =
            ratatui::layout::Layout::horizontal([ratatui::layout::Constraint::Fill(1); 3])
                .spacing(1)
                .areas(buttons);
        (confirm, cancel, Some(settings))
    } else {
        let [confirm, cancel] =
            ratatui::layout::Layout::horizontal([ratatui::layout::Constraint::Fill(1); 2])
                .spacing(1)
                .areas(buttons);
        (confirm, cancel, None)
    };
    if let Some(settings) = settings {
        Button::new(
            messages.get("ac-broker-settings"),
            Action::Key(KeyCode::F(2)),
            Variant::Secondary,
        )
        .draw(frame, settings, scene);
    }
    let (key, variant) = match form.operation {
        crate::Operation::Restore { .. } => ("restore-validate", Variant::Primary),
        crate::Operation::ApplyRestore(_) => ("restore-confirm", Variant::Danger),
        crate::Operation::Server(crate::ServerAction::Stop) => ("desk-stop", Variant::Danger),
        crate::Operation::BackupNow { .. } => ("desk-create", Variant::Primary),
        crate::Operation::CloudflareSetup { .. } => ("ac-publish", Variant::Primary),
        crate::Operation::Address(crate::AddressOperation::Withdraw) => {
            ("ac-withdraw", Variant::Danger)
        }
        _ => ("desk-confirm", Variant::Primary),
    };
    Button::new(messages.get(key), Action::Submit, variant)
        .available(if form.needs_distinct_hosts() {
            crate::ui::Availability::Disabled
        } else {
            crate::ui::Availability::Enabled
        })
        .draw(frame, confirm, scene);
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
    let destination = match &form.operation {
        crate::Operation::Cloud(crate::CloudOperation::SendGroup(targets, _)) => {
            header.push(row(
                messages,
                "cloud-target-page",
                Some(format!("{} / {}", form.target_page + 1, targets.len())),
            ));
            targets
                .get(form.target_page)
                .map(|t| (&t.connection, Some(&t.folder)))
        }
        crate::Operation::Cloud(crate::CloudOperation::Remote(target, cmd)) => {
            if let crate::RemoteCommand::Download(id) = cmd {
                header.push(Line::from(format!("elzevir-{}.age", text(id))));
            }
            Some((&target.connection, Some(&target.folder)))
        }
        crate::Operation::Cloud(crate::CloudOperation::Plan {
            id,
            connection,
            folder,
            ..
        }) if !connection.is_empty() => Some((connection, (!id.is_empty()).then_some(folder))),
        _ => None,
    };
    if let Some((connection, folder)) = destination {
        header.push(row(
            messages,
            "cc-destination-connection",
            Some(
                panel
                    .cloud_view()
                    .map_or_else(|| text(connection), |v| connection_name(v, connection)),
            ),
        ));
        if let Some(folder) = folder {
            let folder = messages.values("cloud-folder", &[("value", text(folder))]);
            for line in folder.chars().collect::<Vec<_>>().chunks(60) {
                header.push(Line::from(line.iter().collect::<String>()));
            }
        }
    }
    if let crate::Operation::Cloud(crate::CloudOperation::Plan { id, .. }) = &form.operation
        && !id.is_empty()
        && form
            .labels
            .iter()
            .zip(&form.values)
            .any(|(label, value)| *label == "op-time" && value.is_empty())
    {
        header.push(Line::from(messages.get("schedule-after-pause")));
    }
}

fn address_header(
    panel: &Panel,
    form: &crate::Form,
    messages: &Messages,
    header: &mut Vec<Line<'static>>,
) {
    if matches!(form.operation, crate::Operation::CloudflareSetup { .. }) {
        header.push(Line::from(messages.get("ac-confirm-publish")));
        header.push(Line::from(messages.get("ac-confirm-staff")));
        if form.needs_distinct_hosts() {
            header.push(Line::from(messages.get("ac-host-required")));
        }
    }
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
            header.push(Line::from(text(name)));
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

fn institution_header(
    panel: &Panel,
    form: &crate::Form,
    messages: &Messages,
    header: &mut Vec<Line<'static>>,
) {
    let crate::Mode::Live(state) = &panel.mode else {
        return;
    };
    let name = state
        .institution
        .name
        .as_deref()
        .map_or_else(|| messages.get("panel-no-name"), text);
    if matches!(
        form.operation,
        crate::Operation::Restore { .. } | crate::Operation::ApplyRestore(_)
    ) {
        let snapshot = crate::backup_center::sample(panel);
        let value = |v: Option<String>| v.unwrap_or_else(|| messages.get("panel-unknown"));
        header.push(row(messages, "restore-target", Some(name)));
        header.push(Line::from(
            messages.values(
                "restore-conditions",
                &[
                    (
                        "service",
                        messages.get(crate::restore_view::service_state(snapshot)),
                    ),
                    (
                        "sessions",
                        value(
                            snapshot
                                .and_then(|s| s.operations.sessions)
                                .map(|n| n.to_string()),
                        ),
                    ),
                    (
                        "space",
                        value(
                            snapshot
                                .and_then(|s| s.storage.available_bytes)
                                .map(crate::backup_center::bytes),
                        ),
                    ),
                ],
            ),
        ));
    } else {
        header.push(Line::from(name));
        header.push(row(
            messages,
            "op-sessions",
            state
                .snapshot
                .as_ref()
                .and_then(|s| s.operations.sessions)
                .map(|n| n.to_string()),
        ));
    }
}

fn form_header(panel: &Panel, form: &crate::Form, messages: &Messages) -> Vec<Line<'static>> {
    let mut header = vec![Line::from(messages.get(form.operation.key()))];
    institution_header(panel, form, messages, &mut header);
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
        if panel.managed() {
            header.push(Line::from(messages.get("restore-outage")));
        } else {
            header.push(Line::from(messages.get("restore-personal")));
            header.push(Line::from("elzevir backup restore --help"));
        }
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
        let mut input = Rect::new(area.x, y + 1, area.width, 1);
        if matches!(form.operation, crate::Operation::Restore { .. }) {
            let width = 19.min(input.width);
            input.width -= width;
            Button::new(
                messages.get(if index == 0 {
                    "restore-browse-archive"
                } else {
                    "restore-browse-key"
                }),
                Action::Key(KeyCode::F(if index == 0 { 3 } else { 4 })),
                Variant::Secondary,
            )
            .draw(frame, Rect::new(input.right(), y + 1, width, 1), scene);
        }
        if let Some(choices) = crate::operations::choices(key) {
            choice_buttons(frame, input, (index, value, choices), messages, scene);
        } else {
            let hover = scene.add(Rect::new(area.x, y, input.width, 2), Action::Field(index));
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
