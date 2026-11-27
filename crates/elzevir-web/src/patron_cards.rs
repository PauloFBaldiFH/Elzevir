// SPDX-License-Identifier: GPL-3.0-or-later
//! Patron cards as one PDF (docs/specs/circulacao/carteirinhas.md).

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use elzevir_app::printing::{CardTexts, MAX_CARDS, PatronCard, PrintError};
use elzevir_domain::patron::PatronId;

use crate::AppState;
use crate::patrons::{failed, open, sees_patrons, shown};

/// `GET /equipe/leitores/carteirinhas?leitor=…`: the cards of the patrons named, in that
/// order, each once (carteirinhas.md, items 1 to 3).
pub(crate) async fn print(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(pairs): Query<Vec<(String, String)>>,
) -> Response {
    let desk = match open(&state, &headers, sees_patrons) {
        Ok((_, desk)) => desk,
        Err(response) => return *response,
    };
    let Some(printer) = state.printer.clone() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut ids: Vec<PatronId> = Vec::new();
    for (name, value) in &pairs {
        if name != "leitor" {
            continue;
        }
        let Ok(id) = value.parse().map(PatronId) else {
            return (StatusCode::BAD_REQUEST, "not a patron").into_response();
        };
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    if ids.is_empty() {
        return (StatusCode::BAD_REQUEST, "no patron").into_response();
    }
    if ids.len() > MAX_CARDS {
        let refused = PrintError::TooMany {
            count: ids.len(),
            max: MAX_CARDS,
        };
        return (StatusCode::PAYLOAD_TOO_LARGE, refused.to_string()).into_response();
    }
    let t = &state.inner.i18n;
    let texts = CardTexts {
        cardnumber: t.get("patron-cards-cardnumber"),
        expires: t.get("patron-cards-expires"),
    };
    let i18n = state.clone();
    let typeset = tokio::task::spawn_blocking(move || {
        let t = &i18n.inner.i18n;
        let mut cards = Vec::new();
        for id in ids {
            match desk.patron(id) {
                Ok(Some(patron)) => {
                    let expires = patron.fields.expires.map(|day| shown(t, day));
                    cards.push(PatronCard::new(&patron, expires));
                }
                Ok(None) => {}
                Err(error) => return Err(Box::new(failed(&error))),
            }
        }
        if cards.is_empty() {
            return Err(Box::new(StatusCode::NOT_FOUND.into_response()));
        }
        Ok(printer.patron_cards(&cards, &texts))
    })
    .await;
    match typeset {
        Ok(Ok(Ok(pdf))) => (
            [
                (header::CONTENT_TYPE, "application/pdf"),
                (
                    header::CONTENT_DISPOSITION,
                    "attachment; filename=\"carteirinhas.pdf\"",
                ),
            ],
            pdf,
        )
            .into_response(),
        Ok(Ok(Err(error))) => {
            (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response()
        }
        Ok(Err(response)) => *response,
        Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}
