// SPDX-License-Identifier: GPL-3.0-or-later
//! Lending from an Item's pill in the catalog, and the arrow across the wall (interface
//! §18.2). The catalog side never carries patron data: the pill shows the due date, and
//! the arrow asks the server who has the Item.

use std::sync::Arc;

use axum::Form;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use elzevir_app::circulation_rules::{
    CirculationCommand, CirculationDeskError, CirculationRefusal,
};
use elzevir_app::listing::ItemPill;
use elzevir_app::loans::CheckoutRequest;
use elzevir_app::patrons::PatronQuery;
use elzevir_domain::catalog::{InstanceId, ItemId};
use elzevir_domain::patron::PatronId;
use elzevir_web_templates::{PillInstance, StaffItemPill};
use serde::Deserialize;

use crate::catalog::pill_line;
use crate::circulation::{now, open, refusal_notices};
use crate::icons::item_icon;
use crate::security::tokens_match;
use crate::{AppState, render};

/// A checkout asked for in an Item's pill.
#[derive(Deserialize)]
pub(crate) struct LendForm {
    csrf: String,
    instance: i64,
    item: i64,
    cardnumber: String,
}

/// What a checkout from the pill did.
enum Lent {
    Done,
    /// Refused before the rules, with the message key.
    Refused(&'static str),
    /// Refused by the circulation rules.
    Rules(CirculationRefusal),
}

/// The patron with exactly this card number; never a match by name or CPF.
fn card_holder(
    patrons: &dyn elzevir_app::patrons::PatronDesk,
    cardnumber: &str,
) -> Option<PatronId> {
    let query = PatronQuery::parse(cardnumber)?;
    let rows = patrons.search(&query).ok()?;
    rows.into_iter()
        .find(|row| row.cardnumber == query.cardnumber)
        .map(|row| row.id)
}

/// `POST /equipe/acervo/emprestar`: lends the Item to the patron of the card, at the branch
/// the Item is at, and answers with the inside of its pill: yellow with the due date, or
/// still green with the box open and the reasons.
pub(crate) async fn lend(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<LendForm>,
) -> Response {
    let (session, patrons, loans) = match open(&state, &headers) {
        Ok(opened) => opened,
        Err(response) => return *response,
    };
    if !tokens_match(&session.csrf, &form.csrf) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let t = &state.inner.i18n;
    let catalog = Arc::clone(&state.inner.opac.catalog);
    let (instance, item) = (InstanceId(form.instance), ItemId(form.item));
    let actor = session.identity.id;
    let at = now(&state);
    let done = tokio::task::spawn_blocking(move || {
        let lent = match card_holder(patrons.as_ref(), &form.cardnumber) {
            None => Lent::Refused("pill-unknown-patron"),
            Some(patron) => {
                let barcode = catalog
                    .items(instance)
                    .ok()
                    .and_then(|items| items.into_iter().find(|view| view.id == item))
                    .and_then(|view| view.barcode);
                match (barcode, loans.holding_branch(item)?) {
                    (Some(barcode), Some(desk)) => {
                        let command = CirculationCommand::Checkout(CheckoutRequest {
                            patron,
                            barcode,
                            desk,
                            on_site: false,
                            confirmed: false,
                            at,
                        });
                        match loans.circulate(actor, command) {
                            Ok(_) => Lent::Done,
                            Err(CirculationDeskError::Refused(refusal)) => Lent::Rules(refusal),
                            Err(error) => return Err(error),
                        }
                    }
                    _ => Lent::Refused("checkout-error-unknown-barcode"),
                }
            }
        };
        // The pill is read again after the checkout, so a lent Item comes back yellow.
        let view = catalog
            .instance(instance)
            .ok()
            .flatten()
            .zip(catalog.items(instance).ok())
            .and_then(|(view, items)| {
                let pill = items.into_iter().find(|candidate| candidate.id == item)?;
                Some((item_icon(view.material), ItemPill::from(pill)))
            });
        Ok((lent, view, catalog.codes().ok()))
    })
    .await;
    let (lent, view, codes) = match done {
        Ok(Ok(read)) => read,
        Ok(Err(CirculationDeskError::Busy)) => {
            return (StatusCode::SERVICE_UNAVAILABLE, t.get("circulation-busy")).into_response();
        }
        _ => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    let (Some((icon, pill)), Some(codes)) = (view, codes) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut line = pill_line(&pill, &codes, t);
    line.refused = match lent {
        Lent::Done => Vec::new(),
        Lent::Refused(key) => vec![t.get(key)],
        Lent::Rules(refusal) => refusal_notices(t, &refusal)
            .into_iter()
            .map(|notice| notice.text)
            .collect(),
    };
    render(&StaffItemPill {
        t,
        csrf: &session.csrf,
        can_circulate: true,
        instance: PillInstance {
            id: instance.0,
            icon,
        },
        pill: line,
    })
}

/// `GET /equipe/circulacao/exemplar/{id}`: the arrow of a lent Item crosses the wall to the
/// record of who has it, which logs the opening. Only with the circulation permission.
pub(crate) async fn to_patron(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    let (_, _, loans) = match open(&state, &headers) {
        Ok(opened) => opened,
        Err(response) => return *response,
    };
    match tokio::task::spawn_blocking(move || loans.borrower(ItemId(id))).await {
        Ok(Ok(Some(patron))) => {
            Redirect::to(&format!("/equipe/leitores/{}", patron.0)).into_response()
        }
        Ok(Ok(None)) => StatusCode::NOT_FOUND.into_response(),
        _ => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}
