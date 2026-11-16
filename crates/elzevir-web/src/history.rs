// SPDX-License-Identifier: GPL-3.0-or-later
//! The History of the focused Work or Instance (docs/specs/catalogo/historico.md): who
//! changed what and when, read from the `audit_log`, for cataloguers only.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use elzevir_app::history::{Change, HISTORY_LIMIT, HistoryEntry, Part, entry};
use elzevir_app::staff::Permission;
use elzevir_domain::statement::{EntityKind, EntityRef};
use elzevir_web_templates::{FormSection, History, HistoryLine, I18n};

use crate::form_sections::{instance_sections, work_sections};
use crate::new_record::blank;
use crate::staff::current_session;
use crate::{AppState, render};

/// `GET /equipe/acervo/obra/{id}/historico`.
pub(crate) async fn work_history(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    history(&state, &headers, EntityKind::Work, id).await
}

/// `GET /equipe/acervo/instancia/{id}/historico`.
pub(crate) async fn instance_history(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    history(&state, &headers, EntityKind::Instance, id).await
}

async fn history(state: &AppState, headers: &HeaderMap, entity: EntityKind, id: i64) -> Response {
    let Some(session) = current_session(state, headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if !session.identity.can(Permission::Cataloguing) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let port = Arc::clone(&state.inner.staff_ports.history);
    let subject = EntityRef { entity, id };
    let read = tokio::task::spawn_blocking(move || port.history(subject, HISTORY_LIMIT)).await;
    let Ok(Ok(logged)) = read else {
        return (StatusCode::SERVICE_UNAVAILABLE, "catalog unavailable").into_response();
    };
    let t = &state.inner.i18n;
    let blank = blank();
    let sections = match entity {
        EntityKind::Work => work_sections(t, &blank),
        _ => instance_sections(t, &blank),
    };
    let lines = logged
        .into_iter()
        .map(|logged| line(t, &sections, entry(logged)))
        .collect();
    render(&History { t, lines })
}

fn line(t: &I18n, sections: &[FormSection], entry: HistoryEntry) -> HistoryLine {
    let when = entry.at.as_deref().map_or_else(
        || t.get("history-unknown-time"),
        // `2026-11-16T15:32:22Z` reads `2026-11-16 15:32 UTC`; the script shows it in the
        // viewer's own time.
        |at| {
            format!(
                "{} {} UTC",
                at.get(..10).unwrap_or(at),
                at.get(11..16).unwrap_or("")
            )
        },
    );
    HistoryLine {
        at: entry.at.unwrap_or_default(),
        when,
        who: entry.actor.unwrap_or_else(|| t.get("history-system")),
        what: what(t, sections, &entry.change),
    }
}

fn what(t: &I18n, sections: &[FormSection], change: &Change) -> String {
    match change {
        Change::Created => t.get("history-created"),
        Change::Imported => t.get("history-imported"),
        Change::Edited(parts) if parts.is_empty() => t.get("history-edited-whole"),
        Change::Edited(parts) => {
            let labels: Vec<String> = parts.iter().map(|part| label(t, sections, part)).collect();
            t.get_with("history-edited", "parts", &labels.join(", "))
        }
        Change::Items(count) => t.get_numbers(
            "history-items",
            &[("count", u64::try_from(*count).unwrap_or(u64::MAX))],
        ),
        Change::Other => t.get("history-other"),
    }
}

/// The label the form shows for `part`; its own name when the form has no such field.
fn label(t: &I18n, sections: &[FormSection], part: &Part) -> String {
    let fields = || sections.iter().flat_map(|section| &section.fields);
    let found = match part {
        Part::Field(change) => fields().find(|field| field.change == change),
        Part::Property(property) => fields().find(|field| field.property == property),
    };
    match (found, part) {
        (Some(field), _) => field.label(t),
        (None, Part::Field(name) | Part::Property(name)) => name.clone(),
    }
}
