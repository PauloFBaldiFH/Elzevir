// SPDX-License-Identifier: GPL-3.0-or-later
//! Configuration, observed topology and local evidence are distinct UI fields.

use crate::dashboard::{row, text};
use crate::{AddressBook, CheckResult, Institution, Messages, StaffAccess};
use ratatui::text::Line;

fn mode(messages: &Messages, value: Option<&StaffAccess>) -> String {
    messages.get(match value {
        None => "panel-unknown",
        Some(StaffAccess::SameAddress) => "panel-access-same",
        Some(StaffAccess::Hostname { .. }) => "panel-access-hostname",
        Some(StaffAccess::SeparateListener { .. }) => "panel-access-separate",
    })
}

fn check(messages: &Messages, result: CheckResult) -> String {
    messages.get(match result {
        CheckResult::Ok => "panel-component-ok",
        CheckResult::Failed => "panel-component-failed",
        CheckResult::NotRun => "panel-check-not-run",
        CheckResult::Unknown => "panel-check-unknown",
    })
}

fn details(rows: &mut Vec<Line<'static>>, value: Option<&StaffAccess>, messages: &Messages) {
    match value {
        Some(StaffAccess::Hostname {
            catalog_host,
            staff_host,
        }) => {
            rows.push(row(
                messages,
                "panel-access-catalog-host",
                Some(text(catalog_host)),
            ));
            rows.push(row(
                messages,
                "panel-access-staff-host",
                Some(text(staff_host)),
            ));
        }
        Some(StaffAccess::SeparateListener { listen, allow_ips }) => {
            rows.push(row(
                messages,
                "panel-access-staff-listen",
                Some(listen.to_string()),
            ));
            if allow_ips.is_empty() {
                rows.push(Line::from(messages.get("panel-access-no-ip-filter")));
            }
            for network in allow_ips {
                rows.push(row(
                    messages,
                    "panel-access-allowed-ip",
                    Some(text(network)),
                ));
            }
        }
        _ => {}
    }
}

pub(crate) fn rows(
    institution: &Institution,
    book: Option<&AddressBook>,
    pending: bool,
    messages: &Messages,
) -> Vec<Line<'static>> {
    let mut rows = vec![row(
        messages,
        "panel-access-configured",
        Some(mode(messages, institution.configured_access.as_ref())),
    )];
    details(&mut rows, institution.configured_access.as_ref(), messages);
    rows.push(row(
        messages,
        "panel-access-observed",
        Some(mode(messages, book.and_then(|b| b.observed_mode.as_ref()))),
    ));
    details(
        &mut rows,
        book.and_then(|b| b.observed_mode.as_ref()),
        messages,
    );
    rows.extend([
        row(
            messages,
            "panel-local-catalog",
            book.and_then(|b| b.catalog.as_deref()).map(text),
        ),
        row(
            messages,
            "panel-local-staff",
            book.and_then(|b| b.staff.as_deref()).map(text),
        ),
        row(
            messages,
            "panel-access-catalog-test",
            book.map(|b| check(messages, b.catalog_test)),
        ),
        row(
            messages,
            "panel-access-staff-test",
            book.map(|b| check(messages, b.staff_test)),
        ),
        row(
            messages,
            "panel-access-tested-at",
            book.and_then(|b| b.tested_at.as_deref()).map(text),
        ),
        Line::from(messages.get(if pending {
            "panel-access-pending"
        } else {
            "panel-access-actions"
        })),
        Line::from(messages.get("panel-access-local-scope")),
        Line::default(),
        Line::from(messages.get("panel-access-lan-candidates")),
    ]);
    let candidates = book.is_some_and(|b| !b.catalog_lan.is_empty() || !b.staff_lan.is_empty());
    if !candidates {
        rows.push(Line::from(messages.get("panel-access-no-lan")));
    }
    if let Some(book) = book {
        for url in &book.catalog_lan {
            rows.push(row(messages, "panel-access-lan-catalog", Some(text(url))));
        }
        for url in &book.staff_lan {
            rows.push(row(messages, "panel-access-lan-staff", Some(text(url))));
        }
    }
    rows.extend([
        Line::from(messages.get("panel-access-internet")),
        Line::default(),
        Line::from(messages.get("panel-access-alternatives")),
        Line::from(messages.get("panel-access-option-same")),
        Line::from(messages.get("panel-access-option-hostname")),
        Line::from(messages.get("panel-access-option-separate")),
        Line::from(messages.get("panel-access-config-change")),
    ]);
    rows
}
