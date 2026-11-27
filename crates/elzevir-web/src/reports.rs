// SPDX-License-Identifier: GPL-3.0-or-later
//! The fixed reports, on the page or as CSV (docs/specs/operacao/relatorios.md, items 10
//! to 13).

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use elzevir_app::reports::{
    CirculationLine, CollectionLine, PatronsLine, Period, ReportError, ReportReader,
};
use elzevir_web_templates::{I18n, ReportTable, StaffReports};
use jiff::ToSpan;
use jiff::civil::Date;
use serde::Deserialize;

use crate::icons::content_badge;
use crate::inventory::cell;
use crate::patrons::today;
use crate::staff::current_session;
use crate::{AppState, render};

/// Query string of `/equipe/relatorios`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct ReportParams {
    relatorio: String,
    de: String,
    ate: String,
    desde: String,
    formato: String,
}

/// What was asked, once read.
enum Asked {
    Circulation(Period),
    Collection,
    Patrons(Date),
}

/// `GET /equipe/relatorios`: the forms, and the report asked for, on the page or as CSV
/// (items 10 to 13).
pub(crate) async fn page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<ReportParams>,
) -> Response {
    if current_session(&state, &headers).is_none() {
        return Redirect::to("/equipe/login").into_response();
    }
    let Some(reader) = state.reports.clone() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let today = today(&state);
    let from = or_default(&params.de, today.first_of_month());
    let to = or_default(&params.ate, today);
    let since = or_default(&params.desde, today.saturating_sub(1.year()));
    let t = &state.inner.i18n;
    let asked = match asked(&params) {
        Ok(asked) => asked,
        Err(refused) => {
            let page = StaffReports {
                t,
                report: &params.relatorio,
                from: &from,
                to: &to,
                since: &since,
                refused,
                table: None,
                csv_url: None,
            };
            return (StatusCode::UNPROCESSABLE_ENTITY, render(&page)).into_response();
        }
    };
    let table = match asked {
        None => None,
        Some(asked) => match read(reader, asked).await {
            Ok(lines) => Some(table(t, &lines)),
            Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
        },
    };
    if params.formato == "csv" {
        return match table {
            Some(table) => csv(&params.relatorio, &table),
            None => StatusCode::BAD_REQUEST.into_response(),
        };
    }
    let csv_url = table.as_ref().map(|_| csv_url(&params));
    render(&StaffReports {
        t,
        report: &params.relatorio,
        from: &from,
        to: &to,
        since: &since,
        refused: None,
        table,
        csv_url,
    })
}

/// The typed value, else the default day.
fn or_default(typed: &str, day: Date) -> String {
    if typed.is_empty() {
        day.to_string()
    } else {
        typed.to_owned()
    }
}

/// The report asked for, `None` for none, or the key of why it is refused.
fn asked(params: &ReportParams) -> Result<Option<Asked>, Option<&'static str>> {
    let day = |text: &str| text.parse::<Date>().map_err(|_| Some("reports-bad-day"));
    Ok(Some(match params.relatorio.as_str() {
        "" => return Ok(None),
        "circulacao" => Asked::Circulation(
            Period::new(day(&params.de)?, day(&params.ate)?).ok_or(Some("reports-bad-period"))?,
        ),
        "acervo" => Asked::Collection,
        "leitores" => Asked::Patrons(day(&params.desde)?),
        _ => return Err(Some("reports-unknown")),
    }))
}

/// The lines of one report.
enum Lines {
    Circulation(Vec<CirculationLine>),
    Collection(Vec<CollectionLine>),
    Patrons(Vec<PatronsLine>),
}

async fn read(reader: Arc<dyn ReportReader>, asked: Asked) -> Result<Lines, ReportError> {
    tokio::task::spawn_blocking(move || match asked {
        Asked::Circulation(period) => reader.circulation(period).map(Lines::Circulation),
        Asked::Collection => reader.collection().map(Lines::Collection),
        Asked::Patrons(since) => reader.patrons(since).map(Lines::Patrons),
    })
    .await
    .map_err(|error| ReportError(error.to_string()))?
}

/// Item 12: each report as headers and rows, with a last row of totals.
fn table(t: &I18n, lines: &Lines) -> ReportTable {
    let headers = |keys: &[&str]| keys.iter().map(|key| t.get(key)).collect::<Vec<_>>();
    let no_category = || t.get("reports-no-category");
    match lines {
        Lines::Circulation(lines) => {
            let mut rows: Vec<Vec<String>> = lines
                .iter()
                .map(|line| {
                    vec![
                        line.branch.clone(),
                        line.category.clone().unwrap_or_else(no_category),
                        line.checkouts.to_string(),
                        line.returns.to_string(),
                    ]
                })
                .collect();
            let sum = |pick: fn(&CirculationLine) -> u64| lines.iter().map(pick).sum::<u64>();
            rows.push(vec![
                t.get("reports-total"),
                String::new(),
                sum(|line| line.checkouts).to_string(),
                sum(|line| line.returns).to_string(),
            ]);
            ReportTable {
                headers: headers(&[
                    "reports-branch",
                    "reports-category",
                    "reports-checkouts",
                    "reports-returns",
                ]),
                rows,
            }
        }
        Lines::Collection(lines) => {
            let mut rows: Vec<Vec<String>> = lines
                .iter()
                .map(|line| {
                    vec![
                        line.class.map_or_else(
                            || t.get("reports-no-class"),
                            |class| format!("{class:03}–{:03}", class + 99),
                        ),
                        line.content.map_or_else(
                            || t.get("reports-no-content"),
                            |kind| t.get(content_badge(kind).label_key),
                        ),
                        line.items.to_string(),
                    ]
                })
                .collect();
            rows.push(vec![
                t.get("reports-total"),
                String::new(),
                lines.iter().map(|line| line.items).sum::<u64>().to_string(),
            ]);
            ReportTable {
                headers: headers(&["reports-class", "reports-content", "reports-items"]),
                rows,
            }
        }
        Lines::Patrons(lines) => {
            let mut rows: Vec<Vec<String>> = lines
                .iter()
                .map(|line| {
                    vec![
                        line.branch.clone(),
                        line.category.clone(),
                        line.patrons.to_string(),
                        line.active.to_string(),
                    ]
                })
                .collect();
            let sum = |pick: fn(&PatronsLine) -> u64| lines.iter().map(pick).sum::<u64>();
            rows.push(vec![
                t.get("reports-total"),
                String::new(),
                sum(|line| line.patrons).to_string(),
                sum(|line| line.active).to_string(),
            ]);
            ReportTable {
                headers: headers(&[
                    "reports-branch",
                    "reports-category",
                    "reports-patron-count",
                    "reports-active",
                ]),
                rows,
            }
        }
    }
}

/// The link to the same report as CSV.
fn csv_url(params: &ReportParams) -> String {
    let encode = |value: &str| {
        if value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            value.to_owned()
        } else {
            String::new()
        }
    };
    format!(
        "/equipe/relatorios?relatorio={}&de={}&ate={}&desde={}&formato=csv",
        encode(&params.relatorio),
        encode(&params.de),
        encode(&params.ate),
        encode(&params.desde),
    )
}

/// Item 13: the table as CSV, as the inventory writes it.
fn csv(report: &str, table: &ReportTable) -> Response {
    let line = |values: &[String]| {
        values
            .iter()
            .map(|value| cell(value))
            .collect::<Vec<_>>()
            .join(",")
    };
    let mut body = format!("{}\r\n", line(&table.headers));
    for row in &table.rows {
        body.push_str(&line(row));
        body.push_str("\r\n");
    }
    let name = match report {
        "circulacao" => "circulacao",
        "acervo" => "acervo",
        _ => "leitores",
    };
    (
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8".to_owned()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"relatorio-{name}.csv\""),
            ),
            (header::CACHE_CONTROL, "no-store".to_owned()),
        ],
        body,
    )
        .into_response()
}
