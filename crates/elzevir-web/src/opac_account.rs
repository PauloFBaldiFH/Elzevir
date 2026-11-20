// SPDX-License-Identifier: GPL-3.0-or-later
//! The patron's own account page on the OPAC: who they are, their loans, restrictions
//! and debts (docs/specs/opac/conta-do-leitor.md, items 12 to 16).

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use elzevir_app::accounts::AccountLine;
use elzevir_app::circulation_rules::CirculationDeskError;
use elzevir_app::loans::{LoanDesk, PatronLoan};
use elzevir_domain::catalog::Money;
use elzevir_domain::patron::PatronId;
use elzevir_domain::suspension::{RestrictionKind, SuspensionEnd};
use elzevir_web_templates::{I18n, LoanLine, OpacAccount};
use jiff::Zoned;
use jiff::civil::Date;

use crate::circulation::{due_text, money, now, suspension_text};
use crate::patron_account::account_rows;
use crate::patron_login::{current_patron, no_store};
use crate::{AppState, render};

/// What the circulation desk knows about the patron.
struct Circulation {
    loans: Vec<PatronLoan>,
    restrictions: Vec<(RestrictionKind, SuspensionEnd)>,
    owed: Money,
    lines: Vec<AccountLine>,
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
        });
    };
    Ok(Circulation {
        loans: desk.loans(patron)?,
        restrictions: desk.restrictions(patron, today)?,
        owed: desk.owed(patron)?,
        lines: desk.account(patron)?,
    })
}

/// `GET /conta`: the patron's own account (spec, items 12 to 16).
pub(crate) async fn account(State(state): State<AppState>, headers: HeaderMap) -> Response {
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
        loans: circulation
            .loans
            .iter()
            .map(|loan| loan_line(t, loan, &moment))
            .collect(),
        account: account_rows(t, &circulation.lines),
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

fn loan_line(t: &I18n, loan: &PatronLoan, now: &Zoned) -> LoanLine {
    LoanLine {
        barcode: loan.barcode.clone(),
        title: loan.title.clone(),
        due: due_text(t, &loan.due),
        overdue: loan.due.timestamp() < now.timestamp(),
        renewals: (loan.renewals > 0)
            .then(|| t.get_numbers("loans-renewals", &[("count", u64::from(loan.renewals))])),
        new: false,
    }
}
