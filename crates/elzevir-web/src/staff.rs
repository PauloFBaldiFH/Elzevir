// SPDX-License-Identifier: GPL-3.0-or-later
//! Staff sign-in, home and sign-out.

use axum::Form;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use elzevir_app::staff::{LoginRefused, Permission, log_in as check_credentials};
use elzevir_web_templates::{PendingChip, StaffHome, StaffLogin};
use serde::Deserialize;

use crate::security::{random_token, tokens_match};
use crate::session::ActiveSession;
use crate::{AppState, render};

const SESSION_COOKIE: &str = "elzevir_session";
const LOGIN_CSRF_COOKIE: &str = "elzevir_login_csrf";

/// `GET /equipe`: the staff home, or a redirect to sign in.
pub(crate) async fn home(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let Some(session) = current_session(&state, &headers) else {
        return Redirect::to("/equipe/login").into_response();
    };
    let notes = crate::board::staff_cards(&state, &session).await;
    let pending = pending_chips(&state, &session).await;
    render(&StaffHome {
        t: &state.inner.i18n,
        modules: crate::staff_bar::home_modules(&state, &session),
        notes,
        board: state.board.is_some(),
        pending,
    })
}

/// The desk's pending work for the Circulation card, for those who circulate; counts of
/// zero are left out, and a failed count shows none, since the home works without them.
async fn pending_chips(state: &AppState, session: &ActiveSession) -> Vec<PendingChip> {
    let Some(reader) = state.pending.clone() else {
        return Vec::new();
    };
    if !session.identity.can(Permission::Circulation) {
        return Vec::new();
    }
    let now = jiff::Timestamp::now();
    let Ok(Ok(counts)) = tokio::task::spawn_blocking(move || reader.pending(now)).await else {
        return Vec::new();
    };
    let t = &state.inner.i18n;
    [
        ("home-pending-overdue", counts.overdue),
        ("home-pending-holds", counts.waiting_holds),
        ("home-pending-notices", counts.notices_to_print),
    ]
    .into_iter()
    .filter(|(_, count)| *count > 0)
    .map(|(key, count)| PendingChip {
        module: "module-circulation",
        text: t.get_numbers(key, &[("count", count)]),
    })
    .collect()
}

/// `GET /equipe/admin/memory`: allocator report as JSON, for administrators.
pub(crate) async fn memory(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let Some(session) = current_session(&state, &headers) else {
        return Redirect::to("/equipe/login").into_response();
    };
    if !session.identity.can(Permission::Administration) {
        return StatusCode::FORBIDDEN.into_response();
    }
    axum::Json(state.inner.memory.report()).into_response()
}

/// `GET /equipe/login`: the sign-in form with a fresh anti-forgery token.
pub(crate) async fn login_form(State(state): State<AppState>) -> Response {
    login_page(&state, "", None, StatusCode::OK)
}

/// Fields posted by the sign-in form.
#[derive(Deserialize)]
pub(crate) struct LoginForm {
    login: String,
    password: String,
    csrf: String,
}

/// `POST /equipe/login`.
pub(crate) async fn log_in(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<LoginForm>,
) -> Response {
    let expected = cookie(&headers, LOGIN_CSRF_COOKIE).unwrap_or_default();
    if expected.is_empty() || !tokens_match(&expected, &form.csrf) {
        return (StatusCode::FORBIDDEN, "invalid form token").into_response();
    }

    // Argon2 and the directory lookup are blocking work.
    let directory = std::sync::Arc::clone(&state.inner.staff);
    let (login, password) = (form.login.clone(), form.password);
    let checked = tokio::task::spawn_blocking(move || {
        check_credentials(directory.as_ref(), &login, &password)
    })
    .await;

    let refusal = match checked {
        Ok(Ok(identity)) => return start_session(&state, identity),
        Ok(Err(LoginRefused::InvalidCredentials)) => "staff-login-invalid",
        Ok(Err(LoginRefused::NotStaff)) => "staff-login-not-staff",
        Ok(Err(LoginRefused::Unavailable(_))) | Err(_) => "staff-login-unavailable",
    };
    login_page(&state, &form.login, Some(refusal), StatusCode::UNAUTHORIZED)
}

/// Fields posted by the sign-out form.
#[derive(Deserialize)]
pub(crate) struct LogoutForm {
    csrf: String,
}

/// `POST /equipe/logout`.
pub(crate) async fn log_out(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<LogoutForm>,
) -> Response {
    let Some(id) = cookie(&headers, SESSION_COOKIE) else {
        return Redirect::to("/equipe/login").into_response();
    };
    match state.inner.sessions.touch(&id) {
        Some(session) if tokens_match(&session.csrf, &form.csrf) => {
            state.inner.sessions.end(&id);
            let mut response = Redirect::to("/equipe/login").into_response();
            set_cookie(&mut response, &expired_cookie(SESSION_COOKIE));
            response
        }
        Some(_) => (StatusCode::FORBIDDEN, "invalid form token").into_response(),
        None => Redirect::to("/equipe/login").into_response(),
    }
}

fn start_session(state: &AppState, identity: elzevir_app::staff::StaffIdentity) -> Response {
    let Ok(id) = state.inner.sessions.start(identity) else {
        return (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response();
    };
    let mut response = Redirect::to("/equipe").into_response();
    set_cookie(
        &mut response,
        &session_cookie(SESSION_COOKIE, &id, state.inner.config.secure_cookies),
    );
    set_cookie(&mut response, &expired_cookie(LOGIN_CSRF_COOKIE));
    response
}

fn login_page(
    state: &AppState,
    login: &str,
    error: Option<&'static str>,
    status: StatusCode,
) -> Response {
    let Ok(csrf) = random_token() else {
        return (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response();
    };
    let mut response = render(&StaffLogin {
        t: &state.inner.i18n,
        csrf: &csrf,
        login,
        error,
    });
    *response.status_mut() = status;
    set_cookie(
        &mut response,
        &session_cookie(LOGIN_CSRF_COOKIE, &csrf, state.inner.config.secure_cookies),
    );
    response
}

pub(crate) fn current_session(state: &AppState, headers: &HeaderMap) -> Option<ActiveSession> {
    let id = cookie(headers, SESSION_COOKIE)?;
    state.inner.sessions.touch(&id)
}

/// The value of cookie `name` in the request, if present.
pub(crate) fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value.to_owned())
}

pub(crate) fn session_cookie(name: &str, value: &str, secure: bool) -> String {
    let secure = if secure { "; Secure" } else { "" };
    format!("{name}={value}; Path=/; HttpOnly; SameSite=Lax{secure}")
}

pub(crate) fn expired_cookie(name: &str) -> String {
    format!("{name}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0")
}

pub(crate) fn set_cookie(response: &mut Response, cookie: &str) {
    if let Ok(value) = HeaderValue::from_str(cookie) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
}
