// SPDX-License-Identifier: GPL-3.0-or-later
//! The text of a notice, and the screen of printed notices: the ones pending, ready to
//! print, and marked as sent once printed (docs/specs/circulacao/avisos.md, items 31 to 33).

use axum::Form;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use elzevir_app::circulation_rules::CirculationCommand;
use elzevir_app::notices::{NoticeAbout, NoticeId, NoticeItem, NoticeOutcome, PendingNotice};
use elzevir_domain::notice::NoticeLevel;
use elzevir_domain::notice_text::{NoticeText, NoticeTextKind, fill};
use elzevir_web_templates::{I18n, PrintedNotice, StaffPrintNotices};
use jiff::tz::TimeZone;
use serde::Deserialize;

use crate::circulation::{now, open};
use crate::patrons::shown;
use crate::security::tokens_match;
use crate::{AppState, render};

const PATH: &str = "/equipe/circulacao/avisos";

/// Most notices one page shows and one press marks.
const PAGE: usize = 200;

/// The subject and the text of a notice, in the language of `t`.
#[must_use]
pub fn notice_text(t: &I18n, notice: &PendingNotice, time_zone: &TimeZone) -> (String, String) {
    let item_line = |item: &NoticeItem| match &item.due {
        Some(due) => t.get_texts(
            "notice-overdue-item",
            &[
                ("title", &item.title),
                ("barcode", &item.barcode),
                (
                    "date",
                    &shown(t, due.with_time_zone(time_zone.clone()).date()),
                ),
            ],
        ),
        None => t.get_texts(
            "notice-hold-item",
            &[("title", &item.title), ("barcode", &item.barcode)],
        ),
    };
    let items = notice
        .items
        .iter()
        .map(item_line)
        .collect::<Vec<_>>()
        .join("\n");
    let (kind, until) = match notice.about {
        NoticeAbout::Overdue(level) => (NoticeTextKind::Overdue(level), None),
        NoticeAbout::HoldWaiting(until) => (NoticeTextKind::Hold, Some(shown(t, until))),
    };
    let text = notice
        .text
        .clone()
        .unwrap_or_else(|| built_in_text(t, kind));
    let values = [
        ("name", notice.name.as_str()),
        ("library", notice.library.as_str()),
        ("items", items.as_str()),
        ("date", until.as_deref().unwrap_or_default()),
    ];
    (fill(&text.subject, &values), fill(&text.body, &values))
}

/// The built-in text of a notice in the language of `t`, with its placeholders, which a
/// library starts from when it writes its own (spec, item 35).
#[must_use]
pub(crate) fn built_in_text(t: &I18n, kind: NoticeTextKind) -> NoticeText {
    let (subject_key, body_key) = match kind {
        NoticeTextKind::Overdue(NoticeLevel::First) => {
            ("notice-overdue-subject-1", "notice-overdue-body")
        }
        NoticeTextKind::Overdue(NoticeLevel::Second) => {
            ("notice-overdue-subject-2", "notice-overdue-body")
        }
        NoticeTextKind::Overdue(NoticeLevel::Third) => {
            ("notice-overdue-subject-3", "notice-overdue-body")
        }
        NoticeTextKind::Hold => ("notice-hold-subject", "notice-hold-body"),
    };
    let placeholders =
        ["name", "library", "items", "date"].map(|name| (name, format!("{{{name}}}")));
    let values: Vec<(&str, &str)> = placeholders
        .iter()
        .map(|(name, value)| (*name, value.as_str()))
        .collect();
    NoticeText {
        subject: t.get_with(subject_key, "library", "{library}"),
        body: t.get_texts(body_key, &values),
    }
}

/// Query string of the page.
#[derive(Deserialize)]
pub(crate) struct PrintParams {
    marked: Option<usize>,
}

/// `GET /equipe/circulacao/avisos`: the printed notices pending, oldest first.
pub(crate) async fn print_notices(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<PrintParams>,
) -> Response {
    let session = match open(&state, &headers) {
        Ok((session, ..)) => session,
        Err(response) => return *response,
    };
    let Some(queue) = state.notices.clone() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Ok(Ok(pending)) = tokio::task::spawn_blocking(move || queue.pending_prints(PAGE)).await
    else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let t = &state.inner.i18n;
    let time_zone = &state.inner.config.time_zone;
    render(&StaffPrintNotices {
        t,
        csrf: &session.csrf,
        notices: pending
            .iter()
            .map(|notice| {
                let (subject, text) = notice_text(t, notice, time_zone);
                PrintedNotice {
                    id: notice.id.0,
                    name: notice.name.clone(),
                    address: notice.address.clone(),
                    subject,
                    text,
                }
            })
            .collect(),
        marked: params.marked.map(|count| {
            t.get_numbers(
                "print-notices-marked",
                &[("count", u64::try_from(count).unwrap_or(u64::MAX))],
            )
        }),
    })
}

/// `POST /equipe/circulacao/avisos`: each notice shown (`n`) becomes sent (spec, item 33).
pub(crate) async fn mark_printed(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(fields): Form<Vec<(String, String)>>,
) -> Response {
    let (session, _, loans) = match open(&state, &headers) {
        Ok(opened) => opened,
        Err(response) => return *response,
    };
    let csrf = fields
        .iter()
        .find(|(name, _)| name == "csrf")
        .map_or("", |(_, value)| value.as_str());
    if !tokens_match(&session.csrf, csrf) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(notices) = fields
        .iter()
        .filter(|(name, _)| name == "n")
        .map(|(_, value)| value.parse().ok().map(NoticeId))
        .collect::<Option<Vec<_>>>()
        .filter(|notices| notices.len() <= PAGE)
    else {
        return (StatusCode::BAD_REQUEST, "bad notice").into_response();
    };
    let count = notices.len();
    let actor = session.identity.id;
    let at = now(&state);
    let marked = tokio::task::spawn_blocking(move || {
        notices.into_iter().try_for_each(|notice| {
            let command = CirculationCommand::MarkNotice {
                notice,
                outcome: NoticeOutcome::Sent,
                at: at.clone(),
            };
            loans.circulate(actor, command).map(|_| ())
        })
    })
    .await;
    match marked {
        Ok(Ok(())) => Redirect::to(&format!("{PATH}?marked={count}")).into_response(),
        Ok(Err(_)) | Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}
