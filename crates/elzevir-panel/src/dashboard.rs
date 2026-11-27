// SPDX-License-Identifier: GPL-3.0-or-later
//! Connected dashboard rendering. All values arrive as observations.

use crate::{Component, LiveState, Messages, Panel, Section, ServerStatus};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::Line,
    widgets::{Paragraph, Wrap},
};

pub(crate) fn text(value: &str) -> String {
    safe_text(value, 512)
}

pub(crate) fn safe_text(value: &str, limit: usize) -> String {
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
        _ if expired => ("panel-state-stale", crate::ui::theme::MUTED),
        None => ("panel-state-loading", crate::ui::theme::MUTED),
        Some(ServerStatus::Running(_)) => ("panel-state-running", crate::ui::theme::SUCCESS),
        Some(ServerStatus::Degraded(_)) => ("panel-state-degraded", crate::ui::theme::WARNING),
        Some(ServerStatus::Stopped) => ("panel-state-stopped", crate::ui::theme::MUTED),
        Some(ServerStatus::Unavailable) => ("panel-state-unavailable", crate::ui::theme::MUTED),
    };
    let details = snapshot.and_then(|sample| match &sample.status {
        ServerStatus::Running(details) | ServerStatus::Degraded(details) => Some(details),
        _ => None,
    });
    let mut rows = vec![
        Line::from(messages.get(key)).style(Style::default().fg(color)),
        Line::default(),
    ];
    if panel.section == Section::Diagnostics {
        rows.extend(crate::diagnostic_view::rows(
            snapshot,
            details,
            panel.diagnostics.pending,
            messages,
        ));
    } else if panel.section == Section::Addresses {
        rows.extend(address_rows(panel, snapshot, state, messages));
    } else if matches!(panel.section, Section::Backup | Section::Updates) {
        rows.extend(crate::operation_view::rows(snapshot, panel, messages));
    } else if panel.section == Section::Settings {
        rows.push(row(
            messages,
            "settings-name",
            state.institution.name.as_deref().map(text),
        ));
        rows.push(row(
            messages,
            "home-bind",
            state.institution.configured_listen.map(|a| a.to_string()),
        ));
        rows.push(row(
            messages,
            "settings-language",
            state.institution.language.as_deref().map(text),
        ));
        rows.push(Line::from(messages.get("settings-edit-help")));
    } else if panel.section == Section::Restart {
        // The service view draws its state and buttons over this block.
        rows.clear();
    } else {
        rows.push(Line::from(messages.get("panel-live-unavailable-section")));
    }
    frame.render_widget(
        Paragraph::new(rows)
            .wrap(Wrap { trim: true })
            .scroll((panel.scroll, 0))
            .block(
                crate::view::block(messages.get(panel.section.key()), ascii)
                    .border_style(Style::new().fg(crate::ui::theme::ACCENT)),
            ),
        work,
    );
    draw_context(frame, context, state, details, messages, ascii);
}

fn draw_context(
    frame: &mut Frame,
    context: Rect,
    state: &LiveState,
    details: Option<&crate::Details>,
    messages: &Messages,
    ascii: bool,
) {
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
    use crate::TunnelTab::{Addresses, Log, Manage, Test};
    let tabs = [
        (Addresses, "panel-addresses"),
        (Manage, "address-manage"),
        (Test, "address-test"),
        (Log, "address-log"),
    ];
    let titles = tabs
        .iter()
        .map(|(tab, key)| {
            let title = messages.get(key);
            if panel.tunnel_tab == *tab {
                format!("[{title}]")
            } else {
                title
            }
        })
        .collect::<Vec<_>>()
        .join(" | ");
    let mut rows = vec![
        Line::from(titles),
        Line::from(messages.get("address-navigation")),
    ];
    for key in [panel.access_notice, panel.access_help]
        .into_iter()
        .flatten()
    {
        rows.push(Line::from(messages.get(key)));
    }
    if panel.tunnel_tab == Manage {
        for key in [
            "address-enrollment",
            "broker-help",
            "address-connector-controls",
            "op-cloudflare-help",
        ] {
            rows.push(Line::from(messages.get(key)));
        }
    }
    if let Some(sample) = snapshot {
        for uri in &sample.operations.authorization_uris {
            rows.push(row(
                messages,
                "op-authorize-uri",
                Some(safe_text(uri, 2048)),
            ));
        }
    }
    if panel.tunnel_tab != crate::TunnelTab::Share {
        rows.extend(crate::tunnel_view::rows(
            snapshot.map(|sample| &sample.operations),
            panel.tunnel_tab,
            messages,
        ));
    }
    if matches!(panel.tunnel_tab, Addresses | Test) {
        rows.extend(crate::address_view::rows(
            &state.institution,
            snapshot.map(|sample| &sample.addresses),
            panel.diagnostics.pending,
            messages,
        ));
    }
    rows
}
