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
use elzevir_web_templates::{BarLink, BarScope, ScopeKind, StaffBar};
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
    /// Letter used with Alt (§22 item 6).
    shortcut: char,
    /// Path of the screen that answers and the query parameter it reads; `None` while the
    /// screen does not exist yet, so the scope shows as a disabled placeholder.
    target: Option<(&'static str, &'static str)>,
    /// Whether it acts on an Item instead of searching (the draft's "operations").
    operation: bool,
    allowed: Allowed,
}

/// One link of the header, the drawer or the home: a module, a tool or a "New" entry.
struct LinkEntry {
    label_key: &'static str,
    icon: &'static str,
    tone: &'static str,
    /// Where it goes; `None` while the screen does not exist yet (a placeholder).
    href: Option<&'static str>,
    allowed: Allowed,
}

const fn link(
    label_key: &'static str,
    icon: &'static str,
    tone: &'static str,
    href: Option<&'static str>,
    allowed: Allowed,
) -> LinkEntry {
    LinkEntry {
        label_key,
        icon,
        tone,
        href,
        allowed,
    }
}

/// The search scopes, in the order of the home-v11 draft.
const SCOPES: [ScopeEntry; 6] = [
    ScopeEntry {
        key: "leitores",
        label_key: "bar-scope-patrons",
        hint_key: "bar-hint-patrons",
        icon: "bar-patrons",
        tone: "patrons",
        shortcut: 'u',
        target: Some(("/equipe/leitores", "q")),
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
        target: Some(("/equipe/acervo", "q")),
        operation: false,
        allowed: any_staff,
    },
    ScopeEntry {
        key: "autoridades",
        label_key: "bar-scope-authorities",
        hint_key: "bar-hint-authorities",
        icon: "mod-authorities",
        tone: "catalog",
        shortcut: 'a',
        target: None,
        operation: false,
        allowed: any_staff,
    },
    ScopeEntry {
        key: "repositorios",
        label_key: "bar-scope-repositories",
        hint_key: "bar-hint-repositories",
        icon: "mod-repositories",
        tone: "patrons",
        shortcut: 'p',
        target: None,
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
        target: Some(("/equipe/circulacao/devolucao", "codigo")),
        operation: true,
        allowed: circulates,
    },
    ScopeEntry {
        key: "renovacao",
        label_key: "bar-scope-renewal",
        hint_key: "bar-hint-renewal",
        icon: "mod-renew",
        tone: "circulation",
        shortcut: 'r',
        target: None,
        operation: true,
        allowed: any_staff,
    },
];

/// The "New" menu, in the draft's order.
const NEW_ENTRIES: [LinkEntry; 5] = [
    link(
        "bar-new-record",
        "bar-catalog",
        "catalog",
        Some("/equipe/acervo/nova"),
        catalogues,
    ),
    link(
        "bar-new-patron",
        "bar-patrons",
        "patrons",
        Some("/equipe/leitores/novo"),
        manages_patrons,
    ),
    link(
        "bar-new-authority",
        "mod-authorities",
        "catalog",
        None,
        any_staff,
    ),
    link(
        "bar-new-repository",
        "mod-repositories",
        "patrons",
        None,
        any_staff,
    ),
    link(
        "bar-new-subscription",
        "mod-serials",
        "circulation",
        None,
        any_staff,
    ),
];

/// The main modules of the home and the drawer, in the draft's order and tones.
const MODULES: [LinkEntry; 10] = [
    link(
        "module-circulation",
        "bar-circulation",
        "circulation",
        Some("/equipe/circulacao/devolucao"),
        circulates,
    ),
    link(
        "module-patrons",
        "bar-patrons",
        "patrons",
        Some("/equipe/leitores"),
        sees_patrons,
    ),
    link(
        "module-cataloguing",
        "bar-catalog",
        "catalog",
        Some("/equipe/acervo"),
        any_staff,
    ),
    link(
        "module-authorities",
        "mod-authorities",
        "catalog",
        None,
        any_staff,
    ),
    link(
        "module-repositories",
        "mod-repositories",
        "patrons",
        None,
        any_staff,
    ),
    link(
        "desk-title",
        "mod-repositories",
        "catalog",
        Some("/equipe/repositorio"),
        repository_staff,
    ),
    link(
        "module-serials",
        "mod-serials",
        "circulation",
        None,
        any_staff,
    ),
    link("module-reports", "mod-reports", "admin", None, any_staff),
    link("module-tools", "mod-tools", "admin", None, any_staff),
    link(
        "module-administration",
        "mod-rules",
        "admin",
        Some("/equipe/admin"),
        administers,
    ),
];

/// Screens that exist today, listed in the drawer under the modules until each module page
/// gathers its own.
const TOOLS: [LinkEntry; 7] = [
    link(
        "tool-import",
        "mod-import",
        "catalog",
        Some("/equipe/acervo/importar"),
        imports,
    ),
    link(
        "tool-inventory",
        "mod-tools",
        "catalog",
        Some("/equipe/acervo/inventario"),
        catalogues,
    ),
    link(
        "tool-printed-notices",
        "mod-notices",
        "circulation",
        Some("/equipe/circulacao/avisos"),
        circulates,
    ),
    link(
        "tool-registries",
        "mod-registries",
        "admin",
        Some("/equipe/admin/cadastros"),
        catalogues,
    ),
    link(
        "tool-rules",
        "mod-admin",
        "admin",
        Some("/equipe/admin/regras"),
        administers,
    ),
    link(
        "tool-notices",
        "mod-bell",
        "admin",
        Some("/equipe/admin/avisos"),
        administers,
    ),
    link(
        "tool-calendar",
        "mod-calendar",
        "admin",
        Some("/equipe/admin/calendario"),
        administers,
    ),
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

fn repository_staff(state: &AppState, session: &ActiveSession) -> bool {
    state.desk.is_some() && catalogues(state, session)
}

fn imports(state: &AppState, session: &ActiveSession) -> bool {
    state.imports.is_some() && session.identity.can(Permission::Cataloguing)
}

fn administers(_: &AppState, session: &ActiveSession) -> bool {
    session.identity.can(Permission::Administration)
}

fn manages_patrons(state: &AppState, session: &ActiveSession) -> bool {
    state.patrons.is_some() && session.identity.can(Permission::Patrons)
}

/// The entries this operator may use, as the templates draw them. Placeholders are open to
/// everyone, since they lead nowhere yet.
fn links(entries: &[LinkEntry], state: &AppState, session: &ActiveSession) -> Vec<BarLink> {
    entries
        .iter()
        .filter(|entry| (entry.allowed)(state, session))
        .map(|entry| BarLink {
            label_key: entry.label_key,
            icon: entry.icon,
            tone: entry.tone,
            href: entry.href,
        })
        .collect()
}

/// The modules of the staff home for this operator (§22 item 11).
pub(crate) fn home_modules(state: &AppState, session: &ActiveSession) -> Vec<BarLink> {
    links(&MODULES, state, session)
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
    // first permitted scope that works (§22 item 7).
    let chosen = permitted
        .iter()
        .filter(|entry| entry.target.is_some())
        .find(|entry| entry.key == params.escopo)
        .or_else(|| permitted.iter().find(|entry| entry.target.is_some()))
        .map(|entry| entry.key);
    let mut after_search = false;
    let scopes: Vec<BarScope> = permitted
        .into_iter()
        .map(|entry| {
            let kind = match (entry.operation, after_search) {
                (false, _) => ScopeKind::Search,
                (true, true) => ScopeKind::FirstOperation,
                (true, false) => ScopeKind::Operation,
            };
            after_search = !entry.operation;
            BarScope {
                key: entry.key,
                label_key: entry.label_key,
                hint_key: entry.hint_key,
                icon: entry.icon,
                tone: entry.tone,
                shortcut: entry.shortcut,
                selected: Some(entry.key) == chosen,
                ready: entry.target.is_some(),
                kind,
            }
        })
        .collect();
    render(&StaffBar {
        t: &state.inner.i18n,
        login: &session.identity.login,
        csrf: &session.csrf,
        scopes,
        new_entries: links(&NEW_ENTRIES, &state, &session),
        modules: links(&MODULES, &state, &session),
        tools: links(&TOOLS, &state, &session),
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
    // A placeholder scope has no screen yet.
    let Some((target, parameter)) = entry.target else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let query = params.q.trim();
    if query.is_empty() {
        return Redirect::to(target).into_response();
    }
    Redirect::to(&format!("{target}?{parameter}={}", query_value(query))).into_response()
}

/// `value` percent-encoded for a query string: everything but the unreserved characters of
/// RFC 3986 is escaped, so the redirect can never carry a second parameter or a header break.
pub(crate) fn query_value(value: &str) -> String {
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
        // N opens "New"; I and L stay reserved for the AI and Lists tools of the draft.
        let reserved = ['i', 'l', 'n'];
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
