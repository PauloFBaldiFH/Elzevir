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
use elzevir_app::holds::{
    HoldAside, HoldCancellation, HoldId, HoldLevel, HoldRequest, HoldSuspensionRequest, PatronHold,
    Suspension, SuspensionChange,
};
use elzevir_domain::catalog::BranchId;
use elzevir_domain::hold::Found;
use elzevir_domain::patron::PatronId;
use elzevir_domain::suspension::RestrictionKind;
use elzevir_web_templates::{HoldRow, HoldSuspensionAction, I18n};
use jiff::civil::Date;
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
            status: status(t, hold),
            suspension: match (hold.found, hold.suspended) {
                (Some(_), _) => HoldSuspensionAction::None,
                (None, Some(_)) => HoldSuspensionAction::Resume,
                (None, None) => HoldSuspensionAction::Suspend,
            },
        })
        .collect()
}

/// Where the hold stands, as shown.
fn status(t: &I18n, hold: &PatronHold) -> String {
    let position = hold.priority.to_string();
    match (hold.found, hold.expires_on, hold.suspended) {
        (Some(Found::Waiting), Some(until), _) => {
            t.get_with("holds-waiting", "date", &shown(t, until))
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
    Suspension(HoldId, SuspensionChange),
}

/// Reads the form: the token, the desk, and what to do; `None` when a field is unknown or
/// missing.
fn read_form(fields: Vec<(String, String)>) -> Option<(String, Option<BranchId>, Asked)> {
    let (mut csrf, mut desk, mut op) = (String::new(), None, None);
    let (mut barcode, mut level, mut pickup) = (String::new(), None, None);
    let mut until: Vec<(i64, String)> = Vec::new();
    for (name, value) in fields {
        if let Some(hold) = name.strip_prefix("until-") {
            until.push((hold.parse().ok()?, value));
            continue;
        }
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
        other => match other.split_once(':')? {
            ("cancel", hold) => Asked::Cancel(HoldId(hold.parse().ok()?)),
            ("resume", hold) => {
                Asked::Suspension(HoldId(hold.parse().ok()?), SuspensionChange::Resume)
            }
            ("suspend", hold) => {
                let hold: i64 = hold.parse().ok()?;
                // The browser sends an empty date when none was chosen.
                let day = until
                    .iter()
                    .find(|(id, _)| *id == hold)
                    .map(|(_, day)| day.trim())
                    .filter(|day| !day.is_empty());
                let suspension = match day {
                    None => Suspension::Indefinite,
                    Some(day) => Suspension::Until(day.parse::<Date>().ok()?),
                };
                Asked::Suspension(HoldId(hold), SuspensionChange::Suspend(suspension))
            }
            _ => return None,
        },
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
            by_patron: false,
        }),
        Asked::Cancel(hold) => CirculationCommand::CancelHold(HoldCancellation {
            patron,
            hold,
            at,
            by_patron: false,
        }),
        Asked::Suspension(hold, change) => CirculationCommand::SuspendHold(HoldSuspensionRequest {
            patron,
            hold,
            change,
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
        Ok(Ok(CirculationOutput::HoldSuspensionChanged)) => {
            outcome.notices.push(ok(t.get("hold-suspension-changed")));
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
        assert!(matches!(
            read_form(fields(&[
                ("until-7", ""),
                ("until-8", "2026-12-20"),
                ("op", "suspend:7")
            ])),
            Some((
                _,
                None,
                Asked::Suspension(HoldId(7), SuspensionChange::Suspend(Suspension::Indefinite))
            ))
        ));
        let until = "2026-12-20".parse().unwrap();
        assert!(matches!(
            read_form(fields(&[("until-7", ""), ("until-8", "2026-12-20"), ("op", "suspend:8")])),
            Some((_, None, Asked::Suspension(HoldId(8), SuspensionChange::Suspend(Suspension::Until(day))))) if day == until
        ));
        assert!(matches!(
            read_form(fields(&[("op", "resume:8")])),
            Some((
                _,
                None,
                Asked::Suspension(HoldId(8), SuspensionChange::Resume)
            ))
        ));
        assert!(read_form(fields(&[("until-8", "20/12/2026"), ("op", "suspend:8")])).is_none());
        assert!(read_form(fields(&[("until-x", ""), ("op", "resume:8")])).is_none());
        assert!(read_form(fields(&[("op", "pause:8")])).is_none());
        assert!(read_form(fields(&[("what", "1")])).is_none());
    }
}
