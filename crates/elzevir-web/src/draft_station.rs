// SPDX-License-Identifier: GPL-3.0-or-later
//! The capture station of a new family (docs/specs/catalogo/estacao-de-captura.md): each
//! draft source asked on its own, and the drafts it found kept in memory until approved.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use elzevir_app::drafts::{
    DRAFTS_PER_PAGE, Draft, DraftQuery, DraftQueryError, DraftSource, MAX_DRAFT_PAGE,
    SourceOutcome, TypedQuery,
};
use elzevir_app::staff::Permission;
use elzevir_web_templates::{DraftAnswer, DraftLine, DraftOutcome, I18n};
use serde::Deserialize;

use crate::security::random_token;
use crate::staff::current_session;
use crate::{AppState, render};

/// How long a found draft can still be approved (item 14).
const KEPT_FOR: Duration = Duration::from_hours(2);
/// Most drafts kept at once; the oldest goes first (item 14).
const MOST_KEPT: usize = 1000;

/// The institution's draft sources and the drafts they found lately.
pub(crate) struct Drafts {
    source: Arc<dyn DraftSource>,
    kept: Mutex<VecDeque<Kept>>,
}

struct Kept {
    key: String,
    at: Instant,
    draft: Draft,
}

impl Drafts {
    pub(crate) fn new(source: Arc<dyn DraftSource>) -> Self {
        Self {
            source,
            kept: Mutex::new(VecDeque::new()),
        }
    }

    /// The sources' names, in the configured order.
    pub(crate) fn sources(&self) -> Vec<String> {
        self.source.sources()
    }

    /// Keeps `draft` under a new random key; `None` when no key can be drawn.
    fn keep(&self, draft: Draft) -> Option<String> {
        let key = random_token().ok()?;
        let now = Instant::now();
        // A poisoned list still holds whole entries: each push and pop is one step.
        let mut kept = self.kept.lock().unwrap_or_else(PoisonError::into_inner);
        while kept
            .front()
            .is_some_and(|oldest| now.duration_since(oldest.at) > KEPT_FOR)
            || kept.len() >= MOST_KEPT
        {
            kept.pop_front();
        }
        kept.push_back(Kept {
            key: key.clone(),
            at: now,
            draft,
        });
        Some(key)
    }

    /// The draft kept under `key`, while it has not expired.
    pub(crate) fn kept(&self, key: &str) -> Option<Draft> {
        let kept = self.kept.lock().unwrap_or_else(PoisonError::into_inner);
        kept.iter()
            .find(|entry| entry.key == key && entry.at.elapsed() <= KEPT_FOR)
            .map(|entry| entry.draft.clone())
    }
}

/// The query string of one source's answer (item 7).
#[derive(Deserialize)]
pub(crate) struct StationParams {
    #[serde(default)]
    isbn: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    author: String,
    #[serde(default)]
    any: String,
    page: Option<u32>,
}

/// `GET /equipe/acervo/fontes/{n}`: what source `n` answers (items 4, 5 and 7).
pub(crate) async fn answer(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(index): Path<usize>,
    Query(params): Query<StationParams>,
) -> Response {
    let Some(drafts) = state.drafts.clone() else {
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
    let query = match DraftQuery::new(&TypedQuery {
        isbn: &params.isbn,
        issn: "",
        title: &params.title,
        author: &params.author,
        any: &params.any,
    }) {
        Ok(query) => query,
        Err(error) => {
            let key = match error {
                DraftQueryError::Empty => "draft-query-empty",
                DraftQueryError::TooLong => "draft-query-long",
            };
            return (StatusCode::UNPROCESSABLE_ENTITY, t.get(key)).into_response();
        }
    };
    let page = params.page.unwrap_or(1).clamp(1, MAX_DRAFT_PAGE);
    let source = Arc::clone(&drafts.source);
    match tokio::task::spawn_blocking(move || source.search_one(index, &query, page)).await {
        Ok(Some(result)) => render(&DraftAnswer {
            t,
            outcome: shown(&drafts, t, result.outcome, page),
        }),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

/// The station's view of an answer, keeping each draft for "Approve".
fn shown(drafts: &Drafts, t: &I18n, outcome: SourceOutcome, page: u32) -> DraftOutcome {
    match outcome {
        SourceOutcome::Found {
            total,
            drafts: found,
            skipped,
        } => {
            if total == 0 && found.is_empty() && skipped == 0 {
                return DraftOutcome::Nothing;
            }
            let lines = found
                .into_iter()
                .filter_map(|draft| {
                    let summary = draft.summary.clone();
                    let key = drafts.keep(draft)?;
                    let details = [summary.author, summary.edition, summary.publication]
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>()
                        .join(" | ");
                    Some(DraftLine {
                        key,
                        title: summary.title,
                        details,
                        isbns: summary.isbns.join(", "),
                        authority: None,
                    })
                })
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
