// SPDX-License-Identifier: GPL-3.0-or-later
//! Read-only provider discovery. Choices carry the same bounded name rclone advertised.
use crate::{
    BackupPage, CloudOperation, Form, Operation, Operations, Panel, Section,
    backup_center::{Command, Controls, block, inner, line, pager, row, sample},
    cloud_view::worker_available,
    dashboard::text,
    ui::{Action, Button, Variant, theme},
};
use ratatui::{
    crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers},
    layout::{Constraint, Layout, Rect},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Name([u8; 64]);

impl Name {
    fn from_catalog(s: &str) -> Option<Self> {
        if s.is_empty() || !s.is_ascii() || s.contains('\0') {
            return None;
        }
        let mut bytes = [0; 64];
        bytes.get_mut(..s.len())?.copy_from_slice(s.as_bytes());
        Some(Self(bytes))
    }
    fn value(&self) -> &str {
        std::str::from_utf8(&self.0)
            .unwrap_or_default()
            .trim_end_matches('\0')
    }
}

pub(super) fn choose(panel: &mut Panel, name: Name) {
    if !worker_available(panel)
        || !sample(panel)
            .and_then(|s| s.operations.cloud.as_ref())
            .is_some_and(|v| v.providers.iter().any(|(n, _)| n == name.value()))
    {
        panel.access_notice = Some("cc-provider-unavailable");
        return;
    }
    panel.backups.page = BackupPage::Connections;
    panel.cloud_tab = 0;
    panel.backups.offset = 0;
    panel.form = Some(Form::new(Operation::Cloud(CloudOperation::Connect {
        name: String::new(),
        provider: name.value().into(),
    })));
}

pub(super) fn input(panel: &mut Panel, event: &Event) -> bool {
    if panel.section != Section::Backup
        || panel.backups.page != BackupPage::Providers
        || panel.form.is_some()
        || panel.palette
    {
        return false;
    }
    let query = &mut panel.backups.provider_query;
    match event {
        Event::Key(k)
            if k.kind != KeyEventKind::Release
                && !k
                    .modifiers
                    .intersects(KeyModifiers::ALT | KeyModifiers::CONTROL) =>
        {
            match k.code {
                KeyCode::Char(c) if !c.is_control() => {
                    if query.chars().count() < 64 {
                        query.push(c);
                    }
                }
                KeyCode::Backspace => {
                    query.pop();
                }
                KeyCode::Esc => panel.backups.page = BackupPage::Connections,
                _ => return false,
            }
        }
        Event::Paste(value) => {
            let count = 64_usize.saturating_sub(query.chars().count());
            query.extend(value.chars().filter(|c| !c.is_control()).take(count));
        }
        _ => return false,
    }
    panel.backups.offset = 0;
    true
}

pub(super) fn draw(ui: &mut Controls<'_, '_>, area: Rect, panel: &Panel, ops: Option<&Operations>) {
    block(ui.f, area, ui.m.get("cc-filter-help"));
    let r = inner(area);
    let [filter, clear] = Layout::horizontal([Constraint::Fill(1); 2])
        .spacing(1)
        .areas(row(r, 1, 1));
    let query = &panel.backups.provider_query;
    line(
        ui.f,
        filter,
        ui.m.values("cc-filter", &[("value", text(query))]),
        theme::TEXT,
        true,
    );
    ui.buttons(
        clear,
        &[(
            "cc-clear",
            Action::Backup(Command::ClearFilter),
            Variant::Quiet,
            !query.is_empty(),
        )],
    );
    let Some(view) = ops.and_then(|o| o.cloud.as_ref()) else {
        line(
            ui.f,
            row(r, 3, 1),
            ui.m.get("schedule-cloud-unknown"),
            theme::WARNING,
            false,
        );
        return;
    };
    let needle = query.to_lowercase();
    let matches = view
        .providers
        .iter()
        .filter(|(n, d)| n.to_lowercase().contains(&needle) || d.to_lowercase().contains(&needle))
        .collect::<Vec<_>>();
    let capacity = usize::from(r.height.saturating_sub(4));
    let start = panel
        .backups
        .offset
        .min(matches.len().saturating_sub(capacity));
    for (i, (name, desc)) in matches.iter().skip(start).take(capacity).enumerate() {
        if let Some(choice) = Name::from_catalog(name) {
            let button = Button::new(
                format!("{}  ·  {}", text(name), text(desc)),
                Action::Backup(Command::Provider(choice)),
                Variant::Secondary,
            );
            button.draw(ui.f, row(r, 3 + u16::try_from(i).unwrap_or(0), 1), ui.scene);
        }
    }
    if matches.is_empty() {
        line(
            ui.f,
            row(r, 3, 1),
            ui.m.get("cc-no-match"),
            theme::MUTED,
            false,
        );
    } else if matches.len() > capacity {
        pager(
            ui,
            row(r, r.height.saturating_sub(1), 1),
            start,
            matches.len(),
            capacity,
        );
    }
}
