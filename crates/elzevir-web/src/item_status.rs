// SPDX-License-Identifier: GPL-3.0-or-later
//! The status of an Item at the desk, and the names of the status numbers
//! (docs/specs/circulacao/lost-items.md, items 2 and 3).

use std::sync::Arc;

use axum::Form;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use elzevir_app::circulation_rules::{CirculationCommand, CirculationDeskError, CirculationOutput};
use elzevir_app::item_status::{
    ItemAtTheDesk, ItemStatusChange, ItemStatusChanged, ItemStatusDesk, StatusKind, StatusName,
    StatusNameChange,
};
use elzevir_app::loans::LoanDesk;
use elzevir_web_templates::{
    I18n, ItemStatusView, Notice, StaffItemStatus, StaffStatusNames, StatusForm, StatusKindNames,
};
use serde::Deserialize;

use crate::circulation::{due_text, money, now, ok, refusal_notices, refused};
use crate::security::tokens_match;
use crate::session::ActiveSession;
use crate::{AppState, render};

type Opened = (ActiveSession, Arc<dyn LoanDesk>, Arc<dyn ItemStatusDesk>);

/// The circulation permission, the desk and the status reads, or the answer to give.
fn open(state: &AppState, headers: &HeaderMap) -> Result<Opened, Box<Response>> {
    let (session, _, loans) = crate::circulation::open(state, headers)?;
    let Some(statuses) = state.item_statuses.clone() else {
        return Err(Box::new(StatusCode::NOT_FOUND.into_response()));
    };
    Ok((session, loans, statuses))
}

/// The generic name of a kind (item 2).
const fn generic_key(kind: StatusKind) -> &'static str {
    match kind {
        StatusKind::Lost => "item-status-lost",
        StatusKind::Damaged => "item-status-damaged",
        StatusKind::Withdrawn => "item-status-withdrawn",
        StatusKind::NotForLoan => "item-status-not-for-loan",
    }
}

/// The name of a number: the library's, or the generic one with the number.
fn label(t: &I18n, names: &[StatusName], kind: StatusKind, number: i64) -> String {
    if number == 0 {
        return t.get("item-status-none");
    }
    names
        .iter()
        .find(|name| name.kind == kind && name.number == number)
        .map_or_else(
            || format!("{} ({number})", t.get(generic_key(kind))),
            |name| name.label.clone(),
        )
}

fn view(t: &I18n, item: ItemAtTheDesk, names: &[StatusName]) -> ItemStatusView {
    let statuses = item
        .statuses
        .iter()
        .map(|line| {
            let mut numbers: Vec<i64> = std::iter::once(0)
                .chain(
                    names
                        .iter()
                        .filter(|name| name.kind == line.kind)
                        .map(|name| name.number),
                )
                .collect();
            // A new library names nothing yet: the generic first number is always offered.
            numbers.push(1);
            numbers.push(line.number);
            numbers.sort_unstable();
            numbers.dedup();
            let mut shown = label(t, names, line.kind, line.number);
            if let Some(since) = &line.since {
                shown = format!(
                    "{shown}, {}",
                    t.get_with("item-status-since", "date", &due_text(t, since))
                );
            }
            StatusForm {
                kind: line.kind.code(),
                name: t.get(generic_key(line.kind)),
                current: line.number,
                shown,
                options: numbers
                    .into_iter()
                    .map(|number| (number, label(t, names, line.kind, number)))
                    .collect(),
            }
        })
        .collect();
    ItemStatusView {
        barcode: item.barcode,
        title: item.title,
        call_number: item.call_number.unwrap_or_default(),
        holding: item.holding_name,
        loan: item.due.map_or_else(String::new, |due| {
            t.get_with("item-status-on-loan", "date", &due_text(t, &due))
        }),
        statuses,
    }
}

/// The page for `barcode`, with the notices of the last change.
async fn page(
    state: &AppState,
    session: &ActiveSession,
    statuses: Arc<dyn ItemStatusDesk>,
    barcode: &str,
    mut notices: Vec<Notice>,
) -> Response {
    let wanted = barcode.trim().to_owned();
    let read = tokio::task::spawn_blocking(move || {
        let item = if wanted.is_empty() {
            None
        } else {
            statuses.item_at_the_desk(&wanted)?
        };
        Ok::<_, CirculationDeskError>((item, statuses.status_names()?))
    })
    .await;
    let Ok(Ok((item, names))) = read else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let t = &state.inner.i18n;
    let missing = item.is_none() && !barcode.trim().is_empty();
    if missing {
        notices.push(refused(t.get_with(
            "item-status-not-found",
            "barcode",
            barcode.trim(),
        )));
    }
    let refusal = notices.iter().any(|notice| notice.refused);
    let mut response = render(&StaffItemStatus {
        t,
        csrf: &session.csrf,
        barcode: barcode.trim(),
        notices,
        item: item.map(|item| view(t, item, &names)),
    });
    if response.status() == StatusCode::OK {
        if missing {
            *response.status_mut() = StatusCode::NOT_FOUND;
        } else if refusal {
            *response.status_mut() = StatusCode::UNPROCESSABLE_ENTITY;
        }
    }
    response
}

/// The barcode asked for.
#[derive(Deserialize)]
pub(crate) struct Lookup {
    #[serde(default)]
    codigo: String,
}

/// `GET /equipe/circulacao/exemplar`.
pub(crate) async fn item_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(lookup): Query<Lookup>,
) -> Response {
    match open(&state, &headers) {
        Ok((session, _, statuses)) => {
            page(&state, &session, statuses, &lookup.codigo, Vec::new()).await
        }
        Err(response) => *response,
    }
}

/// One status changed from the page.
#[derive(Deserialize)]
pub(crate) struct StatusChangeForm {
    csrf: String,
    codigo: String,
    kind: String,
    from: i64,
    to: i64,
}

fn changed_notices(t: &I18n, done: &ItemStatusChanged) -> Vec<Notice> {
    let mut notices = vec![ok(t.get("item-status-saved"))];
    for (_, amount) in &done.charged {
        notices.push(ok(t.get_with(
            "item-status-charged",
            "amount",
            &money(t, *amount),
        )));
    }
    if done.loan_ended {
        notices.push(ok(t.get("item-status-loan-ended")));
    }
    if done.transfer_cancelled {
        notices.push(ok(t.get("item-status-transfer-cancelled")));
    }
    notices
}

/// `POST /equipe/circulacao/exemplar`.
pub(crate) async fn change_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<StatusChangeForm>,
) -> Response {
    let (session, loans, statuses) = match open(&state, &headers) {
        Ok(opened) => opened,
        Err(response) => return *response,
    };
    if !tokens_match(&session.csrf, &form.csrf) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(kind) = StatusKind::from_code(&form.kind) else {
        return StatusCode::UNPROCESSABLE_ENTITY.into_response();
    };
    let barcode = form.codigo.trim().to_owned();
    let lookup = Arc::clone(&statuses);
    let wanted = barcode.clone();
    let found = tokio::task::spawn_blocking(move || lookup.item_at_the_desk(&wanted)).await;
    let item = match found {
        Ok(Ok(Some(item))) => item.item,
        Ok(Ok(None)) => return page(&state, &session, statuses, &barcode, Vec::new()).await,
        Ok(Err(_)) | Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    let command = CirculationCommand::SetItemStatus(ItemStatusChange {
        item,
        kind,
        from: form.from,
        to: form.to,
        at: now(&state),
    });
    let actor = session.identity.id;
    let done = tokio::task::spawn_blocking(move || loans.circulate(actor, command)).await;
    let t = &state.inner.i18n;
    let notices = match done {
        Ok(Ok(CirculationOutput::ItemStatusChanged(done))) => changed_notices(t, &done),
        Ok(Ok(_)) => Vec::new(),
        Ok(Err(CirculationDeskError::Refused(refusal))) => refusal_notices(t, &refusal),
        Ok(Err(CirculationDeskError::Busy)) => vec![refused(t.get("circulation-busy"))],
        Ok(Err(CirculationDeskError::Unavailable(_))) | Err(_) => {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };
    page(&state, &session, statuses, &barcode, notices).await
}

/// The names page, with the notices of the last change.
async fn names_page(
    state: &AppState,
    session: &ActiveSession,
    statuses: Arc<dyn ItemStatusDesk>,
    notices: Vec<Notice>,
) -> Response {
    let Ok(Ok(names)) = tokio::task::spawn_blocking(move || statuses.status_names()).await else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let t = &state.inner.i18n;
    let refusal = notices.iter().any(|notice| notice.refused);
    let kinds = StatusKind::ALL
        .into_iter()
        .map(|kind| StatusKindNames {
            kind: kind.code(),
            name: t.get(generic_key(kind)),
            names: names
                .iter()
                .filter(|name| name.kind == kind)
                .map(|name| (name.number, name.label.clone()))
                .collect(),
        })
        .collect();
    let mut response = render(&StaffStatusNames {
        t,
        csrf: &session.csrf,
        notices,
        kinds,
    });
    if refusal && response.status() == StatusCode::OK {
        *response.status_mut() = StatusCode::UNPROCESSABLE_ENTITY;
    }
    response
}

/// `GET /equipe/circulacao/situacoes`.
pub(crate) async fn status_names(State(state): State<AppState>, headers: HeaderMap) -> Response {
    match open(&state, &headers) {
        Ok((session, _, statuses)) => names_page(&state, &session, statuses, Vec::new()).await,
        Err(response) => *response,
    }
}

/// A name set or removed.
#[derive(Deserialize)]
pub(crate) struct NameForm {
    csrf: String,
    kind: String,
    number: i64,
    #[serde(default)]
    label: String,
    #[serde(default)]
    remove: Option<String>,
}

/// `POST /equipe/circulacao/situacoes`.
pub(crate) async fn change_name(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<NameForm>,
) -> Response {
    let (session, loans, statuses) = match open(&state, &headers) {
        Ok(opened) => opened,
        Err(response) => return *response,
    };
    if !tokens_match(&session.csrf, &form.csrf) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(kind) = StatusKind::from_code(&form.kind) else {
        return StatusCode::UNPROCESSABLE_ENTITY.into_response();
    };
    let change = if form.remove.is_some() {
        StatusNameChange::Remove {
            kind,
            number: form.number,
        }
    } else {
        StatusNameChange::Set(StatusName {
            kind,
            number: form.number,
            label: form.label,
        })
    };
    let actor = session.identity.id;
    let done = tokio::task::spawn_blocking(move || {
        loans.circulate(actor, CirculationCommand::ChangeStatusName(change))
    })
    .await;
    let t = &state.inner.i18n;
    let notices = match done {
        Ok(Ok(_)) => vec![ok(t.get("item-status-saved"))],
        Ok(Err(CirculationDeskError::Refused(refusal))) => refusal_notices(t, &refusal),
        Ok(Err(CirculationDeskError::Busy)) => vec![refused(t.get("circulation-busy"))],
        Ok(Err(CirculationDeskError::Unavailable(_))) | Err(_) => {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };
    names_page(&state, &session, statuses, notices).await
}
