// SPDX-License-Identifier: GPL-3.0-or-later
//! Diagnostic presentation over safe typed observations only.

use crate::dashboard::{component, row, text};
use crate::{CheckResult, Details, ExportState, LogState, Messages, Snapshot};
use ratatui::text::Line;

fn check(messages: &Messages, value: CheckResult) -> String {
    messages.get(match value {
        CheckResult::NotRun => "panel-check-not-run",
        CheckResult::Ok => "panel-component-ok",
        CheckResult::Failed => "panel-component-failed",
        CheckResult::Unknown => "panel-check-unknown",
    })
}

pub(crate) fn rows(
    snapshot: Option<&Snapshot>,
    details: Option<&Details>,
    pending: bool,
    messages: &Messages,
) -> Vec<Line<'static>> {
    let values = snapshot.map(|s| &s.diagnostics);
    let mut rows = vec![
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
        row(
            messages,
            "panel-quick-check",
            values.map(|v| check(messages, v.quick_check)),
        ),
        row(
            messages,
            "panel-catalog-http",
            values.map(|v| check(messages, v.catalog_http)),
        ),
        row(
            messages,
            "panel-space",
            snapshot
                .and_then(|s| s.storage.available_bytes)
                .map(|n| n.to_string()),
        ),
        row(
            messages,
            "panel-space-state",
            Some(check(
                messages,
                CheckResult::available_space(snapshot.and_then(|s| s.storage.available_bytes)),
            )),
        ),
        row(
            messages,
            "panel-local-volume",
            values.map(|v| check(messages, v.local_volume)),
        ),
        row(
            messages,
            "panel-directory-access",
            values.map(|v| check(messages, v.directory_access)),
        ),
        row(
            messages,
            "panel-verified-at",
            values.and_then(|v| v.verified_at.as_deref()).map(text),
        ),
        Line::from(messages.get("panel-clock-unchecked")),
        Line::from(messages.get("panel-tunnel-unchecked")),
        Line::from(messages.get("panel-release-unchecked")),
        Line::from(messages.get(if pending {
            "panel-diagnostic-pending"
        } else {
            "panel-diagnostic-actions"
        })),
        match values.map(|v| &v.export) {
            Some(ExportState::Saved(path)) => row(messages, "panel-export-saved", Some(text(path))),
            Some(ExportState::Failed) => Line::from(messages.get("panel-export-failed")),
            _ => Line::from(messages.get("panel-export-none")),
        },
        Line::default(),
    ];
    let Some(logs) = values.map(|v| &v.logs) else {
        return rows;
    };
    rows.push(Line::from(messages.get(match logs.state {
        LogState::Missing => "panel-logs-missing",
        LogState::Unavailable => "panel-logs-unavailable",
        LogState::Ready => "panel-logs-live",
    })));
    if logs.limited {
        rows.push(Line::from(messages.get("panel-logs-limited")));
    }
    if logs.omitted > 0 {
        rows.push(row(
            messages,
            "panel-logs-omitted",
            Some(logs.omitted.to_string()),
        ));
    }
    if logs.state == LogState::Ready && logs.entries.is_empty() {
        rows.push(Line::from(messages.get("panel-logs-empty")));
    }
    for entry in &logs.entries {
        let event = messages.get(&format!(
            "panel-log-{}",
            entry.event.code().replace('.', "-")
        ));
        rows.push(Line::from(format!(
            "{} {} {event}",
            text(&entry.timestamp),
            entry.event.level()
        )));
    }
    rows
}
