// SPDX-License-Identifier: GPL-3.0-or-later
//! The search of authority sources in the "+ Authority" form
//! (docs/specs/catalogo/fontes-de-autoridade.md, items 16 to 19): each source asked on
//! its own; "Approve" fills the form's row and stores nothing.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use elzevir_app::authority_sources::{AuthorityDraft, AuthorityQuery};
use elzevir_app::drafts::{DRAFTS_PER_PAGE, DraftQueryError, MAX_DRAFT_PAGE, SourceOutcome};
use elzevir_app::staff::Permission;
use elzevir_web_templates::{AuthorityApproval, DraftAnswer, DraftLine, DraftOutcome, I18n};
use serde::Deserialize;

use crate::record_form::{subject_kind, subject_names};
use crate::staff::current_session;
use crate::{AppState, render};

/// The query string of one source's answer (item 18).
#[derive(Deserialize)]
pub(crate) struct AuthorityParams {
    #[serde(default)]
    name: String,
    #[serde(default)]
    class: String,
    page: Option<u32>,
}

/// `GET /equipe/acervo/fontes-de-autoridade/{n}`: what authority source `n` answers.
pub(crate) async fn answer(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(index): Path<usize>,
    Query(params): Query<AuthorityParams>,
) -> Response {
    let Some(sources) = state.authority_sources.clone() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match current_session(&state, &headers) {
        None => return StatusCode::UNAUTHORIZED.into_response(),
        Some(session) if !session.identity.can(Permission::Cataloguing) => {
            return StatusCode::FORBIDDEN.into_response();
        }
        Some(_) => {}
    }
    let t = &state.inner.i18n;
    let Some(kind) = subject_kind(&params.class) else {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            t.get("authority-class-unknown"),
        )
            .into_response();
    };
    let query = match AuthorityQuery::new(&params.name, kind) {
        Ok(query) => query,
        Err(DraftQueryError::Empty) => {
            return (StatusCode::UNPROCESSABLE_ENTITY, t.get("draft-query-empty")).into_response();
        }
        Err(DraftQueryError::TooLong) => {
            return (StatusCode::UNPROCESSABLE_ENTITY, t.get("draft-query-long")).into_response();
        }
    };
    let page = params.page.unwrap_or(1).clamp(1, MAX_DRAFT_PAGE);
    let asked = Arc::clone(&sources);
    match tokio::task::spawn_blocking(move || asked.search_one_authority(index, &query, page)).await
    {
        Ok(Some(result)) => render(&DraftAnswer {
            t,
            outcome: shown(t, &result.source, result.outcome, page),
        }),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

/// The form's view of an answer: each line carries what "Approve" fills in.
fn shown(
    t: &I18n,
    source: &str,
    outcome: SourceOutcome<AuthorityDraft>,
    page: u32,
) -> DraftOutcome {
    match outcome {
        SourceOutcome::Found {
            total,
            drafts,
            skipped,
        } => {
            if total == 0 && drafts.is_empty() && skipped == 0 {
                return DraftOutcome::Nothing;
            }
            let lines = drafts
                .into_iter()
                .filter_map(|draft| line(t, source, draft))
                .collect();
            let shown_so_far = u64::from(page).saturating_mul(u64::from(DRAFTS_PER_PAGE));
            DraftOutcome::Found {
                total: t.get_numbers("draft-found", &[("total", total)]),
                drafts: lines,
                skipped,
                next_page: (shown_so_far < total && page < MAX_DRAFT_PAGE).then_some(page + 1),
            }
        }
        SourceOutcome::NoResponse => DraftOutcome::NoResponse,
        SourceOutcome::Refused(message) => DraftOutcome::Refused(message),
    }
}

/// One line: the heading, then the class, the control number and the variants.
fn line(t: &I18n, source: &str, draft: AuthorityDraft) -> Option<DraftLine> {
    let heading = draft.heading;
    let (kind, label) = subject_names(heading.kind)?;
    let identifier = heading.identifier.unwrap_or_default();
    let mut details = vec![t.get(label)];
    if !identifier.is_empty() {
        details.push(identifier.clone());
    }
    if !heading.variants.is_empty() {
        details.push(format!(
            "{} {}",
            t.get("authority-see-from"),
            heading.variants.join("; ")
        ));
    }
    Some(DraftLine {
        key: String::new(),
        title: heading.heading.clone(),
        details: details.join(" | "),
        isbns: String::new(),
        authority: Some(AuthorityApproval {
            kind,
            heading: heading.heading,
            identifier,
            source: source.to_owned(),
        }),
    })
}
