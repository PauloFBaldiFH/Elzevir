// SPDX-License-Identifier: GPL-3.0-or-later
//! Creating a family or an Instance from the staff screen (docs/specs/catalogo/criacao.md,
//! items 5 to 12). The screen sends the same changes as an edit; the app turns them into
//! a new record and the writer stores it.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use elzevir_app::catalog::{InstanceChange, NewInstanceOfWork, WorkChange};
use elzevir_app::creating::{new_instance, new_record};
use elzevir_app::drafts::approved;
use elzevir_app::editing::{Edit, FieldError};
use elzevir_domain::catalog::WorkId;
use serde::Deserialize;

use crate::AppState;
use crate::editing::{FormChange, apply, authorize, instance_changes, invalid, work_changes};

/// A new family as the screen sends it.
#[derive(Deserialize)]
pub(crate) struct NewFamilyBody {
    #[serde(default)]
    work: Vec<FormChange<WorkChange>>,
    #[serde(default)]
    instance: Vec<FormChange<InstanceChange>>,
    /// The key of the approved draft the family came from
    /// (docs/specs/catalogo/estacao-de-captura.md, items 9 and 13).
    #[serde(default)]
    draft: Option<String>,
}

/// A new Instance as the screen sends it.
#[derive(Deserialize)]
pub(crate) struct NewInstanceBody {
    changes: Vec<FormChange<InstanceChange>>,
}

/// `POST /equipe/acervo/nova` (item 5).
pub(crate) async fn create_family(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<NewFamilyBody>,
) -> Response {
    let session = match authorize(&state, &headers) {
        Ok(session) => session,
        Err(status) => return status.into_response(),
    };
    let NewFamilyBody {
        work,
        instance,
        draft,
    } = body;
    let (work, instance) = match (work_changes(work), instance_changes(instance)) {
        (Ok(work), Ok(instance)) => (work, instance),
        (work, instance) => {
            let errors: Vec<_> = work
                .err()
                .into_iter()
                .chain(instance.err())
                .flatten()
                .collect();
            return invalid(&state, &errors);
        }
    };
    let mut record = match new_record(work, instance) {
        Ok(record) => record,
        Err(errors) => return invalid(&state, &errors),
    };
    if let Some(key) = draft {
        // The record as received, never as the browser sent it (item 14).
        let source = state
            .drafts
            .as_ref()
            .and_then(|drafts| drafts.kept(&key))
            .and_then(|draft| approved(&draft).ok());
        let Some(source) = source else {
            return invalid(
                &state,
                &[FieldError {
                    field: "draft",
                    message_key: "draft-expired",
                }],
            );
        };
        record.source = source.source;
        record.instance.origin = source.instance.origin;
    }
    apply(&state, &session, Edit::NewRecord(Box::new(record))).await
}

/// `POST /equipe/acervo/obra/{id}/instancias` (item 6).
pub(crate) async fn create_instance(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<NewInstanceBody>,
) -> Response {
    let session = match authorize(&state, &headers) {
        Ok(session) => session,
        Err(status) => return status.into_response(),
    };
    let changes = match instance_changes(body.changes) {
        Ok(changes) => changes,
        Err(errors) => return invalid(&state, &errors),
    };
    match new_instance(changes) {
        Ok(instance) => {
            let new = NewInstanceOfWork {
                work: WorkId(id),
                instance,
            };
            apply(&state, &session, Edit::NewInstance(Box::new(new))).await
        }
        Err(errors) => invalid(&state, &errors),
    }
}
