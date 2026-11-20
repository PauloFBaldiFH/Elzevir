// SPDX-License-Identifier: GPL-3.0-or-later
//! The staff header (interface §22): one registry of search scopes and "New" entries, the
//! header fragment every staff screen loads, and the search dispatch behind it.
//!
//! Features from later roadmaps join as registry entries once their screens exist; until then
//! they have no entry, so nothing on the page points to a screen that is not there.

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use elzevir_app::staff::Permission;
use elzevir_web_templates::{BarLink, BarScope, StaffBar};
use serde::Deserialize;

use crate::session::ActiveSession;
use crate::staff::current_session;
use crate::{AppState, render};

/// Who may use an entry, judged on the server for the header and again for the dispatch.
type Allowed = fn(&AppState, &ActiveSession) -> bool;

/// One search scope: what the operator types and the screen that answers.
struct ScopeEntry {
    /// The value of `escopo`.
    key: &'static str,
    label_key: &'static str,
    hint_key: &'static str,
    icon: &'static str,
    tone: &'static str,
    /// Letter used with Alt; the drafts' letters A, P, R, I and L stay reserved (§22 item 6).
    shortcut: char,
    /// Path of the screen that answers, without the query.
    target: &'static str,
    /// Name of the query parameter that screen reads.
    parameter: &'static str,
    /// Whether it acts on an Item instead of searching (the draft's "operations").
    operation: bool,
    allowed: Allowed,
}

/// One entry of the "New" menu.
struct NewEntry {
    label_key: &'static str,
    icon: &'static str,
    tone: &'static str,
    href: &'static str,
    allowed: Allowed,
}

/// The search scopes, in the order the header shows them.
const SCOPES: [ScopeEntry; 3] = [
    ScopeEntry {
        key: "leitores",
        label_key: "bar-scope-patrons",
        hint_key: "bar-hint-patrons",
        icon: "bar-patrons",
        tone: "patrons",
        shortcut: 'u',
        target: "/equipe/leitores",
        parameter: "q",
        operation: false,
        allowed: sees_patrons,
    },
    ScopeEntry {
        key: "acervo",
        label_key: "bar-scope-catalog",
        hint_key: "bar-hint-catalog",
        icon: "bar-catalog",
        tone: "catalog",
        shortcut: 'c',
        target: "/equipe/acervo",
        parameter: "q",
        operation: false,
        allowed: any_staff,
    },
    ScopeEntry {
        key: "devolucao",
        label_key: "bar-scope-checkin",
        hint_key: "bar-hint-checkin",
        icon: "bar-checkin",
        tone: "circulation",
        shortcut: 'd',
        // The desk opens with the barcode filled in, so the desk's branch is still chosen there.
        target: "/equipe/circulacao/devolucao",
        parameter: "codigo",
        operation: true,
        allowed: circulates,
    },
];

/// The "New" menu, in the order it shows.
const NEW_ENTRIES: [NewEntry; 2] = [
    NewEntry {
        label_key: "bar-new-record",
        icon: "bar-catalog",
        tone: "catalog",
        href: "/equipe/acervo/nova",
        allowed: catalogues,
    },
    NewEntry {
        label_key: "bar-new-patron",
        icon: "bar-patrons",
        tone: "patrons",
        href: "/equipe/leitores/novo",
        allowed: manages_patrons,
    },
];

fn any_staff(_: &AppState, _: &ActiveSession) -> bool {
    true
}

fn sees_patrons(state: &AppState, session: &ActiveSession) -> bool {
    state.patrons.is_some()
        && (session.identity.can(Permission::Circulation)
            || session.identity.can(Permission::Patrons))
}

fn circulates(state: &AppState, session: &ActiveSession) -> bool {
    state.patrons.is_some()
        && state.loans.is_some()
        && session.identity.can(Permission::Circulation)
}

fn catalogues(_: &AppState, session: &ActiveSession) -> bool {
    session.identity.can(Permission::Cataloguing)
}

fn manages_patrons(state: &AppState, session: &ActiveSession) -> bool {
    state.patrons.is_some() && session.identity.can(Permission::Patrons)
}

/// The query of the header and of its dispatch.
#[derive(Deserialize)]
pub(crate) struct BarParams {
    #[serde(default)]
    escopo: String,
    #[serde(default)]
    q: String,
}

/// `GET /equipe/barra?escopo=`: the header fragment, with only what this operator may use.
pub(crate) async fn bar(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<BarParams>,
) -> Response {
    let Some(session) = current_session(&state, &headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let permitted: Vec<&ScopeEntry> = SCOPES
        .iter()
        .filter(|entry| (entry.allowed)(&state, &session))
        .collect();
    // A screen without a scope of its own, or with one this operator lacks, starts on the
    // first permitted scope (§22 item 7).
    let chosen = permitted
        .iter()
        .find(|entry| entry.key == params.escopo)
        .or_else(|| permitted.first())
        .map(|entry| entry.key);
    let mut after_search = false;
    let scopes: Vec<BarScope> = permitted
        .into_iter()
        .map(|entry| {
            let divider_before = entry.operation && after_search;
            after_search = !entry.operation;
            (entry, divider_before)
        })
        .map(|(entry, divider_before)| BarScope {
            key: entry.key,
            label_key: entry.label_key,
            hint_key: entry.hint_key,
            icon: entry.icon,
            tone: entry.tone,
            shortcut: entry.shortcut,
            selected: Some(entry.key) == chosen,
            operation: entry.operation,
            divider_before,
        })
        .collect();
    let new_entries = NEW_ENTRIES
        .iter()
        .filter(|entry| (entry.allowed)(&state, &session))
        .map(|entry| BarLink {
            label_key: entry.label_key,
            icon: entry.icon,
            tone: entry.tone,
            href: entry.href,
        })
        .collect();
    render(&StaffBar {
        t: &state.inner.i18n,
        login: &session.identity.login,
        csrf: &session.csrf,
        scopes,
        new_entries,
    })
}

/// `GET /equipe/busca?escopo=&q=`: sends the search to the screen of its scope. An unknown
/// scope is a bad request; a scope this operator may not use is refused.
pub(crate) async fn search(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<BarParams>,
) -> Response {
    let Some(session) = current_session(&state, &headers) else {
        return Redirect::to("/equipe/login").into_response();
    };
    let Some(entry) = SCOPES.iter().find(|entry| entry.key == params.escopo) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if !(entry.allowed)(&state, &session) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let query = params.q.trim();
    if query.is_empty() {
        return Redirect::to(entry.target).into_response();
    }
    Redirect::to(&format!(
        "{}?{}={}",
        entry.target,
        entry.parameter,
        query_value(query)
    ))
    .into_response()
}

/// `value` percent-encoded for a query string: everything but the unreserved characters of
/// RFC 3986 is escaped, so the redirect can never carry a second parameter or a header break.
fn query_value(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            for digit in [byte >> 4, byte & 0x0F] {
                encoded
                    .extend(char::from_digit(u32::from(digit), 16).map(|c| c.to_ascii_uppercase()));
            }
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_keys_and_shortcuts_are_unique_and_not_reserved() {
        let reserved = ['a', 'p', 'r', 'i', 'l', 'n'];
        for (index, entry) in SCOPES.iter().enumerate() {
            assert!(!reserved.contains(&entry.shortcut), "{}", entry.key);
            for other in &SCOPES[index + 1..] {
                assert_ne!(entry.key, other.key);
                assert_ne!(entry.shortcut, other.shortcut);
            }
        }
    }

    #[test]
    fn query_values_are_escaped() {
        assert_eq!(query_value("Dom Casmurro"), "Dom%20Casmurro");
        assert_eq!(query_value("a&escopo=x\r\n"), "a%26escopo%3Dx%0D%0A");
        assert_eq!(query_value("ção"), "%C3%A7%C3%A3o");
        assert_eq!(query_value("978-85_x.~"), "978-85_x.~");
    }
}
