// SPDX-License-Identifier: GPL-3.0-or-later
//! A patron signing in to the OPAC, their session and their account page
//! (docs/specs/opac/conta-do-leitor.md, items 5 to 12).

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use axum::Form;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use elzevir_app::patron_account::{PatronIdentity, PatronLoginRefused, log_in};
use elzevir_web_templates::{OpacLogin, PatronPanel};
use serde::Deserialize;

use crate::security::{random_token, tokens_match};
use crate::session::Live;
use crate::staff::{cookie, expired_cookie, session_cookie, set_cookie};
use crate::{AppState, render};

/// The patron's session cookie, apart from the staff's (spec, item 9).
const SESSION_COOKIE: &str = "elzevir_patron";
/// The anti-forgery cookie of the sign-in form.
const LOGIN_CSRF_COOKIE: &str = "elzevir_patron_csrf";

/// A patron session ends after this long without use (spec, item 9).
pub(crate) const PATRON_IDLE: Duration = Duration::from_mins(30);

/// Refusals in a row that lock a card (spec, item 7).
const MAX_FAILURES: u8 = 5;
/// How long the refusals count, and how long a card stays locked.
const WINDOW: Duration = Duration::from_mins(15);
/// Most cards followed at once, so that random card numbers cannot fill memory.
const MAX_FOLLOWED: usize = 10_000;

/// Refused sign-ins by card number, in memory (spec, item 7).
#[derive(Default)]
pub(crate) struct Attempts {
    failures: Mutex<HashMap<String, (u8, Instant)>>,
}

impl Attempts {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, (u8, Instant)>> {
        // A poisoned lock only means a request panicked mid-update of one count.
        self.failures.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Whether `card` is locked at `now`.
    pub(crate) fn locked(&self, card: &str, now: Instant) -> bool {
        self.lock().get(card).is_some_and(|(count, last)| {
            *count >= MAX_FAILURES && now.duration_since(*last) < WINDOW
        })
    }

    /// Counts one refusal of `card` at `now`.
    pub(crate) fn failed(&self, card: &str, now: Instant) {
        let mut failures = self.lock();
        if failures.len() >= MAX_FOLLOWED {
            failures.retain(|_, (_, last)| now.duration_since(*last) < WINDOW);
        }
        if failures.len() >= MAX_FOLLOWED && !failures.contains_key(card) {
            return;
        }
        let entry = failures.entry(card.to_owned()).or_insert((0, now));
        if now.duration_since(entry.1) >= WINDOW {
            *entry = (0, now);
        }
        *entry = (entry.0.saturating_add(1), now);
    }

    /// Clears the refusals of `card` after a sign-in.
    pub(crate) fn succeeded(&self, card: &str) {
        self.lock().remove(card);
    }
}

/// The signed-in patron of this request, if any.
pub(crate) fn current_patron(
    state: &AppState,
    headers: &HeaderMap,
) -> Option<Live<PatronIdentity>> {
    let id = cookie(headers, SESSION_COOKIE)?;
    state.inner.patron_sessions.touch(&id)
}

/// Pages with a patron's data are never stored by a cache (spec, item 11).
pub(crate) fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// The "Patron account" panel for this request, and the sign-in cookie it needs.
pub(crate) fn panel(state: &AppState, headers: &HeaderMap) -> (PatronPanel, Option<String>) {
    if state.accounts.is_none() {
        return (PatronPanel::Soon, None);
    }
    if let Some(session) = current_patron(state, headers) {
        let panel = PatronPanel::SignedIn {
            greeting: session.identity.greeting,
            csrf: session.csrf,
        };
        return (panel, None);
    }
    sign_in_panel(String::new(), None)
}

fn sign_in_panel(card: String, error: Option<&'static str>) -> (PatronPanel, Option<String>) {
    match random_token() {
        Ok(csrf) => (
            PatronPanel::SignIn {
                csrf: csrf.clone(),
                card,
                error,
            },
            Some(csrf),
        ),
        Err(_) => (PatronPanel::Soon, None),
    }
}

/// Sends the panel's page with its cookie and no caching.
pub(crate) fn with_panel_cookie(
    state: &AppState,
    response: Response,
    csrf: Option<&str>,
) -> Response {
    let mut response = no_store(response);
    if let Some(csrf) = csrf {
        set_cookie(
            &mut response,
            &session_cookie(LOGIN_CSRF_COOKIE, csrf, state.inner.config.secure_cookies),
        );
    }
    response
}

fn login_page(
    state: &AppState,
    card: String,
    error: Option<&'static str>,
    status: StatusCode,
) -> Response {
    let (panel, csrf) = sign_in_panel(card, error);
    let mut response = render(&OpacLogin {
        t: &state.inner.i18n,
        panel,
    });
    *response.status_mut() = status;
    with_panel_cookie(state, response, csrf.as_deref())
}

/// `GET /conta/entrar`.
pub(crate) async fn login_form(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if state.accounts.is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    if current_patron(&state, &headers).is_some() {
        return Redirect::to("/conta").into_response();
    }
    login_page(&state, String::new(), None, StatusCode::OK)
}

/// Fields of the sign-in form.
#[derive(Deserialize)]
pub(crate) struct LoginFields {
    csrf: String,
    card: String,
    password: String,
}

/// `POST /conta/entrar`.
pub(crate) async fn log_in_patron(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<LoginFields>,
) -> Response {
    let Some(accounts) = state.accounts.clone() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let expected = cookie(&headers, LOGIN_CSRF_COOKIE).unwrap_or_default();
    if expected.is_empty() || !tokens_match(&expected, &form.csrf) {
        return (StatusCode::FORBIDDEN, "invalid form token").into_response();
    }
    let card = form.card.trim().to_owned();
    if state.inner.attempts.locked(&card, Instant::now()) {
        return login_page(
            &state,
            card,
            Some("opac-login-locked"),
            StatusCode::TOO_MANY_REQUESTS,
        );
    }
    // Argon2 and the lookup are blocking work.
    let (typed, password) = (card.clone(), form.password);
    let checked =
        tokio::task::spawn_blocking(move || log_in(accounts.as_ref(), &typed, &password)).await;
    let identity = match checked {
        Ok(Ok(identity)) => identity,
        Ok(Err(PatronLoginRefused::InvalidCredentials)) => {
            state.inner.attempts.failed(&card, Instant::now());
            return login_page(
                &state,
                card,
                Some("opac-login-invalid"),
                StatusCode::UNAUTHORIZED,
            );
        }
        Ok(Err(PatronLoginRefused::Unavailable(_))) | Err(_) => {
            return login_page(
                &state,
                card,
                Some("opac-login-unavailable"),
                StatusCode::SERVICE_UNAVAILABLE,
            );
        }
    };
    state.inner.attempts.succeeded(&card);
    let Ok(id) = state.inner.patron_sessions.start(identity) else {
        return (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response();
    };
    let mut response = Redirect::to("/conta").into_response();
    set_cookie(
        &mut response,
        &session_cookie(SESSION_COOKIE, &id, state.inner.config.secure_cookies),
    );
    set_cookie(&mut response, &expired_cookie(LOGIN_CSRF_COOKIE));
    response
}

/// Fields of the sign-out button.
#[derive(Deserialize)]
pub(crate) struct LogoutFields {
    csrf: String,
}

/// `POST /conta/sair`.
pub(crate) async fn log_out_patron(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<LogoutFields>,
) -> Response {
    let Some(id) = cookie(&headers, SESSION_COOKIE) else {
        return Redirect::to("/").into_response();
    };
    match state.inner.patron_sessions.touch(&id) {
        Some(session) if tokens_match(&session.csrf, &form.csrf) => {
            state.inner.patron_sessions.end(&id);
            let mut response = Redirect::to("/").into_response();
            set_cookie(&mut response, &expired_cookie(SESSION_COOKIE));
            response
        }
        Some(_) => (StatusCode::FORBIDDEN, "invalid form token").into_response(),
        None => Redirect::to("/").into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_refusals_lock_a_card_for_the_window_and_a_sign_in_clears_them() {
        let attempts = Attempts::default();
        let start = Instant::now();
        for _ in 0..4 {
            attempts.failed("0042", start);
        }
        assert!(!attempts.locked("0042", start));
        attempts.failed("0042", start);
        assert!(attempts.locked("0042", start));
        assert!(!attempts.locked("0043", start));
        assert!(!attempts.locked("0042", start + WINDOW));
        attempts.succeeded("0042");
        assert!(!attempts.locked("0042", start));
        // Refusals older than the window start the count again.
        for _ in 0..4 {
            attempts.failed("0044", start);
        }
        attempts.failed("0044", start + WINDOW);
        assert!(!attempts.locked("0044", start + WINDOW));
    }
}
