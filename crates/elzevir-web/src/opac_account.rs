// SPDX-License-Identifier: GPL-3.0-or-later
//! The patron's own account page on the OPAC: who they are, their loans, restrictions
//! and debts, and renewing a loan (docs/specs/opac/conta-do-leitor.md, items 12 to 20).

use axum::Form;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use elzevir_app::accounts::AccountLine;
use elzevir_app::circulation_rules::{CirculationDeskError, CirculationRefusal};
use elzevir_app::holds::PatronHold;
use elzevir_app::loans::{LoanDesk, PatronLoan, RenewRefusal, RenewRequest};
use elzevir_app::patrons::ReadingHistory;
use elzevir_domain::catalog::Money;
use elzevir_domain::patron::PatronId;
use elzevir_domain::renewal::RenewalRefusal;
use elzevir_domain::suspension::{RestrictionKind, SuspensionEnd};
use elzevir_web_templates::{I18n, OpacAccount, OpacLoan, OpacNotice};
use jiff::Zoned;
use jiff::civil::Date;
use serde::Deserialize;

use crate::circulation::{due_text, money, now, suspension_text};
use crate::patron_account::account_rows;
use crate::patron_login::{current_patron, no_store};
use crate::security::tokens_match;
use crate::{AppState, render};

/// What the circulation desk knows about the patron.
struct Circulation {
    loans: Vec<PatronLoan>,
    restrictions: Vec<(RestrictionKind, SuspensionEnd)>,
    owed: Money,
    lines: Vec<AccountLine>,
    holds: Vec<PatronHold>,
}

fn circulation(
    desk: Option<&dyn LoanDesk>,
    patron: PatronId,
    today: Date,
) -> Result<Circulation, CirculationDeskError> {
    let Some(desk) = desk else {
        return Ok(Circulation {
            loans: Vec::new(),
            restrictions: Vec::new(),
            owed: Money(0),
            lines: Vec::new(),
            holds: Vec::new(),
        });
    };
    Ok(Circulation {
        loans: desk.loans(patron)?,
        restrictions: desk.restrictions(patron, today)?,
        owed: desk.owed(patron)?,
        lines: desk.account(patron)?,
        holds: desk.holds(patron)?,
    })
}

/// Query string of `/conta` after a renewal.
#[derive(Deserialize)]
pub(crate) struct AccountParams {
    /// The barcode just renewed.
    renovado: Option<String>,
    /// Why the renewal was refused, as one of the codes of [`refusal_code`].
    recusa: Option<String>,
    /// What a hold or history form did, as one of the codes of [`done_key`].
    aviso: Option<String>,
}

/// `GET /conta`: the patron's own account (spec, items 12 to 16).
pub(crate) async fn account(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<AccountParams>,
) -> Response {
    let Some(accounts) = state.accounts.clone() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(session) = current_patron(&state, &headers) else {
        return Redirect::to("/conta/entrar").into_response();
    };
    let patron = session.identity.id;
    let desk = state.loans.clone();
    let moment = now(&state);
    let today = moment.date();
    let read = tokio::task::spawn_blocking(move || {
        let Some(account) = accounts
            .account(patron)
            .map_err(|error| error.to_string())?
        else {
            return Ok(None);
        };
        let circulation =
            circulation(desk.as_deref(), patron, today).map_err(|error| error.to_string())?;
        Ok::<_, String>(Some((account, circulation)))
    })
    .await;
    let (account, circulation) = match read {
        Ok(Ok(Some(read))) => read,
        // The patron was removed while signed in.
        Ok(Ok(None)) => return Redirect::to("/conta/entrar").into_response(),
        Ok(Err(_)) | Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    let t = &state.inner.i18n;
    no_store(render(&OpacAccount {
        t,
        csrf: &session.csrf,
        name: account.name,
        cardnumber: account.cardnumber,
        branch_name: account.branch_name,
        category_name: account.category_name,
        expires: account.expires.strftime("%d/%m/%Y").to_string(),
        restrictions: circulation
            .restrictions
            .iter()
            .map(|(kind, end)| restriction_text(t, *kind, *end))
            .collect(),
        owed: (circulation.owed.0 > 0)
            .then(|| t.get_with("opac-owes", "amount", &money(t, circulation.owed))),
        notice: notice(t, &params, &circulation.loans),
        can_renew: circulation.restrictions.is_empty(),
        loans: circulation
            .loans
            .iter()
            .map(|loan| loan_line(t, loan, &moment))
            .collect(),
        account: account_rows(t, &circulation.lines),
        holds: crate::opac_holds::hold_rows(t, &circulation.holds),
        keeps_history: account.reading_history == ReadingHistory::Kept,
    }))
}

/// A restriction told to the patron, who cannot lift it.
fn restriction_text(t: &I18n, kind: RestrictionKind, end: SuspensionEnd) -> String {
    match kind {
        RestrictionKind::Suspension => {
            suspension_text(t, end, "opac-suspended", "opac-suspended-lifted")
        }
        RestrictionKind::Overdues => t.get("opac-restricted-overdues"),
    }
}

fn loan_line(t: &I18n, loan: &PatronLoan, now: &Zoned) -> OpacLoan {
    OpacLoan {
        barcode: loan.barcode.clone(),
        title: loan.title.clone(),
        due: due_text(t, &loan.due),
        overdue: loan.due.timestamp() < now.timestamp(),
        renewals_left: t.get_numbers(
            "opac-renewals-left",
            &[("count", u64::from(loan.renewals_left))],
        ),
        renewable: !loan.on_site && loan.renewals_left > 0,
    }
}

/// What the page says after a renewal; an unknown code or barcode says nothing.
fn notice(t: &I18n, params: &AccountParams, loans: &[PatronLoan]) -> Option<OpacNotice> {
    if let Some(barcode) = &params.renovado {
        let text = match loans.iter().find(|loan| &loan.barcode == barcode) {
            Some(loan) => t.get_texts(
                "opac-renewed",
                &[("title", &loan.title), ("date", &due_text(t, &loan.due))],
            ),
            None => t.get("opac-renewed-plain"),
        };
        return Some(OpacNotice {
            refused: false,
            text,
        });
    }
    if let Some((refused, key)) = params.aviso.as_deref().and_then(done_key) {
        return Some(OpacNotice {
            refused,
            text: t.get(key),
        });
    }
    let key = refusal_key(params.recusa.as_deref()?)?;
    Some(OpacNotice {
        refused: true,
        text: t.get(key),
    })
}

/// The notice of a hold or history form after the redirect: whether it was refused, and
/// its text.
fn done_key(code: &str) -> Option<(bool, &'static str)> {
    Some(match code {
        "reserva" => (false, "opac-hold-placed"),
        "reserva_cancelada" => (false, "opac-hold-cancelled"),
        "reserva_separada" => (true, "opac-hold-set-aside"),
        "historico_sim" => (false, "opac-history-started"),
        "historico_nao" => (false, "opac-history-stopped"),
        _ => return None,
    })
}

/// The code a refusal travels as in the address after the redirect.
const fn refusal_code(refusal: &RenewRefusal) -> &'static str {
    match refusal {
        RenewRefusal::UnknownBarcode | RenewRefusal::NotOnLoan => "no_loan",
        RenewRefusal::Refused(RenewalRefusal::OnSite) => "on_site",
        RenewRefusal::Refused(RenewalRefusal::TooMany) => "too_many",
        RenewRefusal::Refused(RenewalRefusal::Overdue) => "overdue",
        RenewRefusal::Refused(RenewalRefusal::TooMuchOwing { .. }) => "owing",
        RenewRefusal::Refused(RenewalRefusal::OnHold) => "on_hold",
        RenewRefusal::Refused(RenewalRefusal::TooSoon { .. }) => "too_soon",
        RenewRefusal::NoDueDate => "no_due_date",
        RenewRefusal::Restricted => "restricted",
    }
}

fn refusal_key(code: &str) -> Option<&'static str> {
    Some(match code {
        "no_loan" => "opac-renew-no-loan",
        "on_site" => "opac-renew-on-site",
        "too_many" => "opac-renew-too-many",
        "overdue" => "opac-renew-overdue",
        "owing" => "opac-renew-owing",
        "on_hold" => "opac-renew-on-hold",
        "too_soon" => "opac-renew-too-soon",
        "no_due_date" => "opac-renew-no-due-date",
        "restricted" => "opac-renew-restricted",
        _ => return None,
    })
}

/// Fields of a loan's renewal button.
#[derive(Deserialize)]
pub(crate) struct RenewFields {
    csrf: String,
    barcode: String,
}

/// `POST /conta/renovar`: the patron renews one of their loans (spec, items 17 to 20).
pub(crate) async fn renew(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<RenewFields>,
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
    let patron = session.identity.id;
    let barcode = form.barcode.trim().to_owned();
    let request = RenewRequest {
        barcode: barcode.clone(),
        override_limit: false,
        at: now(&state),
        by_patron: Some(patron),
    };
    let done = tokio::task::spawn_blocking(move || desk.renew_own(patron, request)).await;
    match done {
        Ok(Ok(_)) => {
            // A barcode that would need escaping is left out; the notice is then plainer.
            let plain = barcode
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte));
            let shown = if plain { barcode.as_str() } else { "" };
            Redirect::to(&format!("/conta?renovado={shown}#loans-title")).into_response()
        }
        Ok(Err(CirculationDeskError::Refused(CirculationRefusal::Renew(refusal)))) => Redirect::to(
            &format!("/conta?recusa={}#loans-title", refusal_code(&refusal)),
        )
        .into_response(),
        Ok(Err(_)) | Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}
