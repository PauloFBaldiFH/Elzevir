// SPDX-License-Identifier: GPL-3.0-or-later
//! Connected dashboard rendering. All values arrive as observations.

use crate::{Component, Details, LiveState, Messages, Panel, Section, ServerStatus};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::Line,
    widgets::{Paragraph, Wrap},
};

pub(crate) fn text(value: &str) -> String {
    safe_text(value, 512)
}

fn safe_text(value: &str, limit: usize) -> String {
    value
        .chars()
        .filter(|c| {
            !c.is_control() && !matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .take(limit)
        .collect()
}

pub(crate) fn row(messages: &Messages, key: &str, value: Option<String>) -> Line<'static> {
    Line::from(messages.values(
        key,
        &[(
            "value",
            value.unwrap_or_else(|| messages.get("panel-unknown")),
        )],
    ))
}

pub(crate) fn component(value: Component, messages: &Messages) -> String {
    messages.get(match value {
        Component::Ok => "panel-component-ok",
        Component::Starting => "panel-component-starting",
        Component::Failed => "panel-component-failed",
    })
}

pub(crate) fn draw(
    frame: &mut Frame,
    panel: &Panel,
    state: &LiveState,
    messages: &Messages,
    work: Rect,
    context: Rect,
    ascii: bool,
) {
    let expired = state.snapshot.is_some() && state.age_seconds > 5;
    let snapshot = state.snapshot.as_ref().filter(|_| !expired);
    let (key, color) = match snapshot.map(|sample| &sample.status) {
        _ if expired => ("panel-state-stale", Color::Gray),
        None => ("panel-state-loading", Color::Gray),
        Some(ServerStatus::Running(_)) => ("panel-state-running", Color::Green),
        Some(ServerStatus::Degraded(_)) => ("panel-state-degraded", Color::Yellow),
        Some(ServerStatus::Stopped) => ("panel-state-stopped", Color::Gray),
        Some(ServerStatus::Unavailable) => ("panel-state-unavailable", Color::Gray),
    };
    let details = snapshot.and_then(|sample| match &sample.status {
        ServerStatus::Running(details) | ServerStatus::Degraded(details) => Some(details),
        _ => None,
    });
    let mut rows = vec![
        Line::from(messages.get(key)).style(Style::default().fg(color)),
        Line::default(),
    ];
    if panel.section == Section::Dashboard {
        rows.extend(readings(snapshot, details, messages));
    } else if panel.section == Section::Diagnostics {
        rows.extend(crate::diagnostic_view::rows(
            snapshot,
            details,
            panel.diagnostics.pending,
            messages,
        ));
    } else if panel.section == Section::Addresses {
        rows.extend(address_rows(panel, snapshot, state, messages));
    } else if matches!(
        panel.section,
        Section::Backup | Section::Restart | Section::Updates
    ) {
        rows.extend(crate::operation_view::rows(
            snapshot,
            panel.diagnostics.pending,
            messages,
        ));
    } else {
        rows.push(Line::from(messages.get("panel-live-unavailable-section")));
    }
    frame.render_widget(
        Paragraph::new(rows)
            .wrap(Wrap { trim: true })
            .scroll((panel.scroll, 0))
            .block(crate::view::block(messages.get(panel.section.key()), ascii)),
        work,
    );
    let name = state
        .institution
        .name
        .as_deref()
        .map_or_else(|| messages.get("panel-no-name"), text);
    let rows = vec![
        Line::from(name),
        Line::default(),
        Line::from(messages.get("panel-configured-listener")),
        Line::from(
            state
                .institution
                .configured_listen
                .map_or_else(|| messages.get("panel-unknown"), |a| a.to_string()),
        ),
        Line::default(),
        Line::from(messages.get("panel-observed-listener")),
        Line::from(
            details
                .and_then(|d| d.listen)
                .map_or_else(|| messages.get("panel-unknown"), |a| a.to_string()),
        ),
        Line::default(),
        if state.snapshot.is_some() {
            row(messages, "panel-age", Some(state.age_seconds.to_string()))
        } else {
            Line::from(messages.get("panel-state-loading"))
        },
        Line::default(),
        Line::from(messages.get("panel-read-only")),
    ];
    frame.render_widget(
        Paragraph::new(rows)
            .wrap(Wrap { trim: true })
            .block(crate::view::block(messages.get("panel-context"), ascii)),
        context,
    );
}

fn address_rows(
    panel: &Panel,
    snapshot: Option<&crate::Snapshot>,
    state: &LiveState,
    messages: &Messages,
) -> Vec<Line<'static>> {
    let mut rows = vec![Line::from(messages.get("op-cloudflare-help"))];
    if let Some(sample) = snapshot {
        for uri in &sample.operations.authorization_uris {
            rows.push(row(
                messages,
                "op-authorize-uri",
                Some(safe_text(uri, 2048)),
            ));
        }
    }
    rows.extend(crate::tunnel_view::rows(
        snapshot.map(|sample| &sample.operations),
        panel.tunnel_tab,
        messages,
    ));
    rows.extend(crate::address_view::rows(
        &state.institution,
        snapshot.map(|sample| &sample.addresses),
        panel.diagnostics.pending,
        messages,
    ));
    rows
}

fn readings(
    snapshot: Option<&crate::Snapshot>,
    details: Option<&Details>,
    messages: &Messages,
) -> Vec<Line<'static>> {
    let host = snapshot.map(|sample| &sample.host);
    let storage = snapshot.map(|sample| &sample.storage);
    vec![
        row(messages, "panel-version", details.map(|d| text(&d.version))),
        row(messages, "panel-pid", details.map(|d| d.pid.to_string())),
        row(
            messages,
            "panel-uptime",
            details.map(|d| d.uptime_seconds.to_string()),
        ),
        row(
            messages,
            "panel-database-state",
            details.map(|d| component(d.database, messages)),
        ),
        row(
            messages,
            "panel-indexer-state",
            details.map(|d| component(d.indexer, messages)),
        ),
        Line::default(),
        row(
            messages,
            "panel-host-cpu",
            host.and_then(|h| h.cpu_percent)
                .filter(|n| n.is_finite() && (0.0..=100.0).contains(n))
                .map(|n| format!("{n:.1}")),
        ),
        row(
            messages,
            "panel-host-memory",
            host.and_then(|h| h.memory_bytes)
                .map(|(used, total)| format!("{} / {}", used / 1_048_576, total / 1_048_576)),
        ),
        row(
            messages,
            "panel-space",
            storage
                .and_then(|s| s.available_bytes)
                .map(|n| n.to_string()),
        ),
        row(
            messages,
            "panel-database-size",
            storage
                .and_then(|s| s.database_bytes)
                .map(|n| n.to_string()),
        ),
        row(
            messages,
            "panel-wal-size",
            storage.and_then(|s| s.wal_bytes).map(|n| n.to_string()),
        ),
        Line::default(),
        row(
            messages,
            "panel-local-catalog",
            snapshot
                .and_then(|s| s.addresses.catalog.as_deref())
                .map(text),
        ),
        row(
            messages,
            "panel-local-staff",
            snapshot
                .and_then(|s| s.addresses.staff.as_deref())
                .map(text),
        ),
        Line::from(messages.get(
            if snapshot.is_some_and(|s| s.addresses.tested_at.is_some()) {
                "panel-access-tests-reference"
            } else {
                "panel-binding-only"
            },
        )),
        Line::from(messages.get("panel-lan-unknown")),
        Line::from(messages.get("panel-internet-unknown")),
        Line::from(messages.get("panel-backup-unknown")),
    ]
}
