// SPDX-License-Identifier: GPL-3.0-or-later
//! The blank forms that create a family or an Instance, with what carries over from the
//! previous record (docs/specs/catalogo/criacao.md, items 13 to 19).

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use elzevir_app::catalog::{NewIdentifier, NewInstance, NewRecord};
use elzevir_app::description::work_of_material;
use elzevir_app::export::stored_record;
use elzevir_app::listing::scanned_isbn;
use elzevir_app::staff::Permission;
use elzevir_domain::catalog::{IdentifierKind, InstanceId, MaterialType};
use elzevir_domain::statement::Description;
use elzevir_web_templates::{FormPart, FormSection, I18n, NewRecordForm};
use serde::Deserialize;

use crate::form_sections::{instance_sections, work_sections};
use crate::record_form::is_filled;
use crate::staff::current_session;
use crate::{AppState, render};

/// The parts of a Work that carry over to the next new family (item 17).
const CARRIED_WORK: &[&str] = &["Classes", "Languages", "Subjects"];
/// The parts of an Instance that carry over to the next new family (item 17).
const CARRIED_INSTANCE: &[&str] = &["Classes", "Publisher", "Place", "Date"];

/// Query string of a new family's form.
#[derive(Deserialize)]
pub(crate) struct NewFamilyParams {
    /// The Instance created before, whose values carry over.
    from: Option<i64>,
    /// A scanned ISBN the new family starts with (docs/specs/catalogo/bipe.md, item 10).
    isbn: Option<String>,
}

/// `GET /equipe/acervo/nova?from=&isbn=`: the form of a new family (items 14 and 17).
pub(crate) async fn new_family(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<NewFamilyParams>,
) -> Response {
    if let Some(refused) = refused(&state, &headers) {
        return refused;
    }
    let previous = match params.from {
        Some(id) => match read(&state, InstanceId(id)).await {
            Ok(record) => record,
            Err(response) => return response,
        },
        None => None,
    };
    let t = &state.inner.i18n;
    let (mut start, inherited) = if let Some(previous) = &previous {
        (carried_over(previous), true)
    } else {
        (blank(), false)
    };
    // Checked again here: only a valid ISBN is taken, as its 13 digits.
    if let Some(isbn) = params.isbn.as_deref().and_then(scanned_isbn) {
        start.instance.identifiers = vec![NewIdentifier {
            kind: IdentifierKind::Isbn,
            value: isbn,
            qualifier: None,
        }];
    }
    render(&family_form(t, &start, inherited))
}

/// `GET /equipe/acervo/instancia/{id}/nova`: the form of a new Instance of the Work of
/// Instance `{id}`, with its title and category (item 13).
pub(crate) async fn new_instance(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    if let Some(refused) = refused(&state, &headers) {
        return refused;
    }
    let source = match read(&state, InstanceId(id)).await {
        Ok(Some(record)) => record,
        Ok(None) => return (StatusCode::NOT_FOUND, "no such record").into_response(),
        Err(response) => return response,
    };
    let catalog = Arc::clone(&state.inner.opac.catalog);
    let Ok(Ok(Some(view))) =
        tokio::task::spawn_blocking(move || catalog.instance(InstanceId(id))).await
    else {
        return (StatusCode::SERVICE_UNAVAILABLE, "catalog unavailable").into_response();
    };
    let mut start = blank();
    start.work_description.classes = source.work_description.classes;
    start.instance.title.clone_from(&source.instance.title);
    start.instance.description.classes = source.instance.description.classes;
    let t = &state.inner.i18n;
    render(&NewRecordForm {
        t,
        endpoint: format!("/equipe/acervo/obra/{}/instancias", view.work.0),
        create: "instance",
        heading_key: "new-instance-heading",
        parts: vec![FormPart {
            name: "instance",
            heading_key: "new-instance-part",
            sections: instance_sections(t, &start),
        }],
    })
}

/// The answer when the request has no session (to the login) or no cataloguing
/// permission (403); `None` when it may have the form.
fn refused(state: &AppState, headers: &HeaderMap) -> Option<Response> {
    let Some(session) = current_session(state, headers) else {
        return Some(Redirect::to("/equipe/login").into_response());
    };
    (!session.identity.can(Permission::Cataloguing)).then(|| StatusCode::FORBIDDEN.into_response())
}

async fn read(state: &AppState, instance: InstanceId) -> Result<Option<NewRecord>, Response> {
    let records = Arc::clone(&state.inner.staff_ports.records);
    match tokio::task::spawn_blocking(move || stored_record(&*records, instance)).await {
        Ok(Ok(stored)) => Ok(stored.map(|stored| stored.record)),
        Ok(Err(_)) | Err(_) => {
            Err((StatusCode::SERVICE_UNAVAILABLE, "catalog unavailable").into_response())
        }
    }
}

/// An empty family. It starts as most do, a printed book: a monographic text Work
/// with `txt` content and a `bf:Print` Instance; the cataloguer changes what differs.
pub(crate) fn blank() -> NewRecord {
    NewRecord {
        work_title: String::new(),
        material_type: MaterialType::Book,
        work_description: work_of_material(MaterialType::Book),
        hub: None,
        relations: Vec::new(),
        classification: None,
        classification_scheme: None,
        summary: None,
        audience: None,
        languages: Vec::new(),
        contributions: Vec::new(),
        subjects: Vec::new(),
        instance: NewInstance {
            description: Description {
                classes: vec!["Print".to_owned()],
                statements: Vec::new(),
            },
            ..NewInstance::default()
        },
        items: Vec::new(),
        source: None,
    }
}

/// What the next family starts with after `previous` (item 17).
fn carried_over(previous: &NewRecord) -> NewRecord {
    let mut start = blank();
    start.work_description = Description {
        classes: previous.work_description.classes.clone(),
        statements: previous
            .work_description
            .statements
            .iter()
            .filter(|statement| statement.property == "content")
            .cloned()
            .collect(),
    };
    start.languages.clone_from(&previous.languages);
    start.subjects.clone_from(&previous.subjects);
    start
        .instance
        .description
        .classes
        .clone_from(&previous.instance.description.classes);
    start
        .instance
        .publisher
        .clone_from(&previous.instance.publisher);
    start.instance.place.clone_from(&previous.instance.place);
    start.instance.date.clone_from(&previous.instance.date);
    start
}

/// Marks the filled fields that carried over, until the cataloguer confirms them.
fn mark(carried: &[&str], mut sections: Vec<FormSection>, inherited: bool) -> Vec<FormSection> {
    for field in sections.iter_mut().flat_map(|section| &mut section.fields) {
        field.inherited = inherited && is_filled(field) && carried.contains(&field.change);
    }
    sections
}

fn family_form<'a>(t: &'a I18n, start: &NewRecord, inherited: bool) -> NewRecordForm<'a> {
    let work = mark(CARRIED_WORK, work_sections(t, start), inherited);
    // The Instance's title is left empty: it takes the Work's (item 9).
    let instance = mark(CARRIED_INSTANCE, instance_sections(t, start), inherited);
    NewRecordForm {
        t,
        endpoint: "/equipe/acervo/nova".to_owned(),
        create: "family",
        heading_key: "new-family-heading",
        parts: vec![
            FormPart {
                name: "work",
                heading_key: "new-work-part",
                sections: work,
            },
            FormPart {
                name: "instance",
                heading_key: "new-instance-part",
                sections: instance,
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_carried_over_parts_are_the_ones_the_form_names() {
        // Each name is a field change of the profile, or the mark would never show.
        let t = I18n::english().unwrap();
        let start = blank();
        let form = family_form(&t, &start, false);
        let changes = |name: &str| -> Vec<&str> {
            form.parts
                .iter()
                .find(|part| part.name == name)
                .unwrap()
                .sections
                .iter()
                .flat_map(|section| &section.fields)
                .map(|field| field.change)
                .collect()
        };
        for carried in CARRIED_WORK {
            assert!(changes("work").contains(carried), "{carried}");
        }
        for carried in CARRIED_INSTANCE {
            assert!(changes("instance").contains(carried), "{carried}");
        }
    }
}
