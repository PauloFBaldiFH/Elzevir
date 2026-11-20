// SPDX-License-Identifier: GPL-3.0-or-later
//! The patron's holds on their record: the list, placing and cancelling
//! (docs/specs/circulacao/reservas.md); and lifting a restriction
//! (docs/specs/circulacao/suspensao.md).

use std::sync::Arc;

use axum::Form;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use elzevir_app::circulation_rules::{CirculationCommand, CirculationDeskError, CirculationOutput};
use elzevir_app::holds::{HoldAside, HoldCancellation, HoldId, HoldLevel, HoldRequest, PatronHold};
use elzevir_domain::catalog::BranchId;
use elzevir_domain::hold::Found;
use elzevir_domain::patron::PatronId;
use elzevir_domain::suspension::RestrictionKind;
use elzevir_web_templates::{HoldRow, I18n};
use serde::Deserialize;

use crate::AppState;
use crate::circulation::{Outcome, checkin_page, now, ok, open, page, refusal_notices, refused};
use crate::patrons::shown;
use crate::security::tokens_match;

/// The patron's holds as shown.
pub(crate) fn hold_rows(t: &I18n, holds: &[PatronHold]) -> Vec<HoldRow> {
    holds
        .iter()
        .map(|hold| HoldRow {
            id: hold.id.0,
            title: hold.title.clone(),
            barcode: hold.barcode.clone(),
            pickup: hold.pickup.clone(),
            day: shown(t, hold.placed_on),
            status: match (hold.found, hold.expires_on) {
                (Some(Found::Waiting), Some(until)) => {
                    t.get_with("holds-waiting", "date", &shown(t, until))
                }
                (Some(_), _) => t.get("holds-in-transit"),
                (None, _) => t.get_with("holds-position", "position", &hold.priority.to_string()),
            },
        })
        .collect()
}

/// The checkin desk's button for a hold.
#[derive(Deserialize)]
pub(crate) struct AsideFields {
    csrf: String,
    hold: i64,
    barcode: String,
    desk: i64,
}

/// `POST /equipe/circulacao/devolucao/reserva`: sets the checked-in Item aside for the hold
/// it answers, and shows the checkin desk again with where the Item goes.
pub(crate) async fn set_aside(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<AsideFields>,
) -> Response {
    let (session, _, loans) = match open(&state, &headers) {
        Ok(opened) => opened,
        Err(response) => return *response,
    };
    if !tokens_match(&session.csrf, &form.csrf) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let desk = BranchId(form.desk);
    let command = CirculationCommand::SetHoldAside(HoldAside {
        hold: HoldId(form.hold),
        barcode: form.barcode,
        desk,
        at: now(&state),
    });
    let actor = session.identity.id;
    let done = tokio::task::spawn_blocking(move || loans.circulate(actor, command)).await;
    let t = &state.inner.i18n;
    let notices = match done {
        Ok(Ok(CirculationOutput::HoldSetAside(aside))) => match aside.expires_on {
            Some(until) if aside.found == Found::Waiting => vec![ok(t.get_with(
                "hold-set-aside-waiting",
                "date",
                &shown(t, until),
            ))],
            _ => {
                let catalog = Arc::clone(&state.inner.opac.catalog);
                let Ok(Ok(codes)) = tokio::task::spawn_blocking(move || catalog.codes()).await
                else {
                    return StatusCode::SERVICE_UNAVAILABLE.into_response();
                };
                let branch = codes
                    .branches
                    .into_iter()
                    .find(|branch| branch.id == aside.pickup)
                    .map(|branch| branch.name)
                    .unwrap_or_default();
                vec![refused(t.get_with(
                    "hold-set-aside-transit",
                    "branch",
                    &branch,
                ))]
            }
        },
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
        crate::circulation::CheckinView {
            notices,
            ..crate::circulation::CheckinView::default()
        },
    )
    .await
}

/// What the hold form asked for.
enum Asked {
    Place {
        barcode: String,
        level: HoldLevel,
        pickup: BranchId,
    },
    Cancel(HoldId),
}

/// Reads the form: the token, the desk, and what to do; `None` when a field is unknown or
/// missing.
fn read_form(fields: Vec<(String, String)>) -> Option<(String, Option<BranchId>, Asked)> {
    let (mut csrf, mut desk, mut op) = (String::new(), None, None);
    let (mut barcode, mut level, mut pickup) = (String::new(), None, None);
    for (name, value) in fields {
        match name.as_str() {
            "csrf" => csrf = value,
            "desk" => desk = value.parse().ok().map(BranchId),
            "barcode" => barcode = value,
            "level" if value == "item" => level = Some(HoldLevel::Item),
            "level" if value == "record" => level = Some(HoldLevel::Record),
            "pickup" => pickup = value.parse().ok().map(BranchId),
            "op" => op = Some(value),
            _ => return None,
        }
    }
    let asked = match op?.as_str() {
        "place" => Asked::Place {
            barcode,
            level: level?,
            pickup: pickup?,
        },
        other => Asked::Cancel(HoldId(other.strip_prefix("cancel:")?.parse().ok()?)),
    };
    Some((csrf, desk, asked))
}

/// `POST /equipe/leitores/{id}/reservas`: places or cancels a hold, and shows the record
/// again.
pub(crate) async fn change(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Form(fields): Form<Vec<(String, String)>>,
) -> Response {
    let (session, patrons, loans) = match open(&state, &headers) {
        Ok(opened) => opened,
        Err(response) => return *response,
    };
    let Some((csrf, desk, asked)) = read_form(fields) else {
        return (StatusCode::BAD_REQUEST, "unknown field").into_response();
    };
    if !tokens_match(&session.csrf, &csrf) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let t = &state.inner.i18n;
    let patron = PatronId(id);
    let at = now(&state);
    let command = match asked {
        Asked::Place {
            barcode,
            level,
            pickup,
        } => CirculationCommand::PlaceHold(HoldRequest {
            patron,
            barcode,
            level,
            pickup,
            at,
        }),
        Asked::Cancel(hold) => CirculationCommand::CancelHold(HoldCancellation {
            patron,
            hold,
            at,
            by_patron: false,
        }),
    };
    let mut outcome = Outcome {
        desk,
        ..Outcome::default()
    };
    let actor = session.identity.id;
    let writer = Arc::clone(&loans);
    match tokio::task::spawn_blocking(move || writer.circulate(actor, command)).await {
        Ok(Ok(CirculationOutput::HoldPlaced(placed))) => {
            outcome.notices.push(ok(
                t.get_numbers("hold-placed", &[("position", u64::from(placed.priority))])
            ));
        }
        Ok(Ok(CirculationOutput::HoldCancelled)) => {
            outcome.notices.push(ok(t.get("hold-cancelled")));
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
    page(&state, &session, (patrons, loans), patron, outcome).await
}

/// The lift button of a restriction, posted with the rest of the record's form.
#[derive(Deserialize)]
pub(crate) struct LiftFields {
    csrf: String,
    lift: String,
    desk: Option<i64>,
}

/// `POST /equipe/leitores/{id}/restricoes`: lifts one of the patron's restrictions, and
/// shows the record again.
pub(crate) async fn lift_restriction(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Form(form): Form<LiftFields>,
) -> Response {
    let (session, patrons, loans) = match open(&state, &headers) {
        Ok(opened) => opened,
        Err(response) => return *response,
    };
    if !tokens_match(&session.csrf, &form.csrf) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(kind) = RestrictionKind::from_name(&form.lift) else {
        return (StatusCode::BAD_REQUEST, "unknown restriction").into_response();
    };
    let t = &state.inner.i18n;
    let patron = PatronId(id);
    let command = CirculationCommand::LiftRestriction { patron, kind };
    let mut outcome = Outcome {
        desk: form.desk.map(BranchId),
        ..Outcome::default()
    };
    let actor = session.identity.id;
    let writer = Arc::clone(&loans);
    match tokio::task::spawn_blocking(move || writer.circulate(actor, command)).await {
        Ok(Ok(_)) => outcome.notices.push(ok(t.get("restriction-lifted"))),
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
    page(&state, &session, (patrons, loans), patron, outcome).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[test]
    fn the_form_places_or_cancels() {
        let Some((_, desk, Asked::Place { level, pickup, .. })) = read_form(fields(&[
            ("csrf", "x"),
            ("desk", "2"),
            ("barcode", "0001"),
            ("level", "record"),
            ("pickup", "1"),
            ("op", "place"),
        ])) else {
            panic!("not a hold");
        };
        assert_eq!(
            (desk, level, pickup),
            (Some(BranchId(2)), HoldLevel::Record, BranchId(1))
        );
        assert!(matches!(
            read_form(fields(&[("op", "cancel:7")])),
            Some((_, None, Asked::Cancel(HoldId(7))))
        ));
        assert!(read_form(fields(&[("op", "place"), ("barcode", "1")])).is_none());
        assert!(read_form(fields(&[("op", "cancel:x")])).is_none());
        assert!(read_form(fields(&[("what", "1")])).is_none());
    }
}
