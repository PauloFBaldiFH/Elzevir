// SPDX-License-Identifier: GPL-3.0-or-later
//! The patron's account through the API, with bearer tokens (docs/specs/opac/api.md,
//! items 8 to 17).
//!
//! The token is a session like the OPAC's, kept in its own store: it travels in the
//! `Authorization` header and never in a cookie, so no anti-forgery token is needed.

use std::time::Instant;

use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use elzevir_app::accounts::{AccountLine, LineKind};
use elzevir_app::circulation_rules::{
    CirculationCommand, CirculationDeskError, CirculationOutput, CirculationRefusal,
};
use elzevir_app::holds::{
    HoldCancellation, HoldId, HoldLevel, HoldRefused, HoldRequest, HoldSuspensionRequest,
    PatronHold, Suspension, SuspensionChange,
};
use elzevir_app::loans::{LoanDesk, PatronLoan, RenewRequest};
use elzevir_app::opac;
use elzevir_app::patron_account::{PatronIdentity, PatronLoginRefused, log_in};
use elzevir_app::patrons::ReadingHistory;
use elzevir_domain::catalog::{BranchId, InstanceId, ItemId};
use elzevir_domain::hold::{Found, HoldRefusal};
use elzevir_domain::patron::PatronId;
use elzevir_domain::suspension::{RestrictionKind, SuspensionEnd};
use serde::{Deserialize, Serialize};

use super::error;
use crate::AppState;
use crate::circulation::now;
use crate::opac_account::refusal_code;
use crate::patron_login::PATRON_IDLE;

/// The token of the request and the patron behind it, while the token lives.
fn bearer(state: &AppState, headers: &HeaderMap) -> Option<(String, PatronIdentity)> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|token| !token.is_empty());
    let token = token?;
    let live = state.inner.api_tokens.touch(token)?;
    Some((token.to_owned(), live.identity))
}

fn unauthorized() -> Response {
    let mut response = error(StatusCode::UNAUTHORIZED, "unauthorized");
    response
        .headers_mut()
        .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
    response
}

/// Answers with a patron's data are never stored by a cache (item 12).
fn private<T: Serialize>(status: StatusCode, body: &T) -> Response {
    let mut response = (status, Json(body)).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn no_content() -> Response {
    let mut response = StatusCode::NO_CONTENT.into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn unavailable() -> Response {
    error(StatusCode::SERVICE_UNAVAILABLE, "unavailable")
}

fn bad_request() -> Response {
    error(StatusCode::BAD_REQUEST, "bad_request")
}

/// A refusal of circulation: `{"error": "refused", "reason": code}` (item 17).
fn refused(reason: &'static str) -> Response {
    #[derive(Serialize)]
    struct Body {
        error: &'static str,
        reason: &'static str,
    }
    (
        StatusCode::CONFLICT,
        Json(Body {
            error: "refused",
            reason,
        }),
    )
        .into_response()
}

fn not_found() -> Response {
    error(StatusCode::NOT_FOUND, "not_found")
}

/// Body of a token request.
#[derive(Deserialize)]
pub(crate) struct Credentials {
    card: String,
    password: String,
}

#[derive(Serialize)]
struct Issued {
    token: String,
    expires_in: u64,
}

/// `POST /api/v1/patron/tokens` (items 8, 9 and 13).
pub(crate) async fn issue(
    State(state): State<AppState>,
    body: Result<Json<Credentials>, JsonRejection>,
) -> Response {
    let Some(accounts) = state.accounts.clone() else {
        return not_found();
    };
    let Ok(Json(credentials)) = body else {
        return bad_request();
    };
    let card = credentials.card.trim().to_owned();
    let attempts = &state.inner.attempts;
    if attempts.locked(&card, Instant::now()) {
        return error(StatusCode::TOO_MANY_REQUESTS, "locked");
    }
    // Argon2 and the lookup are blocking work.
    let typed = card.clone();
    let checked = tokio::task::spawn_blocking(move || {
        log_in(accounts.as_ref(), &typed, &credentials.password)
    })
    .await;
    let identity = match checked {
        Ok(Ok(identity)) => identity,
        Ok(Err(PatronLoginRefused::InvalidCredentials)) => {
            attempts.failed(&card, Instant::now());
            return unauthorized();
        }
        Ok(Err(PatronLoginRefused::Unavailable(_))) | Err(_) => return unavailable(),
    };
    attempts.succeeded(&card);
    let Ok(token) = state.inner.api_tokens.start(identity) else {
        return unavailable();
    };
    private(
        StatusCode::CREATED,
        &Issued {
            token,
            expires_in: PATRON_IDLE.as_secs(),
        },
    )
}

/// `DELETE /api/v1/patron/tokens/current` (item 11).
pub(crate) async fn revoke(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let Some((token, _)) = bearer(&state, &headers) else {
        return unauthorized();
    };
    state.inner.api_tokens.end(&token);
    no_content()
}

#[derive(Serialize)]
struct AccountJson {
    name: String,
    card: String,
    branch: String,
    category: String,
    expires: String,
    restrictions: Vec<Restriction>,
    owed: i64,
    keeps_history: bool,
}

#[derive(Serialize)]
struct Restriction {
    kind: &'static str,
    /// The day it ends; `null` when only the staff lift it.
    until: Option<String>,
}

const fn restriction_kind(kind: RestrictionKind) -> &'static str {
    match kind {
        RestrictionKind::Suspension => "suspension",
        RestrictionKind::Overdues => "overdues",
    }
}

/// `GET /api/v1/patron/account` (item 14).
pub(crate) async fn account(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let Some(accounts) = state.accounts.clone() else {
        return not_found();
    };
    let Some((token, patron)) = bearer(&state, &headers) else {
        return unauthorized();
    };
    let desk = state.loans.clone();
    let today = now(&state).date();
    let read = tokio::task::spawn_blocking(move || {
        let Some(account) = accounts.account(patron.id).map_err(drop)? else {
            return Ok(None);
        };
        let (restrictions, owed) = match desk {
            Some(desk) => (
                desk.restrictions(patron.id, today).map_err(drop)?,
                desk.owed(patron.id).map_err(drop)?.0,
            ),
            None => (Vec::new(), 0),
        };
        Ok::<_, ()>(Some(AccountJson {
            name: account.name,
            card: account.cardnumber,
            branch: account.branch_name,
            category: account.category_name,
            expires: account.expires.to_string(),
            restrictions: restrictions
                .into_iter()
                .map(|(kind, end)| Restriction {
                    kind: restriction_kind(kind),
                    until: match end {
                        SuspensionEnd::Day(day) => Some(day.to_string()),
                        SuspensionEnd::WhenLifted => None,
                    },
                })
                .collect(),
            owed,
            keeps_history: account.reading_history == ReadingHistory::Kept,
        }))
    })
    .await;
    match read {
        Ok(Ok(Some(account))) => private(StatusCode::OK, &account),
        // The patron was removed after the token was issued.
        Ok(Ok(None)) => {
            state.inner.api_tokens.end(&token);
            unauthorized()
        }
        Ok(Err(())) | Err(_) => unavailable(),
    }
}

/// Reads with the patron's desk off the async runtime, and answers with `shape` of it.
async fn read_own<T, B>(
    state: &AppState,
    headers: &HeaderMap,
    read: fn(&dyn LoanDesk, PatronId) -> Result<Vec<T>, CirculationDeskError>,
    shape: fn(T) -> B,
) -> Response
where
    T: Send + 'static,
    B: Serialize,
{
    let Some(desk) = state.loans.clone() else {
        return not_found();
    };
    let Some((_, patron)) = bearer(state, headers) else {
        return unauthorized();
    };
    let patron = patron.id;
    match tokio::task::spawn_blocking(move || read(desk.as_ref(), patron)).await {
        Ok(Ok(rows)) => {
            let rows: Vec<B> = rows.into_iter().map(shape).collect();
            private(StatusCode::OK, &rows)
        }
        Ok(Err(_)) | Err(_) => unavailable(),
    }
}

#[derive(Serialize)]
struct LoanJson {
    item: i64,
    title: String,
    call_number: Option<String>,
    branch: String,
    due: String,
    due_at: String,
    renewals: u32,
    renewals_left: u32,
    on_site: bool,
}

fn loan_json(loan: PatronLoan) -> LoanJson {
    LoanJson {
        item: loan.item.0,
        title: loan.title,
        call_number: loan.call_number,
        branch: loan.branch_name,
        due: loan.due.date().to_string(),
        due_at: loan.due.timestamp().to_string(),
        renewals: loan.renewals,
        renewals_left: loan.renewals_left,
        on_site: loan.on_site,
    }
}

/// `GET /api/v1/patron/loans` (item 15).
pub(crate) async fn loans(State(state): State<AppState>, headers: HeaderMap) -> Response {
    read_own(
        &state,
        &headers,
        |desk, patron| desk.loans(patron),
        loan_json,
    )
    .await
}

#[derive(Serialize)]
struct DebtJson {
    date: String,
    kind: String,
    amount: i64,
    outstanding: i64,
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "read_own maps the rows it owns"
)]
fn debt_json(line: AccountLine) -> DebtJson {
    DebtJson {
        date: line.day.to_string(),
        kind: match line.kind {
            LineKind::Debit(debit) => debit.code(),
            LineKind::Credit(credit) => credit.code(),
        }
        .to_ascii_lowercase(),
        amount: line.amount.0,
        outstanding: line.outstanding.0,
    }
}

/// `GET /api/v1/patron/debts` (item 15).
pub(crate) async fn debts(State(state): State<AppState>, headers: HeaderMap) -> Response {
    read_own(
        &state,
        &headers,
        |desk, patron| desk.account(patron),
        debt_json,
    )
    .await
}

#[derive(Serialize)]
struct HoldJson {
    hold: i64,
    title: String,
    pickup: String,
    placed_on: String,
    status: &'static str,
    position: u32,
    expires_on: Option<String>,
    suspended: bool,
    suspended_until: Option<String>,
}

fn hold_json(hold: PatronHold) -> HoldJson {
    HoldJson {
        hold: hold.id.0,
        title: hold.title,
        pickup: hold.pickup,
        placed_on: hold.placed_on.to_string(),
        status: match hold.found {
            Some(Found::Waiting) => "waiting",
            Some(Found::InTransit) => "in_transit",
            None => "pending",
        },
        position: hold.priority,
        expires_on: hold.expires_on.map(|day| day.to_string()),
        suspended: hold.suspended.is_some(),
        suspended_until: match hold.suspended {
            Some(Suspension::Until(day)) => Some(day.to_string()),
            Some(Suspension::Indefinite) | None => None,
        },
    }
}

/// `GET /api/v1/patron/holds` (item 16).
pub(crate) async fn holds(State(state): State<AppState>, headers: HeaderMap) -> Response {
    read_own(
        &state,
        &headers,
        |desk, patron| desk.holds(patron),
        hold_json,
    )
    .await
}

/// Body of a hold request.
#[derive(Deserialize)]
pub(crate) struct HoldBody {
    instance: i64,
    pickup: i64,
}

#[derive(Serialize)]
struct Placed {
    hold: i64,
    position: u32,
}

/// The code of a hold refusal (item 16), in the groups the OPAC tells apart.
const fn hold_reason(refusal: &HoldRefused) -> &'static str {
    match refusal {
        HoldRefused::UnknownPatron | HoldRefused::UnknownBarcode | HoldRefused::WrongItem => {
            "unavailable"
        }
        HoldRefused::UnknownBranch | HoldRefused::Rules(HoldRefusal::PickupNotValid) => "pickup",
        HoldRefused::UnknownHold => "unknown_hold",
        HoldRefused::SetAside | HoldRefused::SuspendSetAside => "set_aside",
        HoldRefused::Rules(refusal) => match refusal {
            HoldRefusal::Restricted => "restricted",
            HoldRefusal::DebtLimit { .. } => "debt",
            HoldRefusal::ItemAlreadyOnHold | HoldRefusal::TooManyHoldsForThisRecord { .. } => {
                "already_held"
            }
            HoldRefusal::HoldLimit { .. }
            | HoldRefusal::TooManyReservesToday { .. }
            | HoldRefusal::TooManyReserves { .. } => "too_many",
            _ => "not_allowed",
        },
    }
}

/// `POST /api/v1/patron/holds` (item 16).
pub(crate) async fn place_hold(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<HoldBody>, JsonRejection>,
) -> Response {
    let Some(desk) = state.loans.clone() else {
        return not_found();
    };
    let Some((_, patron)) = bearer(&state, &headers) else {
        return unauthorized();
    };
    let patron = patron.id;
    let Ok(Json(body)) = body else {
        return bad_request();
    };
    let at = now(&state);
    let catalog = std::sync::Arc::clone(&state.inner.opac.catalog);
    let placed = tokio::task::spawn_blocking(move || {
        let Some(page) = opac::record(&*catalog, InstanceId(body.instance)).map_err(drop)? else {
            return Ok(None);
        };
        // A record hold names the record through any of its Items, as on the OPAC.
        let Some(barcode) = page.items.into_iter().find_map(|item| item.barcode) else {
            return Ok(Some(Err("unavailable")));
        };
        let command = CirculationCommand::PlaceHold(HoldRequest {
            patron,
            barcode,
            level: HoldLevel::Record,
            pickup: BranchId(body.pickup),
            at,
            by_patron: true,
        });
        Ok::<_, ()>(Some(match desk.patron_hold(command) {
            Ok(CirculationOutput::HoldPlaced(placed)) => Ok(placed),
            Err(CirculationDeskError::Refused(CirculationRefusal::Hold(refusal))) => {
                Err(hold_reason(&refusal))
            }
            Ok(_) | Err(_) => return Err(()),
        }))
    })
    .await;
    match placed {
        Ok(Ok(Some(Ok(placed)))) => private(
            StatusCode::CREATED,
            &Placed {
                hold: placed.hold.0,
                position: placed.priority,
            },
        ),
        Ok(Ok(Some(Err(reason)))) => refused(reason),
        Ok(Ok(None)) => error(StatusCode::NOT_FOUND, "not_found"),
        Ok(Err(())) | Err(_) => unavailable(),
    }
}

/// `DELETE /api/v1/patron/holds/{id}` (item 16).
pub(crate) async fn cancel_hold(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(hold): Path<String>,
) -> Response {
    let Some(desk) = state.loans.clone() else {
        return not_found();
    };
    let Some((_, patron)) = bearer(&state, &headers) else {
        return unauthorized();
    };
    let patron = patron.id;
    let Ok(hold) = hold.parse::<i64>() else {
        return not_found();
    };
    let command = CirculationCommand::CancelHold(HoldCancellation {
        patron,
        hold: HoldId(hold),
        at: now(&state),
        by_patron: true,
    });
    match tokio::task::spawn_blocking(move || desk.patron_hold(command)).await {
        Ok(Ok(_)) => no_content(),
        Ok(Err(CirculationDeskError::Refused(CirculationRefusal::Hold(HoldRefused::SetAside)))) => {
            refused("set_aside")
        }
        // Someone else's hold, or one already gone.
        Ok(Err(CirculationDeskError::Refused(_))) => error(StatusCode::NOT_FOUND, "not_found"),
        Ok(Err(_)) | Err(_) => unavailable(),
    }
}

#[derive(Serialize)]
struct RenewedJson {
    item: i64,
    due: String,
    due_at: String,
    renewals: u32,
}

/// `POST /api/v1/patron/loans/{item}/renewal` (item 17): only a loan of the patron's own,
/// found among their loans, so that another patron's Item is `not_found`.
pub(crate) async fn renew(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(item): Path<String>,
) -> Response {
    let Some(desk) = state.loans.clone() else {
        return not_found();
    };
    let Some((_, patron)) = bearer(&state, &headers) else {
        return unauthorized();
    };
    let patron = patron.id;
    let Ok(item) = item.parse::<i64>().map(ItemId) else {
        return not_found();
    };
    let at = now(&state);
    let done = tokio::task::spawn_blocking(move || {
        let loans = desk.loans(patron)?;
        let Some(loan) = loans.into_iter().find(|loan| loan.item == item) else {
            return Ok(None);
        };
        let request = RenewRequest {
            barcode: loan.barcode,
            override_limit: false,
            at,
            by_patron: Some(patron),
            base: state.inner.config.renewal_base,
        };
        desk.renew_own(patron, request).map(Some)
    })
    .await;
    match done {
        Ok(Ok(Some(renewed))) => private(
            StatusCode::OK,
            &RenewedJson {
                item: item.0,
                due: renewed.due.date().to_string(),
                due_at: renewed.due.timestamp().to_string(),
                renewals: renewed.renewals,
            },
        ),
        Ok(Ok(None)) => error(StatusCode::NOT_FOUND, "not_found"),
        Ok(Err(CirculationDeskError::Refused(CirculationRefusal::Renew(refusal)))) => {
            refused(refusal_code(&refusal))
        }
        Ok(Err(_)) | Err(_) => unavailable(),
    }
}

/// Body of a suspension: the day it ends, or none.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SuspensionBody {
    /// `YYYY-MM-DD`.
    #[serde(default)]
    until: Option<String>,
}

/// `PUT /api/v1/patron/holds/{id}/suspension` (item 16a): suspends a hold of the patron's
/// own not yet set aside, until a day or not.
pub(crate) async fn suspend_hold(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(hold): Path<String>,
    body: Result<Json<SuspensionBody>, JsonRejection>,
) -> Response {
    let Some((patron, hold)) = suspension_target(&state, &headers, &hold) else {
        return suspension_denied(&state, &headers);
    };
    let Ok(Json(body)) = body else {
        return bad_request();
    };
    let suspension = match body.until.as_deref().map(str::parse) {
        None => Suspension::Indefinite,
        Some(Ok(day)) => Suspension::Until(day),
        Some(Err(_)) => return bad_request(),
    };
    change_suspension(&state, patron, hold, SuspensionChange::Suspend(suspension)).await
}

/// `DELETE /api/v1/patron/holds/{id}/suspension` (item 16a): resumes a hold of the
/// patron's own.
pub(crate) async fn resume_hold(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(hold): Path<String>,
) -> Response {
    let Some((patron, hold)) = suspension_target(&state, &headers, &hold) else {
        return suspension_denied(&state, &headers);
    };
    change_suspension(&state, patron, hold, SuspensionChange::Resume).await
}

/// The patron and the hold of a suspension request, when the token is good, the library
/// lets patrons suspend and the id is a number.
fn suspension_target(
    state: &AppState,
    headers: &HeaderMap,
    hold: &str,
) -> Option<(PatronId, HoldId)> {
    state.loans.as_ref()?;
    if !state.inner.config.patrons_suspend_holds {
        return None;
    }
    let (_, patron) = bearer(state, headers)?;
    Some((patron.id, HoldId(hold.parse().ok()?)))
}

/// Why `suspension_target` found nothing: no token, or nothing to act on.
fn suspension_denied(state: &AppState, headers: &HeaderMap) -> Response {
    if state.loans.is_some() && bearer(state, headers).is_none() {
        unauthorized()
    } else {
        not_found()
    }
}

/// Runs `circ.suspend_hold`: 204 when done, `set_aside` for a hold set aside, and
/// `not_found` for another patron's hold or one already gone.
async fn change_suspension(
    state: &AppState,
    patron: PatronId,
    hold: HoldId,
    change: SuspensionChange,
) -> Response {
    let Some(desk) = state.loans.clone() else {
        return not_found();
    };
    let command = CirculationCommand::SuspendHold(HoldSuspensionRequest {
        patron,
        hold,
        change,
    });
    match tokio::task::spawn_blocking(move || desk.patron_hold(command)).await {
        Ok(Ok(_)) => no_content(),
        Ok(Err(CirculationDeskError::Refused(CirculationRefusal::Hold(
            HoldRefused::SuspendSetAside,
        )))) => refused("set_aside"),
        Ok(Err(CirculationDeskError::Refused(_))) => error(StatusCode::NOT_FOUND, "not_found"),
        Ok(Err(_)) | Err(_) => unavailable(),
    }
}
