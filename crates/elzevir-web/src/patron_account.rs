// SPDX-License-Identifier: GPL-3.0-or-later
//! The patron's account on their record: the lines, and paying or writing off
//! (docs/specs/circulacao/conta.md).

use std::sync::Arc;

use axum::Form;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use elzevir_app::accounts::{
    AccountLine, AccountLineId, LineKind, MAX_LINES_PER_PAYMENT, PaymentRequest,
};
use elzevir_app::circulation_rules::{CirculationCommand, CirculationDeskError, CirculationOutput};
use elzevir_domain::account::PaymentKind;
use elzevir_domain::catalog::{BranchId, Money};
use elzevir_domain::patron::PatronId;
use elzevir_web_templates::{AccountRow, I18n};

use crate::AppState;
use crate::circulation::{Outcome, money, now, ok, open, page, refusal_notices, refused};
use crate::patrons::shown;
use crate::security::tokens_match;

/// Most characters of an amount typed at the desk.
const MAX_AMOUNT_LENGTH: usize = 12;

/// The message that names a type of line: `account-` and Koha's code, in lowercase with
/// hyphens (`RENT_DAILY` is `account-rent-daily`).
pub(crate) fn label_key(kind: LineKind) -> String {
    let code = match kind {
        LineKind::Debit(debit) => debit.code(),
        LineKind::Credit(credit) => credit.code(),
    };
    format!("account-{}", code.to_ascii_lowercase().replace('_', "-"))
}

/// The lines of the account as shown.
pub(crate) fn account_rows(t: &I18n, lines: &[AccountLine]) -> Vec<AccountRow> {
    lines
        .iter()
        .map(|line| {
            let what = t.get(&label_key(line.kind));
            AccountRow {
                id: line.id.0,
                day: shown(t, line.day),
                description: match &line.barcode {
                    Some(barcode) => format!("{what} · {barcode}"),
                    None => what,
                },
                amount: money(t, line.amount),
                outstanding: money(t, line.outstanding),
                payable: matches!(line.kind, LineKind::Debit(_)) && line.outstanding.0 > 0,
            }
        })
        .collect()
}

/// An amount typed at the desk (`2`, `2,5`, `2.50`), in cents; `None` when it is not one.
pub(crate) fn typed_amount(text: &str) -> Option<Money> {
    let text = text.trim();
    if text.is_empty() || text.len() > MAX_AMOUNT_LENGTH {
        return None;
    }
    let (whole, cents) = text.split_once([',', '.']).unwrap_or((text, ""));
    let digits = |part: &str| part.bytes().all(|b| b.is_ascii_digit());
    if whole.is_empty() || !digits(whole) || !digits(cents) || cents.len() > 2 {
        return None;
    }
    let whole: i64 = whole.parse().ok()?;
    let cents: i64 = match cents.len() {
        0 => 0,
        1 => cents.parse::<i64>().ok()? * 10,
        _ => cents.parse().ok()?,
    };
    whole.checked_mul(100)?.checked_add(cents).map(Money)
}

/// What the account form asked for.
struct PayForm {
    csrf: String,
    kind: Option<PaymentKind>,
    amount: String,
    desk: Option<i64>,
    lines: Vec<AccountLineId>,
}

fn read_form(fields: Vec<(String, String)>) -> Option<PayForm> {
    let mut form = PayForm {
        csrf: String::new(),
        kind: None,
        amount: String::new(),
        desk: None,
        lines: Vec::new(),
    };
    for (name, value) in fields {
        match name.as_str() {
            "csrf" => form.csrf = value,
            "op" if value == "pay" => form.kind = Some(PaymentKind::Payment),
            "op" if value == "writeoff" => form.kind = Some(PaymentKind::Writeoff),
            "amount" => form.amount = value,
            "desk" => form.desk = value.parse().ok(),
            "line" if form.lines.len() < MAX_LINES_PER_PAYMENT => {
                form.lines.push(AccountLineId(value.parse().ok()?));
            }
            _ => return None,
        }
    }
    Some(form)
}

/// `POST /equipe/leitores/{id}/conta`: pays or writes off, and shows the record again.
pub(crate) async fn pay(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Form(fields): Form<Vec<(String, String)>>,
) -> Response {
    let (session, patrons, loans) = match open(&state, &headers) {
        Ok(opened) => opened,
        Err(response) => return *response,
    };
    let Some(form) = read_form(fields) else {
        return (StatusCode::BAD_REQUEST, "unknown field").into_response();
    };
    if !tokens_match(&session.csrf, &form.csrf) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let (Some(kind), Some(desk)) = (form.kind, form.desk) else {
        return (StatusCode::BAD_REQUEST, "unknown action").into_response();
    };
    let t = &state.inner.i18n;
    let patron = PatronId(id);
    let desk = BranchId(desk);
    let mut outcome = Outcome {
        desk: Some(desk),
        ..Outcome::default()
    };
    let Some(amount) = typed_amount(&form.amount) else {
        outcome.notices.push(refused(t.get("account-error-amount")));
        return page(
            &state,
            &session,
            &headers,
            (patrons, loans),
            patron,
            outcome,
        )
        .await;
    };
    let command = CirculationCommand::Pay(PaymentRequest {
        patron,
        kind,
        amount,
        lines: form.lines,
        desk,
        at: now(&state),
    });
    let actor = session.identity.id;
    let writer = Arc::clone(&loans);
    match tokio::task::spawn_blocking(move || writer.circulate(actor, command)).await {
        Ok(Ok(CirculationOutput::Paid(paid))) => {
            let key = match kind {
                PaymentKind::Payment => "account-paid",
                PaymentKind::Writeoff => "account-written-off",
            };
            outcome.notices.push(ok(t.get_texts(
                key,
                &[
                    ("amount", &money(t, amount)),
                    ("owed", &money(t, paid.owed)),
                ],
            )));
        }
        Ok(Ok(_)) => {}
        Ok(Err(CirculationDeskError::Refused(refusal))) => {
            outcome.notices = refusal_notices(t, &refusal);
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

#[cfg(test)]
mod tests {
    use super::*;
    use elzevir_domain::account::{CreditType, DebitType};

    #[test]
    fn every_type_of_line_has_a_name_in_both_languages() {
        let kinds = DebitType::ALL
            .into_iter()
            .map(LineKind::Debit)
            .chain(CreditType::ALL.into_iter().map(LineKind::Credit));
        for kind in kinds {
            for t in [I18n::english().unwrap(), I18n::portuguese().unwrap()] {
                assert!(t.has(&label_key(kind)), "{kind:?}");
            }
        }
        assert_eq!(
            label_key(LineKind::Debit(DebitType::RentDaily)),
            "account-rent-daily"
        );
    }

    #[test]
    fn amounts_are_typed_with_a_comma_or_a_point() {
        assert_eq!(typed_amount("2"), Some(Money(200)));
        assert_eq!(typed_amount(" 2,5 "), Some(Money(250)));
        assert_eq!(typed_amount("2.50"), Some(Money(250)));
        assert_eq!(typed_amount("0,05"), Some(Money(5)));
        for wrong in [
            "",
            "-1",
            "2,505",
            "1.000,00",
            "abc",
            ",50",
            "99999999999999",
        ] {
            assert_eq!(typed_amount(wrong), None, "{wrong}");
        }
    }
}
