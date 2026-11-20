// SPDX-License-Identifier: GPL-3.0-or-later
//! The patron's side of the wall: loans, lending by barcode, renewing and returning, and
//! the checkin desk (interface §18.3; docs/specs/circulacao/emprestimo.md,
//! devolucao.md, renovacao.md and suspensao.md).

use std::sync::Arc;

use axum::Form;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use elzevir_app::circulation_rules::{
    CirculationCommand, CirculationDeskError, CirculationOutput, CirculationRefusal,
};
use elzevir_app::loans::{
    CheckedIn, CheckinRequest, CheckoutRefusal, CheckoutRequest, LoanDesk, RenewRequest,
};
use elzevir_app::patrons::{PatronCommand, PatronDesk};
use elzevir_app::staff::Permission;
use elzevir_domain::catalog::{BranchId, Money};
use elzevir_domain::checkin::{Destination, LoanOutcome, LostOutcome};
use elzevir_domain::checkout::{Alert, Confirm, HeldFor, Impossible, TooMany};
use elzevir_domain::hold::HoldMatch;
use elzevir_domain::patron::{PatronId, is_expired};
use elzevir_domain::suspension::{RestrictionKind, SuspensionEnd};
use elzevir_web_templates::{
    AsideForm, DeskOption, I18n, Notice, RestrictionRow, StaffCheckin, StaffPatron,
};
use jiff::Zoned;
use serde::Deserialize;

use crate::patron_account::account_rows;
use crate::patron_card::{ZoomScreen, header, loan_line, zoom};
use crate::patrons::{failed, full_name, shown, today};
use crate::security::tokens_match;
use crate::session::ActiveSession;
use crate::{AppState, render};

/// What the last circulation action left on the page.
#[derive(Default)]
pub(crate) struct Outcome {
    pub(crate) notices: Vec<Notice>,
    /// The barcode to keep in the box, when a checkout waits for confirmation.
    pub(crate) barcode: String,
    pub(crate) confirm: bool,
    pub(crate) desk: Option<BranchId>,
    /// The barcode just lent, whose cord is drawn.
    pub(crate) lent: Option<String>,
}

pub(crate) fn now(state: &AppState) -> Zoned {
    jiff::Timestamp::now().to_zoned(state.inner.config.time_zone.clone())
}

/// A due date as shown: the day, and the time unless it is the end of the day.
pub(crate) fn due_text(t: &I18n, due: &Zoned) -> String {
    let day = shown(t, due.date());
    if (due.hour(), due.minute()) == (23, 59) {
        day
    } else {
        format!("{day} {}", due.strftime("%H:%M"))
    }
}

pub(crate) fn ok(text: String) -> Notice {
    Notice {
        refused: false,
        text,
    }
}

pub(crate) fn refused(text: String) -> Notice {
    Notice {
        refused: true,
        text,
    }
}

pub(crate) fn suspension_text(
    t: &I18n,
    end: SuspensionEnd,
    day_key: &str,
    lifted_key: &str,
) -> String {
    match end {
        SuspensionEnd::Day(day) => t.get_with(day_key, "date", &shown(t, day)),
        SuspensionEnd::WhenLifted => t.get(lifted_key),
    }
}

/// An amount as the interface shows it: two decimals, with the language's separator.
pub(crate) fn money(t: &I18n, Money(cents): Money) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let cents = cents.unsigned_abs();
    format!(
        "{sign}{}{}{:02}",
        cents / 100,
        t.get("money-decimal-separator"),
        cents % 100
    )
}

fn limit_text(t: &I18n, key: &str, too_many: &TooMany) -> String {
    t.get_texts(
        key,
        &[
            ("count", &too_many.count.to_string()),
            ("max", &too_many.max_allowed),
        ],
    )
}

fn impossible_text(t: &I18n, reason: &Impossible) -> String {
    match reason {
        Impossible::Suspended(end) => suspension_text(
            t,
            *end,
            "checkout-impossible-suspended",
            "checkout-impossible-suspended-lifted",
        ),
        Impossible::PatronExpired => t.get("checkout-impossible-expired"),
        Impossible::Debt(owed) => {
            t.get_with("checkout-impossible-debt", "amount", &money(t, *owed))
        }
        Impossible::TooMany(too_many) => limit_text(t, "checkout-impossible-too-many", too_many),
        Impossible::NotForLoan(_) => t.get("checkout-impossible-not-for-loan"),
        Impossible::Withdrawn => t.get("checkout-impossible-withdrawn"),
        Impossible::Restricted => t.get("checkout-impossible-restricted"),
    }
}

fn confirm_text(t: &I18n, reason: &Confirm) -> String {
    match reason {
        Confirm::Renew => t.get("checkout-confirm-renew"),
        Confirm::IssuedToAnother => t.get("checkout-confirm-issued-to-another"),
        Confirm::Debt(owed) => t.get_with("checkout-confirm-debt", "amount", &money(t, *owed)),
        Confirm::PatronCannotBorrow => t.get("checkout-confirm-cannot-borrow"),
        Confirm::TooMany(too_many) => limit_text(t, "checkout-confirm-too-many", too_many),
        Confirm::NotForLoan(_) => t.get("checkout-confirm-not-for-loan"),
        Confirm::Lost(_) => t.get("checkout-confirm-lost"),
        Confirm::Held(HeldFor::Waiting) => t.get("checkout-confirm-held-waiting"),
        Confirm::Held(HeldFor::InTransit) => t.get("checkout-confirm-held-in-transit"),
        Confirm::Held(HeldFor::Reserved) => t.get("checkout-confirm-held"),
    }
}

/// The notices of a refused command.
pub(crate) fn refusal_notices(t: &I18n, refusal: &CirculationRefusal) -> Vec<Notice> {
    match refusal {
        CirculationRefusal::Checkout(CheckoutRefusal::Impossible(reasons)) => reasons
            .iter()
            .map(|reason| refused(impossible_text(t, reason)))
            .collect(),
        CirculationRefusal::Checkout(CheckoutRefusal::NeedsConfirmation(reasons)) => reasons
            .iter()
            .map(|reason| refused(confirm_text(t, reason)))
            .collect(),
        other => vec![refused(t.get(other.message_key()))],
    }
}

/// The notices of a checkin.
fn checkin_notices(
    t: &I18n,
    barcode: &str,
    done: &CheckedIn,
    branches: &[DeskOption],
) -> Vec<Notice> {
    let branch_name = |id: BranchId| {
        branches
            .iter()
            .find(|branch| branch.id == id.0)
            .map(|branch| branch.name.clone())
            .unwrap_or_default()
    };
    let mut notices = vec![match done.loan {
        LoanOutcome::NotOnLoan => ok(t.get_with("checkin-not-on-loan", "barcode", barcode)),
        LoanOutcome::Ended { .. } | LoanOutcome::KeptLost => {
            ok(t.get_with("checkin-done", "barcode", barcode))
        }
    }];
    if done.loan == (LoanOutcome::Ended { overdue: true }) {
        notices.push(refused(t.get("checkin-overdue")));
    }
    if let Some(fine) = done.fine {
        notices.push(refused(t.get_with(
            "checkin-fined",
            "amount",
            &money(t, fine),
        )));
    }
    if let Some(until) = done.suspended_until {
        notices.push(refused(t.get_with(
            "checkin-suspended",
            "date",
            &shown(t, until),
        )));
    }
    match done.lost {
        LostOutcome::Found => notices.push(ok(t.get("checkin-found"))),
        LostOutcome::StaysLost => notices.push(refused(t.get("checkin-kept-lost"))),
        LostOutcome::NotLost => {}
    }
    if let Some(hold) = done.hold {
        let pickup = branch_name(hold.pickup);
        notices.push(refused(match hold.found {
            HoldMatch::Reserved(_) => t.get_with("checkin-hold-reserved", "branch", &pickup),
            HoldMatch::InTransit(_) => t.get_with("checkin-hold-in-transit", "branch", &pickup),
            HoldMatch::Waiting(_) => t.get("checkin-hold-waiting"),
        }));
    }
    let (key, branch) = match done.destination {
        Destination::Stays => return notices,
        Destination::Sent { to, .. } => ("checkin-send", to),
        Destination::Needs { to, .. } => ("checkin-needs", to),
        Destination::Arrived { from } => ("checkin-arrived", from),
        Destination::WrongBranch { to } => ("checkin-wrong-branch", to),
    };
    notices.push(refused(t.get_with(key, "branch", &branch_name(branch))));
    notices
}

/// The branches with their names, and the one the desk is at.
async fn desks(state: &AppState, at: Option<BranchId>) -> Result<Vec<DeskOption>, Response> {
    let catalog = Arc::clone(&state.inner.opac.catalog);
    let Ok(Ok(codes)) = tokio::task::spawn_blocking(move || catalog.codes()).await else {
        return Err(StatusCode::SERVICE_UNAVAILABLE.into_response());
    };
    let selected = at.or_else(|| codes.branches.first().map(|branch| branch.id));
    Ok(codes
        .branches
        .into_iter()
        .map(|branch| DeskOption {
            id: branch.id.0,
            selected: Some(branch.id) == selected,
            name: branch.name,
        })
        .collect())
}

/// The session, the patron desk and the loan desk.
pub(crate) type Opened = (ActiveSession, Arc<dyn PatronDesk>, Arc<dyn LoanDesk>);

/// The circulation routes.
pub(crate) fn routes() -> axum::Router<AppState> {
    use axum::routing::{get, post};
    axum::Router::new()
        .route("/equipe/leitores/{id}", get(record))
        .route("/equipe/leitores/{id}/circulacao", post(circulate))
        .route(
            "/equipe/leitores/{id}/conta",
            post(crate::patron_account::pay),
        )
        .route(
            "/equipe/leitores/{id}/reservas",
            post(crate::patron_holds::change),
        )
        .route(
            "/equipe/leitores/{id}/recibo",
            get(crate::checkout_slip::slip),
        )
        .route(
            "/equipe/leitores/{id}/restricoes",
            post(crate::patron_holds::lift_restriction),
        )
        .route(
            "/equipe/circulacao/devolucao",
            get(checkin_desk).post(check_in),
        )
        .route(
            "/equipe/circulacao/avisos",
            get(crate::print_notices::print_notices).post(crate::print_notices::mark_printed),
        )
        .route(
            "/equipe/circulacao/devolucao/reserva",
            post(crate::patron_holds::set_aside),
        )
        .route("/equipe/acervo/emprestar", post(crate::pill_loans::lend))
        .route(
            "/equipe/circulacao/exemplar/{id}",
            get(crate::pill_loans::to_patron),
        )
}

/// The desks, for who has the circulation permission.
pub(crate) fn open(state: &AppState, headers: &HeaderMap) -> Result<Opened, Box<Response>> {
    let (session, patrons) = crate::patrons::open(state, headers, |session| {
        session.identity.can(Permission::Circulation)
    })?;
    let Some(loans) = state.loans.clone() else {
        return Err(Box::new(StatusCode::NOT_FOUND.into_response()));
    };
    Ok((session, patrons, loans))
}

/// `GET /equipe/leitores/{id}`: the record, only with the circulation permission, and every
/// opening in the audit log (interface §18.1).
pub(crate) async fn record(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    match open(&state, &headers) {
        Ok((session, patrons, loans)) => {
            page(
                &state,
                &session,
                &headers,
                (patrons, loans),
                PatronId(id),
                Outcome::default(),
            )
            .await
        }
        Err(response) => *response,
    }
}

pub(crate) async fn page(
    state: &AppState,
    session: &ActiveSession,
    headers: &HeaderMap,
    (patrons, loans): (Arc<dyn PatronDesk>, Arc<dyn LoanDesk>),
    id: PatronId,
    outcome: Outcome,
) -> Response {
    let actor = session.identity.id;
    let day = today(state);
    let read = tokio::task::spawn_blocking(move || {
        // The opening is logged before the data leaves the server.
        patrons.apply(actor, PatronCommand::View(id))?;
        let Some(patron) = patrons.patron(id)? else {
            return Ok(None);
        };
        let current = loans.loans(id).map_err(unavailable)?;
        let suspended = loans.restrictions(id, day).map_err(unavailable)?;
        let owed = loans.owed(id).map_err(unavailable)?;
        let account = loans.account(id).map_err(unavailable)?;
        let holds = loans.holds(id).map_err(unavailable)?;
        Ok(Some((patron, current, suspended, (owed, account, holds))))
    })
    .await;
    let (patron, current, suspended, (owed, account, holds)) = match read {
        Ok(Ok(Some(read))) => read,
        Ok(Ok(None)) => return StatusCode::NOT_FOUND.into_response(),
        Ok(Err(error)) => return failed(&error),
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    let desks = match desks(state, outcome.desk.or(Some(patron.fields.branch))).await {
        Ok(desks) => desks,
        Err(response) => return response,
    };
    let t = &state.inner.i18n;
    let moment = now(state);
    // Only a refusal comes first; an action that went through may still ask for attention.
    let status = if outcome.notices.first().is_some_and(|notice| notice.refused) {
        StatusCode::UNPROCESSABLE_ENTITY
    } else {
        StatusCode::OK
    };
    let expired = patron
        .fields
        .expires
        .is_some_and(|expires| is_expired(expires, day));
    let mut response = render(&StaffPatron {
        t,
        name: full_name(&patron.fields.surname, patron.fields.first_name.as_deref()),
        header: header(t, &patron, &current, &moment, (owed, expired)),
        zoom: zoom(headers, ZoomScreen::PatronRecord),
        expired,
        edit_url: session
            .identity
            .can(Permission::Patrons)
            .then(|| format!("/equipe/leitores/{}/editar", id.0)),
        id: id.0,
        csrf: &session.csrf,
        restrictions: restriction_rows(t, &suspended),
        owed: (owed.0 > 0).then(|| t.get_with("patron-owes", "amount", &money(t, owed))),
        loans: current
            .iter()
            .map(|loan| loan_line(t, loan, &moment, outcome.lent.as_deref()))
            .collect(),
        desks,
        barcode: outcome.barcode,
        notices: outcome.notices,
        confirm: outcome.confirm,
        account: account_rows(t, &account),
        holds: crate::patron_holds::hold_rows(t, &holds),
    });
    if response.status() == StatusCode::OK {
        *response.status_mut() = status;
    }
    response
}

/// The patron's restrictions as shown: a suspension with its end, or the restriction of
/// the overdue notices.
fn restriction_rows(
    t: &I18n,
    restrictions: &[(RestrictionKind, SuspensionEnd)],
) -> Vec<RestrictionRow> {
    restrictions
        .iter()
        .map(|(kind, end)| RestrictionRow {
            kind: kind.as_str(),
            text: match kind {
                RestrictionKind::Suspension => {
                    suspension_text(t, *end, "patron-suspended", "patron-suspended-lifted")
                }
                RestrictionKind::Overdues => t.get("patron-restricted-overdues"),
            },
        })
        .collect()
}

pub(crate) fn unavailable(error: CirculationDeskError) -> elzevir_app::patrons::PatronDeskError {
    match error {
        CirculationDeskError::Busy => elzevir_app::patrons::PatronDeskError::Busy,
        other => elzevir_app::patrons::PatronDeskError::Unavailable(other.to_string()),
    }
}

/// One posted circulation form of the patron's record.
#[derive(Deserialize)]
pub(crate) struct CirculationForm {
    csrf: String,
    op: String,
    #[serde(default)]
    barcode: String,
    desk: i64,
}

/// `POST /equipe/leitores/{id}/circulacao`: lends, confirms, renews or returns, and shows
/// the record again with what happened.
pub(crate) async fn circulate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Form(form): Form<CirculationForm>,
) -> Response {
    let (session, patrons, loans) = match open(&state, &headers) {
        Ok(opened) => opened,
        Err(response) => return *response,
    };
    if !tokens_match(&session.csrf, &form.csrf) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let patron = PatronId(id);
    let desk = BranchId(form.desk);
    let at = now(&state);
    let command = match form.op.split_once(':') {
        None if form.op == "lend" || form.op == "confirm" => {
            CirculationCommand::Checkout(CheckoutRequest {
                patron,
                barcode: form.barcode.clone(),
                desk,
                on_site: false,
                confirmed: form.op == "confirm",
                at,
            })
        }
        Some(("renew", barcode)) => CirculationCommand::Renew(RenewRequest {
            barcode: barcode.to_owned(),
            override_limit: false,
            at,
            by_patron: None,
        }),
        Some(("return", barcode)) => CirculationCommand::Checkin(CheckinRequest {
            barcode: barcode.to_owned(),
            desk,
            at,
        }),
        _ => return (StatusCode::BAD_REQUEST, "unknown action").into_response(),
    };
    let actor = session.identity.id;
    let writer = Arc::clone(&loans);
    let done = tokio::task::spawn_blocking(move || writer.circulate(actor, command)).await;
    let t = &state.inner.i18n;
    let mut outcome = Outcome {
        desk: Some(desk),
        ..Outcome::default()
    };
    match done {
        Ok(Ok(CirculationOutput::CheckedOut(lent))) => {
            outcome.notices.push(ok(t.get_texts(
                "checkout-done",
                &[
                    ("date", &due_text(t, &lent.due)),
                    ("barcode", form.barcode.trim()),
                ],
            )));
            outcome.notices.extend(lent.alerts.iter().map(|alert| {
                ok(t.get(match alert {
                    Alert::Lost(_) => "checkout-alert-lost",
                    Alert::ReturnedFromAnother => "checkout-alert-returned",
                }))
            }));
            outcome.lent = Some(form.barcode.trim().to_owned());
        }
        Ok(Ok(CirculationOutput::Renewed(renewed))) => outcome.notices.push(ok(t.get_with(
            "renew-done",
            "date",
            &due_text(t, &renewed.due),
        ))),
        Ok(Ok(CirculationOutput::CheckedIn(back))) => {
            let barcode = form.op.split_once(':').map_or("", |(_, barcode)| barcode);
            let names = match desks(&state, None).await {
                Ok(names) => names,
                Err(response) => return response,
            };
            outcome.notices = checkin_notices(t, barcode, &back, &names);
        }
        Ok(Ok(_)) => {}
        Ok(Err(CirculationDeskError::Refused(refusal))) => {
            outcome.notices = refusal_notices(t, &refusal);
            if let CirculationRefusal::Checkout(CheckoutRefusal::NeedsConfirmation(_)) = refusal {
                outcome.confirm = true;
                outcome.barcode = form.barcode;
            }
        }
        Ok(Err(CirculationDeskError::Busy)) => {
            outcome.notices.push(refused(t.get("circulation-busy")));
        }
        Ok(Err(CirculationDeskError::Unavailable(_))) | Err(_) => {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    }
    page(
        &state,
        &session,
        &headers,
        (patrons, loans),
        patron,
        outcome,
    )
    .await
}

/// `GET /equipe/circulacao/devolucao`: the checkin desk.
pub(crate) async fn checkin_desk(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(prefill): Query<CheckinPrefill>,
) -> Response {
    match open(&state, &headers) {
        Ok((session, _, _)) => {
            let view = CheckinView {
                barcode: prefill.codigo.trim(),
                ..CheckinView::default()
            };
            checkin_page(&state, &session, None, view).await
        }
        Err(response) => *response,
    }
}

/// The barcode the staff header sends to the desk (interface §22 item 2).
#[derive(Deserialize)]
pub(crate) struct CheckinPrefill {
    #[serde(default)]
    codigo: String,
}

/// What the desk shows besides its form: the last action's notices, the hold to set aside
/// and a barcode to fill in.
#[derive(Default)]
pub(crate) struct CheckinView<'a> {
    pub(crate) notices: Vec<Notice>,
    pub(crate) aside: Option<AsideForm>,
    pub(crate) barcode: &'a str,
}

/// The checkin desk, with what the last action did and, when the Item answers a hold, the
/// button that sets it aside.
pub(crate) async fn checkin_page(
    state: &AppState,
    session: &ActiveSession,
    desk: Option<BranchId>,
    CheckinView {
        notices,
        aside,
        barcode,
    }: CheckinView<'_>,
) -> Response {
    let desks = match desks(state, desk).await {
        Ok(desks) => desks,
        Err(response) => return response,
    };
    // Only a refusal comes first; a checkin that went through may still ask for attention.
    let refused = notices.first().is_some_and(|notice| notice.refused);
    let mut response = render(&StaffCheckin {
        t: &state.inner.i18n,
        csrf: &session.csrf,
        desks,
        notices,
        aside,
        barcode,
    });
    if refused && response.status() == StatusCode::OK {
        *response.status_mut() = StatusCode::UNPROCESSABLE_ENTITY;
    }
    response
}

/// One scanned return.
#[derive(Deserialize)]
pub(crate) struct CheckinForm {
    csrf: String,
    barcode: String,
    desk: i64,
}

/// `POST /equipe/circulacao/devolucao`: returns the scanned Item and shows the desk again,
/// at the same branch, with what happened. No patron data is shown.
pub(crate) async fn check_in(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<CheckinForm>,
) -> Response {
    let (session, _, loans) = match open(&state, &headers) {
        Ok(opened) => opened,
        Err(response) => return *response,
    };
    if !tokens_match(&session.csrf, &form.csrf) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let desk = BranchId(form.desk);
    let command = CirculationCommand::Checkin(CheckinRequest {
        barcode: form.barcode.clone(),
        desk,
        at: now(&state),
    });
    let actor = session.identity.id;
    let done = tokio::task::spawn_blocking(move || loans.circulate(actor, command)).await;
    let t = &state.inner.i18n;
    let mut aside = None;
    let notices = match done {
        Ok(Ok(CirculationOutput::CheckedIn(back))) => {
            let names = match desks(&state, None).await {
                Ok(names) => names,
                Err(response) => return response,
            };
            // A hold already waiting here needs nothing more.
            aside = back
                .hold
                .filter(|hold| !matches!(hold.found, HoldMatch::Waiting(_)))
                .map(|hold| AsideForm {
                    hold: hold.found.hold().0,
                    barcode: form.barcode.trim().to_owned(),
                    desk: desk.0,
                });
            checkin_notices(t, form.barcode.trim(), &back, &names)
        }
        Ok(Ok(_)) => Vec::new(),
        Ok(Err(CirculationDeskError::Refused(refusal))) => refusal_notices(t, &refusal),
        Ok(Err(CirculationDeskError::Busy)) => vec![refused(t.get("circulation-busy"))],
        Ok(Err(CirculationDeskError::Unavailable(_))) | Err(_) => {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };
    checkin_page(
        &state,
        &session,
        Some(desk),
        CheckinView {
            notices,
            aside,
            barcode: "",
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amounts_show_two_decimals_with_the_languages_separator() {
        let english = I18n::english().unwrap();
        let portuguese = I18n::portuguese().unwrap();
        assert_eq!(money(&english, Money(250)), "2.50");
        assert_eq!(money(&portuguese, Money(250)), "2,50");
        assert_eq!(money(&english, Money(5)), "0.05");
        assert_eq!(money(&english, Money(-1205)), "-12.05");
        assert_eq!(
            english.get_with("patron-owes", "amount", &money(&english, Money(800))),
            "Owes 8.00."
        );
    }
}
