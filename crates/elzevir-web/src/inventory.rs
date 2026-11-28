// SPDX-License-Identifier: GPL-3.0-or-later
//! The inventory screen: a reading of the shelf and its report, on the page or as CSV
//! (docs/specs/catalogo/inventario.md, items 10 to 13).

use std::sync::Arc;

use axum::Form;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use elzevir_app::catalog::{CatalogOutput, CatalogRefusal, CodeLists, Place};
use elzevir_app::editing::{Edit, EditFailed};
use elzevir_app::inventory::{
    BarcodesRead, BranchSide, InventoryArea, InventoryFilters, InventoryReport, InventoryScan,
    read_barcodes,
};
use elzevir_app::staff::Permission;
use elzevir_domain::catalog::{BranchId, CollectionId, SublocationId};
use elzevir_domain::inventory::InventoryProblem;
use elzevir_web_templates::{
    I18n, InventoryChoice, InventoryForm, InventoryRow, InventoryView, StaffInventory,
};
use jiff::civil::Date;
use serde::Deserialize;

use crate::circulation::now;
use crate::security::tokens_match;
use crate::session::ActiveSession;
use crate::staff::current_session;
use crate::{AppState, render};

/// Most characters of the barcode text: 20,000 barcodes of 64 characters and a separator.
const MAX_TEXT: usize = 20_000 * 65;

/// The fields of the form, as posted.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct InventoryFields {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    branch: String,
    holding: Option<String>,
    #[serde(default)]
    sublocation: String,
    #[serde(default)]
    collection: String,
    #[serde(default)]
    call_number_from: String,
    #[serde(default)]
    call_number_to: String,
    #[serde(default)]
    seen_on: String,
    #[serde(default)]
    not_seen_since: String,
    skip_on_loan: Option<String>,
    skip_waiting_holds: Option<String>,
    compare: Option<String>,
    out_of_order: Option<String>,
    #[serde(default)]
    barcodes: String,
    /// `csv` for the report as a file.
    #[serde(rename = "as")]
    format: Option<String>,
}

fn session_of(state: &AppState, headers: &HeaderMap) -> Result<ActiveSession, Box<Response>> {
    let Some(session) = current_session(state, headers) else {
        return Err(Box::new(Redirect::to("/equipe/login").into_response()));
    };
    if !session.identity.can(Permission::Cataloguing) {
        return Err(Box::new(StatusCode::FORBIDDEN.into_response()));
    }
    Ok(session)
}

/// `GET /equipe/acervo/inventario`: the blank form, read today.
pub(crate) async fn form(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let session = match session_of(&state, &headers) {
        Ok(session) => session,
        Err(response) => return *response,
    };
    let fields = InventoryFields {
        seen_on: now(&state).date().to_string(),
        compare: Some("1".to_owned()),
        ..InventoryFields::default()
    };
    page(&state, &session, &fields, (None, None), StatusCode::OK).await
}

/// A list's chosen id; empty is none.
fn chosen(text: &str) -> Result<Option<i64>, ()> {
    let text = text.trim();
    if text.is_empty() {
        Ok(None)
    } else {
        text.parse().map(Some).map_err(|_| ())
    }
}

fn day(text: &str) -> Result<Option<Date>, ()> {
    let text = text.trim();
    if text.is_empty() {
        Ok(None)
    } else {
        text.parse().map(Some).map_err(|_| ())
    }
}

fn text(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.chars().take(64).collect())
}

/// The scan the form asks for, or the message key of why not.
fn scan_of(
    fields: &InventoryFields,
    at: jiff::Zoned,
) -> Result<(InventoryScan, BarcodesRead), &'static str> {
    let bad = "inventory-error-date";
    let seen_on = day(&fields.seen_on).map_err(|()| bad)?.ok_or(bad)?;
    let not_seen_since = day(&fields.not_seen_since).map_err(|()| bad)?;
    let ids = (
        chosen(&fields.branch),
        chosen(&fields.sublocation),
        chosen(&fields.collection),
    );
    let (Ok(branch), Ok(sublocation), Ok(collection)) = ids else {
        return Err("inventory-error-no-barcodes");
    };
    let (barcodes, read) = read_barcodes(&fields.barcodes);
    Ok((
        InventoryScan {
            barcodes,
            seen_on,
            at,
            area: InventoryArea {
                branch: branch.map(BranchId),
                side: if fields.holding.is_some() {
                    BranchSide::Holding
                } else {
                    BranchSide::Home
                },
                sublocation: sublocation.map(SublocationId),
                collection: collection.map(CollectionId),
                call_number_from: text(&fields.call_number_from),
                call_number_to: text(&fields.call_number_to),
            },
            filters: InventoryFilters {
                not_seen_since,
                skip_on_loan: fields.skip_on_loan.is_some(),
                skip_waiting_holds: fields.skip_waiting_holds.is_some(),
            },
            compare: fields.compare.is_some(),
            out_of_order: fields.out_of_order.is_some(),
        },
        read,
    ))
}

/// `POST /equipe/acervo/inventario`: the reading, and its report on the page or as CSV.
pub(crate) async fn read(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(fields): Form<InventoryFields>,
) -> Response {
    let session = match session_of(&state, &headers) {
        Ok(session) => session,
        Err(response) => return *response,
    };
    if !tokens_match(&session.csrf, &fields.csrf) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if fields.barcodes.len() > MAX_TEXT {
        let refused = Some("inventory-error-too-many");
        return page(
            &state,
            &session,
            &fields,
            (refused, None),
            StatusCode::UNPROCESSABLE_ENTITY,
        )
        .await;
    }
    let (scan, left_out) = match scan_of(&fields, now(&state)) {
        Ok(scan) => scan,
        Err(key) => {
            return page(
                &state,
                &session,
                &fields,
                (Some(key), None),
                StatusCode::UNPROCESSABLE_ENTITY,
            )
            .await;
        }
    };
    let editor = Arc::clone(&state.inner.staff_ports.editor);
    let actor = session.identity.id;
    let outcome =
        tokio::task::spawn_blocking(move || editor.apply(actor, Edit::Inventory(Box::new(scan))))
            .await;
    let report = match outcome {
        Ok(Ok(CatalogOutput::Inventory(report))) => report,
        Ok(Ok(_)) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, "unexpected answer").into_response();
        }
        Ok(Err(EditFailed::Refused(refusal))) => {
            let key = match refusal {
                CatalogRefusal::TooManyItems { .. } => "inventory-error-too-many",
                _ => "inventory-error-no-barcodes",
            };
            return page(
                &state,
                &session,
                &fields,
                (Some(key), None),
                StatusCode::UNPROCESSABLE_ENTITY,
            )
            .await;
        }
        Ok(Err(EditFailed::Busy | EditFailed::Unavailable(_))) | Err(_) => {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };
    let t = &state.inner.i18n;
    if fields.format.as_deref() == Some("csv") {
        return csv(t, &report);
    }
    let view = view(t, &report, left_out);
    page(
        &state,
        &session,
        &fields,
        (None, Some(view)),
        StatusCode::OK,
    )
    .await
}

fn problems(t: &I18n, problems: &[InventoryProblem]) -> String {
    problems
        .iter()
        .map(|problem| {
            t.get(match problem {
                InventoryProblem::UnknownBarcode => "inventory-problem-unknown-barcode",
                InventoryProblem::Withdrawn => "inventory-problem-withdrawn",
                InventoryProblem::CheckedOut => "inventory-problem-checked-out",
                InventoryProblem::WrongPlace => "inventory-problem-wrong-place",
                InventoryProblem::WasLost => "inventory-problem-was-lost",
                InventoryProblem::OutOfOrder => "inventory-problem-out-of-order",
                InventoryProblem::NotScanned => "inventory-problem-not-scanned",
                InventoryProblem::NoBarcode => "inventory-problem-no-barcode",
            })
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn rows(t: &I18n, report: &InventoryReport) -> Vec<InventoryRow> {
    report
        .lines
        .iter()
        .map(|line| InventoryRow {
            barcode: line.barcode.clone().unwrap_or_default(),
            title: line.title.clone().unwrap_or_default(),
            call_number: line.call_number.clone().unwrap_or_default(),
            problems: problems(t, &line.problems),
        })
        .collect()
}

fn count(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

fn view(t: &I18n, report: &InventoryReport, left_out: BarcodesRead) -> InventoryView {
    let skipped = left_out.repeated + left_out.too_long + left_out.unprintable;
    InventoryView {
        summary: t.get_numbers(
            "inventory-summary",
            &[
                ("read", count(report.read)),
                ("marked", count(report.marked)),
                ("expected", count(report.expected)),
            ],
        ),
        left_out: if skipped == 0 {
            String::new()
        } else {
            t.get_numbers(
                "inventory-left-out",
                &[
                    ("repeated", count(left_out.repeated)),
                    ("too_long", count(left_out.too_long)),
                    ("unprintable", count(left_out.unprintable)),
                ],
            )
        },
        lines: rows(t, report),
    }
}

/// One CSV cell: quoted when it holds a separator, a quote or a line break, and with a
/// leading `'` when a spreadsheet would read it as a formula.
pub(crate) fn cell(value: &str) -> String {
    let value = if value.starts_with(['=', '+', '-', '@']) {
        format!("'{value}")
    } else {
        value.to_owned()
    };
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value
    }
}

/// Item 12: the report as CSV, with the header in the page's language.
fn csv(t: &I18n, report: &InventoryReport) -> Response {
    let header_row = [
        "inventory-barcode",
        "inventory-item-title",
        "inventory-call-number",
        "inventory-problems",
    ]
    .map(|key| cell(&t.get(key)))
    .join(",");
    let mut body = format!("{header_row}\r\n");
    for row in rows(t, report) {
        let line = [row.barcode, row.title, row.call_number, row.problems]
            .iter()
            .map(|value| cell(value))
            .collect::<Vec<_>>()
            .join(",");
        body.push_str(&line);
        body.push_str("\r\n");
    }
    (
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"inventario.csv\"",
            ),
            (header::CACHE_CONTROL, "no-store"),
        ],
        body,
    )
        .into_response()
}

fn choices<Id: Copy>(
    places: &[Place<Id>],
    codes: &CodeLists,
    key: fn(Id) -> i64,
    selected: &str,
) -> Vec<InventoryChoice> {
    places
        .iter()
        .map(|place| {
            let value = key(place.id).to_string();
            InventoryChoice {
                selected: value == selected.trim(),
                name: format!("{} · {}", codes.branch(place.branch), place.name),
                value,
            }
        })
        .collect()
}

fn form_of(fields: &InventoryFields, codes: &CodeLists) -> InventoryForm {
    InventoryForm {
        branches: codes
            .branches
            .iter()
            .map(|branch| {
                let value = branch.id.0.to_string();
                InventoryChoice {
                    selected: value == fields.branch.trim(),
                    name: branch.name.clone(),
                    value,
                }
            })
            .collect(),
        holding: fields.holding.is_some(),
        sublocations: choices(&codes.locations, codes, |id| id.0, &fields.sublocation),
        collections: choices(&codes.collections, codes, |id| id.0, &fields.collection),
        call_number_from: fields.call_number_from.clone(),
        call_number_to: fields.call_number_to.clone(),
        seen_on: fields.seen_on.clone(),
        not_seen_since: fields.not_seen_since.clone(),
        skip_on_loan: fields.skip_on_loan.is_some(),
        skip_waiting_holds: fields.skip_waiting_holds.is_some(),
        compare: fields.compare.is_some(),
        out_of_order: fields.out_of_order.is_some(),
        barcodes: fields.barcodes.clone(),
    }
}

async fn page(
    state: &AppState,
    session: &ActiveSession,
    fields: &InventoryFields,
    (refused, report): (Option<&'static str>, Option<InventoryView>),
    status: StatusCode,
) -> Response {
    let catalog = Arc::clone(&state.inner.opac.catalog);
    let Ok(Ok(codes)) = tokio::task::spawn_blocking(move || catalog.codes()).await else {
        return (StatusCode::SERVICE_UNAVAILABLE, "catalog unavailable").into_response();
    };
    let mut form = form_of(fields, &codes);
    // After a reading, the box is ready for the next shelf.
    if report.is_some() {
        form.barcodes = String::new();
    }
    let mut response = render(&StaffInventory {
        t: &state.inner.i18n,
        csrf: &session.csrf,
        form,
        refused,
        report,
    });
    *response.status_mut() = status;
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_cells_are_quoted_and_never_formulas() {
        assert_eq!(cell("Dom Casmurro"), "Dom Casmurro");
        assert_eq!(cell("Assis, Machado"), "\"Assis, Machado\"");
        assert_eq!(cell("O \"alienista\""), "\"O \"\"alienista\"\"\"");
        assert_eq!(cell("=HYPERLINK(1)"), "'=HYPERLINK(1)");
    }
}
