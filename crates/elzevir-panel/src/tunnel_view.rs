// SPDX-License-Identifier: GPL-3.0-or-later
//! Public candidates and explicit evidence, kept separate from local interface tests.
use crate::dashboard::{row, text};
use crate::{Messages, Operations, PublicCheck, TunnelLog, TunnelState, TunnelTab};
use ratatui::text::Line;
fn check(messages: &Messages, value: PublicCheck) -> String {
    messages.get(match value {
        PublicCheck::NotRun => "panel-check-not-run",
        PublicCheck::Ok => "panel-component-ok",
        PublicCheck::Protected => "tunnel-protected",
        PublicCheck::Failed => "panel-component-failed",
        PublicCheck::Unknown => "panel-check-unknown",
    })
}
pub(crate) fn rows(
    operations: Option<&Operations>,
    tab: TunnelTab,
    messages: &Messages,
) -> Vec<Line<'static>> {
    let mut rows = Vec::new();
    if let Some(result) = operations.and_then(|operations| operations.result) {
        rows.push(Line::from(messages.get(result)));
    }
    rows.extend(domain_rows(operations, messages));
    rows.extend(remote_rows(operations, messages));
    if let Some(operations) = operations {
        for job in operations
            .jobs
            .iter()
            .filter(|job| {
                matches!(
                    job.action,
                    "op-cloudflare-login"
                        | "op-cloudflare-setup"
                        | "op-tunnel-test"
                        | "broker-operation"
                )
            })
            .take(3)
        {
            rows.push(Line::from(format!(
                "{}: {}",
                messages.get(job.action),
                messages.get(job.state)
            )));
        }
    }
    match operations.map(|operations| &operations.tunnel) {
        Some(TunnelState::Configured(tunnel)) => {
            rows.push(row(messages, "tunnel-catalog", Some(text(&tunnel.catalog))));
            rows.push(row(messages, "tunnel-staff", Some(text(&tunnel.staff))));
            rows.push(Line::from(messages.get(if tab == TunnelTab::Test {
                "tunnel-tab-test"
            } else {
                "tunnel-tab-log"
            })));
            if tab == TunnelTab::Test {
                rows.push(row(
                    messages,
                    "tunnel-catalog-check",
                    Some(check(messages, tunnel.catalog_check)),
                ));
                rows.push(row(
                    messages,
                    "tunnel-staff-check",
                    Some(check(messages, tunnel.staff_check)),
                ));
                rows.push(row(
                    messages,
                    "tunnel-tested-at",
                    tunnel.tested_at.as_deref().map(text),
                ));
            } else {
                match &tunnel.logs {
                    TunnelLog::Unavailable => {
                        rows.push(Line::from(messages.get("tunnel-log-unavailable")));
                    }
                    TunnelLog::Ready { events, omitted } => {
                        if events.is_empty() {
                            rows.push(Line::from(messages.get("tunnel-log-empty")));
                        }
                        for event in events {
                            rows.push(Line::from(messages.get(event)));
                        }
                        rows.push(row(
                            messages,
                            "tunnel-log-omitted",
                            Some(omitted.to_string()),
                        ));
                    }
                }
            }
        }
        Some(TunnelState::NotConfigured) => {
            rows.push(Line::from(messages.get("tunnel-not-configured")));
        }
        _ => {
            rows.push(Line::from(messages.get("tunnel-unavailable")));
        }
    }
    rows.push(Line::default());
    rows
}

fn remote_rows(operations: Option<&Operations>, messages: &Messages) -> Vec<Line<'static>> {
    let mut rows = Vec::new();
    if let Some(remote) = operations.and_then(|operations| operations.remote_address.as_ref()) {
        rows.push(row(messages, "broker-observed", Some(text(&remote.at))));
        if operations
            .and_then(|operations| operations.domain.as_ref())
            .is_none_or(|domain| domain.domain.is_empty())
        {
            rows.push(row(
                messages,
                "broker-listed",
                Some(messages.get(if remote.listed {
                    "broker-on"
                } else {
                    "broker-off"
                })),
            ));
        }
        rows.push(row(
            messages,
            "broker-remote",
            Some(messages.get(if remote.staff_remote {
                "broker-on"
            } else {
                "broker-off"
            })),
        ));
    }
    rows
}

fn domain_rows(operations: Option<&Operations>, messages: &Messages) -> Vec<Line<'static>> {
    let mut rows = vec![Line::from(messages.get("domain-title"))];
    if let Some(domain) = operations.and_then(|operations| operations.domain.as_ref()) {
        rows.push(row(
            messages,
            "domain-broker",
            Some(text(&domain.broker_url)),
        ));
        rows.push(row(
            messages,
            "domain-phase",
            Some(messages.get(domain.phase.key())),
        ));
        rows.push(row(
            messages,
            "domain-connector",
            Some(messages.get(domain.connector.key())),
        ));
        if let Some(status) = domain.remote_status {
            rows.push(row(
                messages,
                "domain-remote-status",
                Some(messages.get(status)),
            ));
        }
        if let Some(host) = &domain.catalog {
            rows.push(row(
                messages,
                "domain-assigned-catalog",
                Some(text(&format!("https://{host}"))),
            ));
        }
        if let Some(host) = &domain.staff {
            rows.push(row(
                messages,
                "domain-assigned-staff",
                Some(text(&format!("https://{host}"))),
            ));
        }
        if !domain.staff_allow_cidrs.is_empty() {
            rows.push(row(
                messages,
                "domain-networks",
                Some(text(&domain.staff_allow_cidrs.join(", "))),
            ));
        }
        if let Some(failure) = domain.failure {
            rows.push(Line::from(messages.get(failure)));
        }
    } else {
        rows.push(Line::from(messages.get("domain-unavailable")));
    }
    rows
}
