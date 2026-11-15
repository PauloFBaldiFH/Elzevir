// SPDX-License-Identifier: GPL-3.0-or-later
//! The page of the registries: branches with their contact data, shelving places and
//! collections, and circulation policies (docs/specs/catalogo/bibframe-completo.md,
//! items 23d and 23f).

use std::sync::Arc;

use axum::Form;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use elzevir_app::catalog::{CatalogOutput, CatalogRefusal, CodeLists, Named, Place};
use elzevir_app::editing::{Edit, EditFailed};
use elzevir_app::registries::{
    BranchContact, RegistryChange, RegistryEntry, RegistryList, RegistryRefusal,
};
use elzevir_app::staff::Permission;
use elzevir_domain::catalog::{BranchId, CollectionId, PolicyId, SublocationId};
use elzevir_web_templates::{BranchPart, RegistryRow, StaffRegistries};
use serde::Deserialize;

use crate::security::tokens_match;
use crate::session::ActiveSession;
use crate::staff::current_session;
use crate::{AppState, render};

const PATH: &str = "/equipe/admin/cadastros";

/// Query string of the page.
#[derive(Deserialize)]
pub(crate) struct RegistriesParams {
    /// Present after a change was stored.
    saved: Option<String>,
}

/// `GET /equipe/admin/cadastros`.
pub(crate) async fn registries(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<RegistriesParams>,
) -> Response {
    let Some(session) = current_session(&state, &headers) else {
        return Redirect::to("/equipe/login").into_response();
    };
    if !session.identity.can(Permission::Cataloguing) {
        return StatusCode::FORBIDDEN.into_response();
    }
    page(
        &state,
        &session,
        params.saved.is_some(),
        None,
        StatusCode::OK,
    )
    .await
}

/// One posted form.
#[derive(Deserialize)]
pub(crate) struct RegistryForm {
    csrf: String,
    action: String,
    list: String,
    #[serde(default)]
    id: Option<i64>,
    #[serde(default)]
    branch: Option<i64>,
    #[serde(default)]
    name: String,
    #[serde(default)]
    address: Option<String>,
    #[serde(default)]
    phone: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    url: Option<String>,
}

/// The change a form asks for, or `None` when the form does not make sense.
fn change_of(form: RegistryForm) -> Option<RegistryChange> {
    let entry = |id: i64| match form.list.as_str() {
        "branches" => Some(RegistryEntry::Branch(BranchId(id))),
        "sublocations" => Some(RegistryEntry::Sublocation(SublocationId(id))),
        "collections" => Some(RegistryEntry::Collection(CollectionId(id))),
        "policies" => Some(RegistryEntry::Policy(PolicyId(id))),
        _ => None,
    };
    match form.action.as_str() {
        "create" => {
            let list = match (form.list.as_str(), form.branch) {
                ("branches", _) => RegistryList::Branches,
                ("policies", _) => RegistryList::Policies,
                ("sublocations", Some(branch)) => RegistryList::Sublocations(BranchId(branch)),
                ("collections", Some(branch)) => RegistryList::Collections(BranchId(branch)),
                _ => return None,
            };
            Some(RegistryChange::Create {
                list,
                name: form.name,
            })
        }
        "rename" => Some(RegistryChange::Rename {
            entry: entry(form.id?)?,
            name: form.name,
        }),
        "delete" => Some(RegistryChange::Delete(entry(form.id?)?)),
        "contact" if form.list == "branches" => Some(RegistryChange::Contact {
            branch: BranchId(form.id?),
            contact: BranchContact {
                address: form.address,
                phone: form.phone,
                email: form.email,
                url: form.url,
            },
        }),
        _ => None,
    }
}

/// The anchor of the part of the page a change touched.
fn anchor(change: &RegistryChange) -> String {
    match change {
        RegistryChange::Create {
            list: RegistryList::Sublocations(branch) | RegistryList::Collections(branch),
            ..
        }
        | RegistryChange::Contact { branch, .. } => format!("branch-{}", branch.0),
        RegistryChange::Rename {
            entry: RegistryEntry::Policy(_),
            ..
        }
        | RegistryChange::Delete(RegistryEntry::Policy(_))
        | RegistryChange::Create {
            list: RegistryList::Policies,
            ..
        } => "registry-policies".to_owned(),
        _ => "registry-branches".to_owned(),
    }
}

/// `POST /equipe/admin/cadastros`.
pub(crate) async fn change_registry(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<RegistryForm>,
) -> Response {
    let Some(session) = current_session(&state, &headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if !tokens_match(&session.csrf, &form.csrf) || !session.identity.can(Permission::Cataloguing) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(change) = change_of(form) else {
        return (StatusCode::BAD_REQUEST, "unknown change").into_response();
    };
    let target = format!("{PATH}?saved=1#{}", anchor(&change));
    let editor = Arc::clone(&state.inner.staff_ports.editor);
    let actor = session.identity.id;
    let outcome =
        tokio::task::spawn_blocking(move || editor.apply(actor, Edit::Registry(change))).await;
    match outcome {
        Ok(Ok(CatalogOutput::RegistryChanged(_))) => {
            // Records keep ids, but OPAC pages show the names (item 23f).
            state.inner.records.clear();
            Redirect::to(&target).into_response()
        }
        Ok(Ok(_)) => (StatusCode::INTERNAL_SERVER_ERROR, "unexpected answer").into_response(),
        Ok(Err(EditFailed::Refused(CatalogRefusal::Registry(refusal)))) => {
            page(
                &state,
                &session,
                false,
                Some(refusal.message_key()),
                status_of(refusal),
            )
            .await
        }
        Ok(Err(EditFailed::Refused(refusal))) => {
            (StatusCode::UNPROCESSABLE_ENTITY, refusal.to_string()).into_response()
        }
        Ok(Err(EditFailed::Busy | EditFailed::Unavailable(_))) | Err(_) => {
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

const fn status_of(refusal: RegistryRefusal) -> StatusCode {
    match refusal {
        RegistryRefusal::BadName => StatusCode::UNPROCESSABLE_ENTITY,
        RegistryRefusal::NameTaken | RegistryRefusal::InUse => StatusCode::CONFLICT,
        RegistryRefusal::Unknown => StatusCode::NOT_FOUND,
    }
}

async fn page(
    state: &AppState,
    session: &ActiveSession,
    saved: bool,
    refused: Option<&'static str>,
    status: StatusCode,
) -> Response {
    let catalog = Arc::clone(&state.inner.opac.catalog);
    let Ok(Ok(codes)) = tokio::task::spawn_blocking(move || catalog.codes()).await else {
        return (StatusCode::SERVICE_UNAVAILABLE, "catalog unavailable").into_response();
    };
    let mut response = render(&StaffRegistries {
        t: &state.inner.i18n,
        csrf: &session.csrf,
        branches: branches(&codes),
        policies: rows(&codes.policies, |id| id.0),
        saved,
        refused,
    });
    *response.status_mut() = status;
    response
}

fn rows<Id: Copy>(named: &[Named<Id>], key: fn(Id) -> i64) -> Vec<RegistryRow> {
    named
        .iter()
        .map(|row| RegistryRow {
            id: key(row.id),
            name: row.name.clone(),
        })
        .collect()
}

fn of_branch<Id: Copy>(
    places: &[Place<Id>],
    branch: BranchId,
    key: fn(Id) -> i64,
) -> Vec<RegistryRow> {
    places
        .iter()
        .filter(|place| place.branch == branch)
        .map(|place| RegistryRow {
            id: key(place.id),
            name: place.name.clone(),
        })
        .collect()
}

fn branches(codes: &CodeLists) -> Vec<BranchPart> {
    codes
        .branches
        .iter()
        .map(|branch| {
            let contact = codes
                .contacts
                .iter()
                .find(|(id, _)| *id == branch.id)
                .map(|(_, contact)| contact.clone())
                .unwrap_or_default();
            BranchPart {
                row: RegistryRow {
                    id: branch.id.0,
                    name: branch.name.clone(),
                },
                address: contact.address.unwrap_or_default(),
                phone: contact.phone.unwrap_or_default(),
                email: contact.email.unwrap_or_default(),
                url: contact.url.unwrap_or_default(),
                sublocations: of_branch(&codes.locations, branch.id, |id| id.0),
                collections: of_branch(&codes.collections, branch.id, |id| id.0),
            }
        })
        .collect()
}
