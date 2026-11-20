// SPDX-License-Identifier: GPL-3.0-or-later
//! The checkout slip printed for a patron: the loans out and the overdue ones, or only
//! today's (docs/specs/circulacao/avisos.md, items 38 to 40).

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use elzevir_app::loans::PatronLoan;
use elzevir_app::patrons::PatronCommand;
use elzevir_domain::patron::PatronId;
use elzevir_web_templates::{I18n, SlipLine, SlipSection, StaffCheckoutSlip};
use jiff::Zoned;
use serde::Deserialize;

use crate::circulation::{due_text, now, open, unavailable};
use crate::patrons::{failed, full_name, shown};
use crate::{AppState, render};

/// Query string of the slip.
#[derive(Deserialize)]
pub(crate) struct SlipParams {
    /// Present for the quick slip.
    rapido: Option<String>,
}

/// The loans of each part of a slip: due later and overdue, or (quick) lent or renewed
/// today.
///
/// Koha: `C4::Members::IssueSlip` (`ISSUESLIP`, and `ISSUEQSLIP` for the quick slip).
fn parts<'a>(
    loans: &'a [PatronLoan],
    now: &Zoned,
    quick: bool,
) -> Vec<(&'static str, Vec<&'a PatronLoan>)> {
    if quick {
        let today = now.date();
        let on_today =
            |moment: &Zoned| moment.with_time_zone(now.time_zone().clone()).date() == today;
        let lent = loans
            .iter()
            .filter(|loan| on_today(&loan.issued) || loan.renewed.as_ref().is_some_and(on_today))
            .collect();
        return vec![("slip-today", lent)];
    }
    let (overdue, out): (Vec<_>, Vec<_>) = loans
        .iter()
        .partition(|loan| loan.due.timestamp() <= now.timestamp());
    vec![("slip-out", out), ("slip-overdue", overdue)]
}

/// `GET /equipe/leitores/{id}/recibo`: the slip, logged as an opening of the record.
pub(crate) async fn slip(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Query(params): Query<SlipParams>,
) -> Response {
    let (session, patrons, loans) = match open(&state, &headers) {
        Ok(opened) => opened,
        Err(response) => return *response,
    };
    let actor = session.identity.id;
    let id = PatronId(id);
    let read = tokio::task::spawn_blocking(move || {
        // The opening is logged before the data leaves the server.
        patrons.apply(actor, PatronCommand::View(id))?;
        let Some(patron) = patrons.patron(id)? else {
            return Ok(None);
        };
        let current = loans.loans(id).map_err(unavailable)?;
        Ok(Some((patron, current)))
    })
    .await;
    let (patron, current) = match read {
        Ok(Ok(Some(read))) => read,
        Ok(Ok(None)) => return StatusCode::NOT_FOUND.into_response(),
        Ok(Err(error)) => return failed(&error),
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    let t = &state.inner.i18n;
    let moment = now(&state);
    let quick = params.rapido.is_some();
    render(&StaffCheckoutSlip {
        t,
        title_key: if quick {
            "slip-quick-title"
        } else {
            "slip-title"
        },
        name: full_name(&patron.fields.surname, patron.fields.first_name.as_deref()),
        cardnumber: patron.fields.cardnumber.clone(),
        printed: format!("{} {}", shown(t, moment.date()), moment.strftime("%H:%M")),
        record: format!("/equipe/leitores/{}", id.0),
        sections: parts(&current, &moment, quick)
            .into_iter()
            .map(|(heading_key, loans)| section(t, heading_key, &loans))
            .collect(),
    })
}

fn section(t: &I18n, heading_key: &'static str, loans: &[&PatronLoan]) -> SlipSection {
    SlipSection {
        heading_key,
        lines: loans
            .iter()
            .map(|loan| SlipLine {
                title: loan.title.clone(),
                barcode: loan.barcode.clone(),
                due: due_text(t, &loan.due),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loan(barcode: &str, issued: &str, renewed: Option<&str>, due: &str) -> PatronLoan {
        let at = |text: &str| -> Zoned { format!("{text}[America/Sao_Paulo]").parse().unwrap() };
        PatronLoan {
            barcode: barcode.to_owned(),
            title: "Dom Casmurro".to_owned(),
            due: at(due),
            issued: at(issued),
            renewed: renewed.map(at),
            renewals: u32::from(renewed.is_some()),
            renewals_left: 0,
            on_site: false,
        }
    }

    fn barcodes<'a>(
        parts: &[(&'static str, Vec<&'a PatronLoan>)],
    ) -> Vec<(&'static str, Vec<&'a str>)> {
        parts
            .iter()
            .map(|(key, loans)| {
                (
                    *key,
                    loans.iter().map(|loan| loan.barcode.as_str()).collect(),
                )
            })
            .collect()
    }

    #[test]
    fn the_slip_splits_loans_out_from_overdue_ones_and_the_quick_slip_keeps_todays() {
        let loans = [
            loan(
                "1",
                "2026-11-01T10:00:00-03:00",
                None,
                "2026-11-15T23:59:00-03:00",
            ),
            loan(
                "2",
                "2026-11-20T09:00:00-03:00",
                None,
                "2026-12-04T23:59:00-03:00",
            ),
            loan(
                "3",
                "2026-11-02T10:00:00-03:00",
                Some("2026-11-20T08:00:00-03:00"),
                "2026-12-04T23:59:00-03:00",
            ),
            loan(
                "4",
                "2026-11-19T23:30:00-03:00",
                None,
                "2026-12-03T23:59:00-03:00",
            ),
        ];
        let now: Zoned = "2026-11-20T10:00:00-03:00[America/Sao_Paulo]"
            .parse()
            .unwrap();
        assert_eq!(
            barcodes(&parts(&loans, &now, false)),
            [
                ("slip-out", vec!["2", "3", "4"]),
                ("slip-overdue", vec!["1"])
            ]
        );
        assert_eq!(
            barcodes(&parts(&loans, &now, true)),
            [("slip-today", vec!["2", "3"])]
        );
    }
}
