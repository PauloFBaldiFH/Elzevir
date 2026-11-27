// SPDX-License-Identifier: GPL-3.0-or-later
//! Batch actions on the families selected in the listing (docs/specs/catalogo/lote.md).

use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use elzevir_app::batch::{ItemSet, ItemsChange, instances_of, selection};
use elzevir_app::catalog::{CodeLists, Place};
use elzevir_app::editing::Edit;
use elzevir_app::export::{ExportOptions, RecordFormat, export_selection};
use elzevir_app::printing::{self, PrintError};
use elzevir_app::worklist::{SavedList, SavedListChange, SavedListId, WorkList};
use elzevir_domain::catalog::{BranchId, CollectionId, PolicyId, SublocationId};
use elzevir_web_templates::{BatchChoices, GridOption};
use serde::Deserialize;

use crate::AppState;
use crate::editing::{apply, authorize};
use crate::staff::current_session;

/// Response header naming the Instances left out of an export (lote.md, item 8).
const NOT_EXPORTED: &str = "x-elzevir-not-exported";

/// `GET /equipe/acervo/lote/marc?format=marc21|marcxml&head=…`: the records of the
/// selected families as one attachment (lote.md, items 5 to 8).
pub(crate) async fn export_marc(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(pairs): Query<Vec<(String, String)>>,
) -> Response {
    if current_session(&state, &headers).is_none() {
        return Redirect::to("/equipe/login").into_response();
    }
    let format = pairs
        .iter()
        .rev()
        .find(|(name, _)| name == "format")
        .map(|(_, value)| value.as_str());
    let Some(format) = RecordFormat::of(format) else {
        return (StatusCode::BAD_REQUEST, "unknown format").into_response();
    };
    let heads = selection(
        pairs
            .iter()
            .filter(|(name, _)| name == "head")
            .map(|(_, value)| value.as_str()),
    );
    if heads.is_empty() {
        return (StatusCode::BAD_REQUEST, "no family selected").into_response();
    }
    let listing = Arc::clone(&state.inner.staff_ports.listing);
    let records = Arc::clone(&state.inner.staff_ports.records);
    let written = tokio::task::spawn_blocking(move || {
        let instances = instances_of(&*listing, &heads)?;
        export_selection(
            &*records,
            &instances,
            format,
            &ExportOptions::default().limits,
        )
    })
    .await;
    let Ok(Ok(export)) = written else {
        return (StatusCode::SERVICE_UNAVAILABLE, "catalog unavailable").into_response();
    };
    let left_out = export
        .failures
        .iter()
        .map(|failed| failed.instance.0.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let mut response = (
        [
            (header::CONTENT_TYPE, format.content_type().to_owned()),
            (
                header::CONTENT_DISPOSITION,
                format!(
                    "attachment; filename=\"elzevir-lote{}\"",
                    format.file_suffix()
                ),
            ),
        ],
        export.bytes,
    )
        .into_response();
    if let Ok(value) = left_out.parse()
        && !left_out.is_empty()
    {
        response.headers_mut().insert(NOT_EXPORTED, value);
    }
    response
}

/// Response header with how many Items of the selection have no call number
/// (etiquetas.md, item 2).
const WITHOUT_CALL_NUMBER: &str = "x-elzevir-without-call-number";

/// `GET /equipe/acervo/lote/etiquetas?head=…`: the spine labels of the Items of the
/// selected families, as one PDF (docs/specs/catalogo/etiquetas.md, items 1 to 4).
pub(crate) async fn spine_labels(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(pairs): Query<Vec<(String, String)>>,
) -> Response {
    if current_session(&state, &headers).is_none() {
        return Redirect::to("/equipe/login").into_response();
    }
    let Some(printer) = state.printer.clone() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let heads = selection(
        pairs
            .iter()
            .filter(|(name, _)| name == "head")
            .map(|(_, value)| value.as_str()),
    );
    if heads.is_empty() {
        return (StatusCode::BAD_REQUEST, "no family selected").into_response();
    }
    let listing = Arc::clone(&state.inner.staff_ports.listing);
    let typeset = tokio::task::spawn_blocking(move || {
        let families = listing.families(&heads).ok()?;
        let (labels, without) = printing::spine_labels(&families);
        Some((printer.spine_labels(&labels), labels.is_empty(), without))
    })
    .await;
    let Ok(Some((document, empty, without))) = typeset else {
        return (StatusCode::SERVICE_UNAVAILABLE, "catalog unavailable").into_response();
    };
    if empty {
        return (StatusCode::BAD_REQUEST, "no Item with a call number").into_response();
    }
    match document {
        Ok(pdf) => (
            [
                (header::CONTENT_TYPE, "application/pdf".to_owned()),
                (
                    header::CONTENT_DISPOSITION,
                    "attachment; filename=\"etiquetas.pdf\"".to_owned(),
                ),
                (
                    header::HeaderName::from_static(WITHOUT_CALL_NUMBER),
                    without.to_string(),
                ),
            ],
            pdf,
        )
            .into_response(),
        Err(error @ PrintError::TooMany { .. }) => {
            (StatusCode::PAYLOAD_TOO_LARGE, error.to_string()).into_response()
        }
        Err(error @ PrintError::Template(_)) => {
            (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response()
        }
    }
}

/// The body of a batch change: the selected families and, per column, `""` to leave it,
/// an id to set it, or `"none"` to clear a shelving place or collection (lote.md, item 14).
#[derive(Debug, Deserialize)]
pub(crate) struct ChangeBody {
    heads: Vec<String>,
    set: SetBody,
}

/// The columns of a batch change, as the dialog sends them.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct SetBody {
    home_branch: String,
    holding_branch: String,
    policy: String,
    location: String,
    collection: String,
}

/// What clears a shelving place or collection.
const NONE: &str = "none";

impl SetBody {
    /// The changes chosen, in column order; `None` when a value is not an id.
    fn item_sets(&self) -> Option<Vec<ItemSet>> {
        let id = |value: &str| value.parse::<i64>().ok();
        let place = |value: &str| match value {
            NONE => Some(None),
            value => id(value).map(Some),
        };
        let mut sets = Vec::new();
        if !self.home_branch.is_empty() {
            sets.push(ItemSet::HomeBranch(BranchId(id(&self.home_branch)?)));
        }
        if !self.holding_branch.is_empty() {
            sets.push(ItemSet::HoldingBranch(BranchId(id(&self.holding_branch)?)));
        }
        if !self.policy.is_empty() {
            sets.push(ItemSet::Policy(PolicyId(id(&self.policy)?)));
        }
        if !self.location.is_empty() {
            sets.push(ItemSet::Location(place(&self.location)?.map(SublocationId)));
        }
        if !self.collection.is_empty() {
            sets.push(ItemSet::Collection(
                place(&self.collection)?.map(CollectionId),
            ));
        }
        Some(sets)
    }
}

/// `POST /equipe/acervo/lote/exemplares`: every Item of the selected families changed at
/// once (lote.md, items 9 to 14).
pub(crate) async fn change_items(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ChangeBody>,
) -> Response {
    let session = match authorize(&state, &headers) {
        Ok(session) => session,
        Err(status) => return status.into_response(),
    };
    let Some(set) = body.set.item_sets() else {
        return (StatusCode::BAD_REQUEST, "malformed change").into_response();
    };
    let heads = selection(body.heads.iter().map(String::as_str));
    if heads.is_empty() {
        return (StatusCode::BAD_REQUEST, "no family selected").into_response();
    }
    let listing = Arc::clone(&state.inner.staff_ports.listing);
    let read = tokio::task::spawn_blocking(move || instances_of(&*listing, &heads)).await;
    let Ok(Ok(instances)) = read else {
        return (StatusCode::SERVICE_UNAVAILABLE, "catalog unavailable").into_response();
    };
    let change = ItemsChange { instances, set };
    apply(&state, &session, Edit::BatchItems(Box::new(change))).await
}

/// What the batch dialog offers (lote.md, item 9): shelving places and collections carry
/// the name of their branch.
pub(crate) fn choices(
    codes: &CodeLists,
    saved: &[SavedList],
    shown: Option<SavedListId>,
) -> BatchChoices {
    let named = |id: i64, name: &str| GridOption {
        key: id.to_string(),
        name: name.to_owned(),
        branch: None,
    };
    let placed = |id: i64, place_branch: BranchId, name: &str| GridOption {
        key: id.to_string(),
        name: name.to_owned(),
        branch: Some(codes.branch(place_branch).to_owned()),
    };
    BatchChoices {
        branches: codes
            .branches
            .iter()
            .map(|branch| named(branch.id.0, &branch.name))
            .collect(),
        policies: codes
            .policies
            .iter()
            .map(|policy| named(policy.id.0, &policy.name))
            .collect(),
        locations: places(&codes.locations, |id| id.0, &placed),
        collections: places(&codes.collections, |id| id.0, &placed),
        lists: saved
            .iter()
            .map(|list| named(list.id.0, &list.name))
            .collect(),
        current_list: shown.map(|list| list.0),
    }
}

fn places<Id: Copy>(
    rows: &[Place<Id>],
    key: fn(Id) -> i64,
    placed: &impl Fn(i64, BranchId, &str) -> GridOption,
) -> Vec<GridOption> {
    rows.iter()
        .map(|place| placed(key(place.id), place.branch, &place.name))
        .collect()
}

/// The body of a change of the saved lists (lote.md, item 22).
#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub(crate) enum ListBody {
    Create { name: String, heads: Vec<String> },
    Add { list: i64, heads: Vec<String> },
    Remove { list: i64, heads: Vec<String> },
    Delete { list: i64 },
}

impl ListBody {
    /// The change, its families read as a selection is; `None` when it names none.
    fn change(self) -> Option<SavedListChange> {
        let heads = |names: &[String]| selection(names.iter().map(String::as_str));
        Some(match self {
            Self::Create { name, heads: names } => SavedListChange::Create {
                name,
                heads: heads(&names),
            },
            Self::Add { list, heads: names } => SavedListChange::Add {
                list: SavedListId(list),
                heads: Some(heads(&names)).filter(|heads| !heads.is_empty())?,
            },
            Self::Remove { list, heads: names } => SavedListChange::Remove {
                list: SavedListId(list),
                heads: Some(heads(&names)).filter(|heads| !heads.is_empty())?,
            },
            Self::Delete { list } => SavedListChange::Delete {
                list: SavedListId(list),
            },
        })
    }
}

/// `POST /equipe/acervo/listas`: a saved list created, filled, emptied or deleted
/// (lote.md, items 15 to 22).
pub(crate) async fn change_list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ListBody>,
) -> Response {
    let session = match authorize(&state, &headers) {
        Ok(session) => session,
        Err(status) => return status.into_response(),
    };
    let Some(change) = body.change() else {
        return (StatusCode::BAD_REQUEST, "no family selected").into_response();
    };
    apply(&state, &session, Edit::SavedList(change)).await
}

/// The answer to a stored change of a saved list: where the list is now.
pub(crate) fn list_answer(list: SavedListId) -> serde_json::Value {
    serde_json::json!({
        "list": list.0,
        "url": crate::facets::list_url(WorkList::Saved(list)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dialog_values_become_changes_in_column_order() {
        let body = SetBody {
            home_branch: "2".to_owned(),
            location: NONE.to_owned(),
            collection: "9".to_owned(),
            ..SetBody::default()
        };
        assert_eq!(
            body.item_sets(),
            Some(vec![
                ItemSet::HomeBranch(BranchId(2)),
                ItemSet::Location(None),
                ItemSet::Collection(Some(CollectionId(9))),
            ])
        );
        assert_eq!(SetBody::default().item_sets(), Some(Vec::new()));
        let bad = SetBody {
            policy: NONE.to_owned(),
            ..SetBody::default()
        };
        assert_eq!(bad.item_sets(), None, "a policy cannot be cleared");
    }
}
