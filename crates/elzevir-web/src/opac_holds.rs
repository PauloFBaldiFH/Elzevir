// SPDX-License-Identifier: GPL-3.0-or-later
//! The patron's holds on the OPAC, and their choice about the reading history
//! (docs/specs/opac/conta-do-leitor.md, items 21 to 27).

use axum::Form;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use elzevir_app::circulation_rules::{
    CirculationCommand, CirculationDeskError, CirculationRefusal,
};
use elzevir_app::holds::{
    HoldCancellation, HoldId, HoldLevel, HoldRefused, HoldRequest, HoldSuspensionRequest,
    PatronHold, Suspension, SuspensionChange,
};
use elzevir_app::opac;
use elzevir_app::patron_account::PatronIdentity;
use elzevir_app::patrons::ReadingHistory;
use elzevir_domain::catalog::{BranchId, InstanceId};
use elzevir_domain::hold::{Found, HoldRefusal};
use elzevir_web_templates::{HoldSuspensionAction, I18n, OpacHoldForm, OpacHoldRow, PickupChoice};
use serde::Deserialize;

use crate::circulation::now;
use crate::patron_login::{current_patron, no_store};
use crate::patrons::shown;
use crate::security::tokens_match;
use crate::session::Live;
use crate::{AppState, render};

/// The patron's holds as their account page shows them, with the suspension buttons when
/// the library lets patrons suspend (`suspend`).
pub(crate) fn hold_rows(t: &I18n, holds: &[PatronHold], suspend: bool) -> Vec<OpacHoldRow> {
    holds
        .iter()
        .map(|hold| OpacHoldRow {
            id: hold.id.0,
            title: hold.title.clone(),
            pickup: hold.pickup.clone(),
            status: status(t, hold),
            cancelable: hold.found.is_none(),
            suspension: match (suspend, hold.found, hold.suspended) {
                (false, _, _) | (true, Some(_), _) => HoldSuspensionAction::None,
                (true, None, Some(_)) => HoldSuspensionAction::Resume,
                (true, None, None) => HoldSuspensionAction::Suspend,
            },
        })
        .collect()
}

/// Where the hold stands, as the patron reads it.
fn status(t: &I18n, hold: &PatronHold) -> String {
    let position = hold.priority.to_string();
    match (hold.found, hold.expires_on, hold.suspended) {
        (Some(Found::Waiting), Some(until), _) => {
            t.get_with("opac-hold-waiting", "date", &shown(t, until))
        }
        (Some(_), _, _) => t.get("holds-in-transit"),
        (None, _, Some(Suspension::Until(day))) => t.get_texts(
            "holds-suspended-until",
            &[("date", &shown(t, day)), ("position", &position)],
        ),
        (None, _, Some(Suspension::Indefinite)) => {
            t.get_with("holds-suspended", "position", &position)
        }
        (None, _, None) => t.get_with("holds-position", "position", &position),
    }
}

/// A hold refusal told to the patron.
const fn refusal_key(refusal: &HoldRefused) -> &'static str {
    match refusal {
        HoldRefused::UnknownPatron | HoldRefused::UnknownBarcode | HoldRefused::WrongItem => {
            "opac-hold-unavailable"
        }
        HoldRefused::UnknownBranch | HoldRefused::Rules(HoldRefusal::PickupNotValid) => {
            "opac-hold-pickup"
        }
        HoldRefused::UnknownHold => "opac-hold-unknown",
        HoldRefused::SetAside | HoldRefused::SuspendSetAside => "opac-hold-set-aside",
        HoldRefused::Rules(refusal) => match refusal {
            HoldRefusal::Restricted => "opac-hold-restricted",
            HoldRefusal::DebtLimit { .. } => "opac-hold-debt",
            HoldRefusal::ItemAlreadyOnHold | HoldRefusal::TooManyHoldsForThisRecord { .. } => {
                "opac-hold-already"
            }
            HoldRefusal::HoldLimit { .. }
            | HoldRefusal::TooManyReservesToday { .. }
            | HoldRefusal::TooManyReserves { .. } => "opac-hold-too-many",
            _ => "opac-hold-not-allowed",
        },
    }
}

/// What the hold form shows: the title, the barcode a record hold is placed through, and
/// the pickup branches with the patron's own chosen.
struct Record {
    title: String,
    barcode: Option<String>,
    pickups: Vec<PickupChoice>,
}

fn read_record(
    state: &AppState,
    patron: &PatronIdentity,
    id: InstanceId,
) -> Result<Option<Record>, ()> {
    let catalog = &*state.inner.opac.catalog;
    let Some(page) = opac::record(catalog, id).map_err(drop)? else {
        return Ok(None);
    };
    let codes = catalog.codes().map_err(drop)?;
    let home = state
        .accounts
        .as_ref()
        .and_then(|accounts| accounts.account(patron.id).ok().flatten())
        .map(|account| account.branch_name);
    Ok(Some(Record {
        title: page.instance.title,
        // A record hold names the record through any of its Items (Koha: `opac-reserve.pl`
        // holds the record, not an Item).
        barcode: page.items.into_iter().find_map(|item| item.barcode),
        pickups: codes
            .branches
            .into_iter()
            .map(|branch| PickupChoice {
                selected: home.as_deref() == Some(branch.name.as_str()),
                id: branch.id.0,
                name: branch.name,
            })
            .collect(),
    }))
}

fn form_page(
    state: &AppState,
    session: &Live<PatronIdentity>,
    id: InstanceId,
    record: Record,
    error: Option<&'static str>,
) -> Response {
    let t = &state.inner.i18n;
    no_store(render(&OpacHoldForm {
        t,
        csrf: &session.csrf,
        action: format!("/conta/reservar/{}", id.0),
        record_url: format!("/record/{}", id.0),
        title: record.title,
        pickups: record.pickups,
        error: error.map(|key| t.get(key)),
    }))
}

async fn opened(
    state: &AppState,
    session: &Live<PatronIdentity>,
    id: i64,
) -> Result<Record, Response> {
    let worker = state.clone();
    let identity = session.identity.clone();
    match tokio::task::spawn_blocking(move || read_record(&worker, &identity, InstanceId(id))).await
    {
        Ok(Ok(Some(record))) => Ok(record),
        Ok(Ok(None)) => Err(StatusCode::NOT_FOUND.into_response()),
        Ok(Err(())) | Err(_) => Err(StatusCode::SERVICE_UNAVAILABLE.into_response()),
    }
}

/// `GET /conta/reservar/{id}`: the hold form of a record (spec, item 21).
pub(crate) async fn hold_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    if state.loans.is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Some(session) = current_patron(&state, &headers) else {
        return Redirect::to("/conta/entrar").into_response();
    };
    match opened(&state, &session, id).await {
        Ok(record) => form_page(&state, &session, InstanceId(id), record, None),
        Err(response) => response,
    }
}

/// Fields of the hold form.
#[derive(Deserialize)]
pub(crate) struct HoldFields {
    csrf: String,
    pickup: i64,
}

/// `POST /conta/reservar/{id}`: the patron holds the record (spec, items 21 and 22).
pub(crate) async fn place_hold(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Form(form): Form<HoldFields>,
) -> Response {
    let Some(desk) = state.loans.clone() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(session) = current_patron(&state, &headers) else {
        return Redirect::to("/conta/entrar").into_response();
    };
    if !tokens_match(&session.csrf, &form.csrf) {
        return (StatusCode::FORBIDDEN, "invalid form token").into_response();
    }
    let record = match opened(&state, &session, id).await {
        Ok(record) => record,
        Err(response) => return response,
    };
    let Some(barcode) = record.barcode.clone() else {
        return refused_form(&state, &session, id, record, "opac-hold-unavailable");
    };
    let command = CirculationCommand::PlaceHold(HoldRequest {
        patron: session.identity.id,
        barcode,
        level: HoldLevel::Record,
        pickup: BranchId(form.pickup),
        at: now(&state),
    });
    match tokio::task::spawn_blocking(move || desk.patron_hold(command)).await {
        Ok(Ok(_)) => Redirect::to("/conta?aviso=reserva#holds-title").into_response(),
        Ok(Err(CirculationDeskError::Refused(CirculationRefusal::Hold(refusal)))) => {
            refused_form(&state, &session, id, record, refusal_key(&refusal))
        }
        Ok(Err(_)) | Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

fn refused_form(
    state: &AppState,
    session: &Live<PatronIdentity>,
    id: i64,
    record: Record,
    key: &'static str,
) -> Response {
    let mut response = form_page(state, session, InstanceId(id), record, Some(key));
    *response.status_mut() = StatusCode::UNPROCESSABLE_ENTITY;
    response
}

/// Fields of a button that only needs the token.
#[derive(Deserialize)]
pub(crate) struct TokenFields {
    csrf: String,
}

/// `POST /conta/reservas/{hold}/cancelar`: the patron cancels a hold not yet set aside
/// (spec, item 23).
pub(crate) async fn cancel_hold(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(hold): Path<i64>,
    Form(form): Form<TokenFields>,
) -> Response {
    let Some(desk) = state.loans.clone() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(session) = current_patron(&state, &headers) else {
        return Redirect::to("/conta/entrar").into_response();
    };
    if !tokens_match(&session.csrf, &form.csrf) {
        return (StatusCode::FORBIDDEN, "invalid form token").into_response();
    }
    let command = CirculationCommand::CancelHold(HoldCancellation {
        patron: session.identity.id,
        hold: HoldId(hold),
        at: now(&state),
        by_patron: true,
    });
    let code = match tokio::task::spawn_blocking(move || desk.patron_hold(command)).await {
        Ok(Ok(_)) => "reserva_cancelada",
        Ok(Err(CirculationDeskError::Refused(CirculationRefusal::Hold(HoldRefused::SetAside)))) => {
            "reserva_separada"
        }
        // Someone else's hold, or one already gone: nothing to tell.
        Ok(Err(CirculationDeskError::Refused(_))) => return Redirect::to("/conta").into_response(),
        Ok(Err(_)) | Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    Redirect::to(&format!("/conta?aviso={code}#holds-title")).into_response()
}

/// Fields of the suspend button.
#[derive(Deserialize)]
pub(crate) struct SuspendFields {
    csrf: String,
    /// The day it ends, `YYYY-MM-DD`; empty for no end.
    #[serde(default)]
    until: String,
}

/// `POST /conta/reservas/{hold}/suspender`: the patron suspends a hold not yet set aside,
/// until a day or not (spec, item 25a).
pub(crate) async fn suspend_hold(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(hold): Path<i64>,
    Form(form): Form<SuspendFields>,
) -> Response {
    let suspension = match form.until.trim() {
        "" => Suspension::Indefinite,
        day => match day.parse() {
            Ok(day) => Suspension::Until(day),
            Err(_) => return (StatusCode::BAD_REQUEST, "invalid date").into_response(),
        },
    };
    change_suspension(
        &state,
        &headers,
        hold,
        &form.csrf,
        SuspensionChange::Suspend(suspension),
    )
    .await
}

/// `POST /conta/reservas/{hold}/retomar`: the patron resumes a suspended hold (spec, item
/// 25a).
pub(crate) async fn resume_hold(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(hold): Path<i64>,
    Form(form): Form<TokenFields>,
) -> Response {
    change_suspension(&state, &headers, hold, &form.csrf, SuspensionChange::Resume).await
}

/// Runs `circ.suspend_hold` for the signed-in patron's own hold, and goes back to the
/// account with the notice.
async fn change_suspension(
    state: &AppState,
    headers: &HeaderMap,
    hold: i64,
    csrf: &str,
    change: SuspensionChange,
) -> Response {
    let Some(desk) = state.loans.clone() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !state.inner.config.patrons_suspend_holds {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Some(session) = current_patron(state, headers) else {
        return Redirect::to("/conta/entrar").into_response();
    };
    if !tokens_match(&session.csrf, csrf) {
        return (StatusCode::FORBIDDEN, "invalid form token").into_response();
    }
    let code = match change {
        SuspensionChange::Suspend(_) => "reserva_suspensa",
        SuspensionChange::Resume => "reserva_retomada",
    };
    let command = CirculationCommand::SuspendHold(HoldSuspensionRequest {
        patron: session.identity.id,
        hold: HoldId(hold),
        change,
    });
    let code = match tokio::task::spawn_blocking(move || desk.patron_hold(command)).await {
        Ok(Ok(_)) => code,
        Ok(Err(CirculationDeskError::Refused(CirculationRefusal::Hold(
            HoldRefused::SuspendSetAside,
        )))) => "suspensao_separada",
        // Someone else's hold, or one already gone: nothing to tell.
        Ok(Err(CirculationDeskError::Refused(_))) => return Redirect::to("/conta").into_response(),
        Ok(Err(_)) | Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    Redirect::to(&format!("/conta?aviso={code}#holds-title")).into_response()
}

/// Fields of the reading-history form.
#[derive(Deserialize)]
pub(crate) struct HistoryFields {
    csrf: String,
    /// Present when the patron agrees to keep the history.
    keep: Option<String>,
}

/// `POST /conta/historico`: the patron gives or takes back consent (spec, items 25 to 27).
pub(crate) async fn set_history(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<HistoryFields>,
) -> Response {
    let Some(accounts) = state.accounts.clone() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(session) = current_patron(&state, &headers) else {
        return Redirect::to("/conta/entrar").into_response();
    };
    if !tokens_match(&session.csrf, &form.csrf) {
        return (StatusCode::FORBIDDEN, "invalid form token").into_response();
    }
    let choice = if form.keep.is_some() {
        ReadingHistory::Kept
    } else {
        ReadingHistory::NotKept
    };
    let patron = session.identity.id;
    match tokio::task::spawn_blocking(move || accounts.set_reading_history(patron, choice)).await {
        Ok(Ok(())) => {
            let code = if choice == ReadingHistory::Kept {
                "historico_sim"
            } else {
                "historico_nao"
            };
            Redirect::to(&format!("/conta?aviso={code}#history-title")).into_response()
        }
        Ok(Err(_)) | Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}
