// SPDX-License-Identifier: GPL-3.0-or-later
//! The blank forms that create a family or an Instance, with what carries over from the
//! previous record (docs/specs/catalogo/criacao.md, items 13 to 19).

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use elzevir_app::catalog::{CatalogReadError, NewIdentifier, NewInstance, NewRecord};
use elzevir_app::description::printed_book;
use elzevir_app::drafts::approved;
use elzevir_app::export::stored_record;
use elzevir_app::listing::{ScanCode, scanned_isbn};
use elzevir_app::staff::Permission;
use elzevir_domain::catalog::{IdentifierKind, InstanceId};
use elzevir_domain::statement::Description;
use elzevir_web_templates::{DraftNote, DraftStation, FormPart, FormSection, I18n, NewRecordForm};
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
    /// The key of an approved draft the family starts from
    /// (docs/specs/catalogo/estacao-de-captura.md, item 8).
    rascunho: Option<String>,
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
    if let Some(key) = &params.rascunho {
        return drafted_family(&state, key).await;
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
    let isbn = params.isbn.as_deref().and_then(scanned_isbn);
    if let Some(isbn) = &isbn {
        start.instance.identifiers = vec![NewIdentifier {
            kind: IdentifierKind::Isbn,
            value: isbn.clone(),
            qualifier: None,
        }];
    }
    let mut form = family_form(t, &start, inherited);
    // A scanned ISBN the catalog lacks is looked up in the sources at once (item 6).
    form.station = station(&state, isbn);
    render(&form)
}

/// The capture station of a new family, when the institution has draft sources.
fn station(state: &AppState, isbn: Option<String>) -> Option<DraftStation> {
    state.drafts.as_ref().map(|drafts| DraftStation {
        sources: drafts.sources(),
        auto: isbn.is_some(),
        isbn: isbn.unwrap_or_default(),
    })
}

/// The form of a new family filled from the draft kept under `key`, with the ISBNs the
/// catalog already has (docs/specs/catalogo/estacao-de-captura.md, items 11 and 12).
async fn drafted_family(state: &AppState, key: &str) -> Response {
    let Some(draft) = state.drafts.as_ref().and_then(|drafts| drafts.kept(key)) else {
        return (StatusCode::NOT_FOUND, state.inner.i18n.get("draft-expired")).into_response();
    };
    let Ok(start) = approved(&draft) else {
        return StatusCode::UNPROCESSABLE_ENTITY.into_response();
    };
    let isbns: Vec<String> = start
        .instance
        .identifiers
        .iter()
        .filter(|identifier| identifier.kind == IdentifierKind::Isbn)
        .filter_map(|identifier| scanned_isbn(&identifier.value))
        .collect();
    let listing = Arc::clone(&state.inner.staff_ports.listing);
    let duplicates = tokio::task::spawn_blocking(move || {
        isbns
            .into_iter()
            .map(|isbn| {
                let found = listing.scanned(ScanCode::Isbn(&isbn))?;
                Ok(found.map(|_| isbn))
            })
            .collect::<Result<Vec<_>, CatalogReadError>>()
    })
    .await;
    let Ok(Ok(duplicates)) = duplicates else {
        return (StatusCode::SERVICE_UNAVAILABLE, "catalog unavailable").into_response();
    };
    let t = &state.inner.i18n;
    let mut form = family_form(t, &start, false);
    form.station = station(state, None);
    form.draft = Some(DraftNote {
        key: key.to_owned(),
        source: draft
            .origin
            .split_once(':')
            .map_or(draft.origin.as_str(), |(_, name)| name)
            .to_owned(),
        duplicates: duplicates.into_iter().flatten().collect(),
    });
    render(&form)
}

/// Query string of a new Instance's form.
#[derive(Deserialize)]
pub(crate) struct NewInstanceParams {
    /// `1`: the form starts as a copy of Instance `{id}` (item 22).
    #[serde(default)]
    copiar: Option<u8>,
}

/// `GET /equipe/acervo/instancia/{id}/nova`: the form of a new Instance of the Work of
/// Instance `{id}`, with its title and category (item 13), or with everything of it but
/// its identifiers when `copiar=1` (item 22).
pub(crate) async fn new_instance(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Query(params): Query<NewInstanceParams>,
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
    let copy = params.copiar == Some(1);
    let start = if copy {
        copied(source)
    } else {
        let mut start = blank();
        start.work_description.classes = source.work_description.classes;
        start.instance.title.clone_from(&source.instance.title);
        start.instance.description.classes = source.instance.description.classes;
        start
    };
    let t = &state.inner.i18n;
    render(&NewRecordForm {
        t,
        endpoint: format!("/equipe/acervo/obra/{}/instancias", view.work.0),
        create: "instance",
        heading_key: if copy {
            "new-instance-copy-heading"
        } else {
            "new-instance-heading"
        },
        submit_key: "new-create",
        parts: vec![FormPart {
            name: "instance",
            heading_key: "new-instance-part",
            sections: instance_sections(t, &start),
        }],
        station: None,
        draft: None,
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

/// An empty family, starting as a printed book ([`printed_book`]).
pub(crate) fn blank() -> NewRecord {
    let (work_description, instance_description) = printed_book();
    NewRecord {
        work_title: String::new(),
        work_description,
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
            description: instance_description,
            ..NewInstance::default()
        },
        items: elzevir_app::catalog::RecordItems::default(),
        source: None,
    }
}

/// The Instance properties a copy leaves out (item 22): its identifiers name that
/// Instance alone, and its administrative metadata (control number, dates) describes the
/// record it came from.
const NOT_COPIED: &[&str] = &["identifiedBy", "adminMetadata"];

/// A new Instance like the one of `source` (item 22): every field but [`NOT_COPIED`] and
/// its identifiers, and without Items, which a form never holds.
fn copied(mut source: NewRecord) -> NewRecord {
    source.instance.identifiers.clear();
    source
        .instance
        .description
        .statements
        .retain(|statement| !NOT_COPIED.contains(&statement.property.as_str()));
    source.items = elzevir_app::catalog::RecordItems::default();
    source.source = None;
    source
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
        submit_key: "new-create",
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
        station: None,
        draft: None,
    }
}

#[cfg(test)]
mod tests {
    use elzevir_app::catalog::NewNote;
    use elzevir_domain::catalog::NoteKind;
    use elzevir_domain::statement::{Statement, Value};

    use super::*;

    #[test]
    fn a_copied_instance_keeps_everything_but_its_identifiers() {
        let mut source = blank();
        source.instance.title = "Dom Casmurro".to_owned();
        source.instance.edition = Some("2. ed.".to_owned());
        source.instance.identifiers.push(NewIdentifier {
            kind: IdentifierKind::Isbn,
            value: "9788535910667".to_owned(),
            qualifier: None,
        });
        source.instance.notes.push(NewNote {
            visibility: None,
            kind: NoteKind::General,
            text: "Inclui índice.".to_owned(),
        });
        let literal = |text: &str| Value::Literal {
            text: text.to_owned(),
            language: None,
        };
        for (property, text) in [
            ("identifiedBy", "T1"),
            ("dimensions", "21 cm"),
            ("adminMetadata", "000465738"),
        ] {
            source.instance.description.statements.push(Statement {
                property: property.to_owned(),
                value: literal(text),
            });
        }
        let copy = copied(source.clone());
        assert!(copy.instance.identifiers.is_empty());
        let mut expected = source.instance;
        expected.identifiers.clear();
        expected
            .description
            .statements
            .retain(|statement| statement.property == "dimensions");
        assert_eq!(copy.instance, expected);
    }

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
