// SPDX-License-Identifier: GPL-3.0-or-later
//! The focused block of an authority, an agent or a subject: its form, which says how
//! many records saving it changes, and its edit (docs/specs/catalogo/autoridades.md).

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use elzevir_app::authorities::{LinkRequest, link_request, validate_agent_changes};
use elzevir_app::catalog::{AgentChange, AgentUpdate, AuthorityLink, SubjectUpdate};
use elzevir_app::editing::{Edit, FieldError};
use elzevir_app::staff::Permission;
use elzevir_domain::catalog::{AgentId, InstanceId, SubjectId, WorkId};
use elzevir_web_templates::{
    AuthorityNote, FormField, FormInput, FormPart, FormSection, IdentifierChip, NewRecordForm,
    RecordForm,
};
use serde::Deserialize;

use crate::editing::{EditBody, apply, authorize, invalid};
use crate::record_form::link_field;
use crate::staff::current_session;
use crate::{AppState, render};

/// `GET /equipe/acervo/autoridade/{id}/campos`.
pub(crate) async fn agent_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    if let Some(refused) = refused(&state, &headers) {
        return refused;
    }
    let authorities = Arc::clone(&state.inner.staff_ports.authorities);
    match tokio::task::spawn_blocking(move || authorities.agent(AgentId(id))).await {
        Ok(Ok(Some(agent))) => heading_block(
            &state,
            format!("/equipe/acervo/autoridade/{id}"),
            agent.heading,
            agent.version,
            (agent.works, agent.instances),
            &agent.identifiers,
        ),
        Ok(Ok(None)) => (StatusCode::NOT_FOUND, "no such agent").into_response(),
        Ok(Err(_)) | Err(_) => unavailable(),
    }
}

/// `GET /equipe/acervo/assunto/{id}/campos`.
pub(crate) async fn subject_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    if let Some(refused) = refused(&state, &headers) {
        return refused;
    }
    let authorities = Arc::clone(&state.inner.staff_ports.authorities);
    match tokio::task::spawn_blocking(move || authorities.subject(SubjectId(id))).await {
        Ok(Ok(Some(subject))) => heading_block(
            &state,
            format!("/equipe/acervo/assunto/{id}"),
            subject.heading,
            subject.version,
            (subject.works, subject.instances),
            &subject.identifiers,
        ),
        Ok(Ok(None)) => (StatusCode::NOT_FOUND, "no such subject").into_response(),
        Ok(Err(_)) | Err(_) => unavailable(),
    }
}

/// No session: the login; no cataloguing permission: 403.
fn refused(state: &AppState, headers: &HeaderMap) -> Option<Response> {
    let Some(session) = current_session(state, headers) else {
        return Some(Redirect::to("/equipe/login").into_response());
    };
    (!session.identity.can(Permission::Cataloguing)).then(|| StatusCode::FORBIDDEN.into_response())
}

fn unavailable() -> Response {
    (StatusCode::SERVICE_UNAVAILABLE, "catalog unavailable").into_response()
}

/// The block of an authority: its heading, how many Works and records saving it
/// changes, and its identifiers in other datasets.
fn heading_block(
    state: &AppState,
    endpoint: String,
    heading: String,
    version: i64,
    (works, instances): (usize, usize),
    identifiers: &[AuthorityLink],
) -> Response {
    let t = &state.inner.i18n;
    let count = |n: usize| u64::try_from(n).unwrap_or(u64::MAX);
    render(&RecordForm {
        t,
        endpoint,
        version,
        sections: vec![FormSection {
            heading_key: None,
            fields: vec![FormField {
                change: "Heading",
                error: "heading",
                label_key: "staff-field-authorized-name",
                input: FormInput::Text {
                    value: heading,
                    required: true,
                },
                property: "",
                ontology_label: "",
                inherited: false,
            }],
        }],
        history: None,
        authority: Some(AuthorityNote {
            reach: t.get_numbers(
                "authority-reach",
                &[("works", count(works)), ("instances", count(instances))],
            ),
            identifiers: identifiers.iter().map(identifier_chip).collect(),
        }),
    })
}

/// The datasets whose identifiers are known by their web address.
const DATASETS: &[(&str, &str)] = &[
    ("id.loc.gov", "LC"),
    ("viaf.org", "VIAF"),
    ("wikidata.org", "Wikidata"),
    ("isni.org", "ISNI"),
    ("orcid.org", "ORCID"),
    ("d-nb.info", "GND"),
    ("id.worldcat.org", "WorldCat"),
];

/// A chip for an identifier: a web address opens, under the dataset's name or its host;
/// any other value, such as a control number, is shown as it is.
fn identifier_chip(link: &AuthorityLink) -> IdentifierChip {
    let value = link.value.clone();
    let host = ["https://", "http://"]
        .iter()
        .find_map(|scheme| value.strip_prefix(scheme))
        .and_then(|rest| rest.split('/').next())
        .filter(|host| !host.is_empty());
    let Some(host) = host else {
        return IdentifierChip {
            label: value.clone(),
            value,
            href: None,
        };
    };
    let label = DATASETS
        .iter()
        .find(|(domain, _)| host == *domain || host.ends_with(&format!(".{domain}")))
        .map_or_else(|| host.to_owned(), |(_, name)| (*name).to_owned());
    IdentifierChip {
        label,
        href: Some(value.clone()),
        value,
    }
}

/// `POST /equipe/acervo/autoridade/{id}`.
pub(crate) async fn edit_agent(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<EditBody<AgentChange>>,
) -> Response {
    edit_heading(&state, &headers, body, |row_version, changes| {
        Edit::Agent(Box::new(AgentUpdate {
            agent: AgentId(id),
            row_version,
            changes,
        }))
    })
    .await
}

/// `POST /equipe/acervo/assunto/{id}`.
pub(crate) async fn edit_subject(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<EditBody<AgentChange>>,
) -> Response {
    edit_heading(&state, &headers, body, |row_version, changes| {
        Edit::Subject(Box::new(SubjectUpdate {
            subject: SubjectId(id),
            row_version,
            changes,
        }))
    })
    .await
}

async fn edit_heading(
    state: &AppState,
    headers: &HeaderMap,
    body: EditBody<AgentChange>,
    edit: impl FnOnce(i64, Vec<AgentChange>) -> Edit,
) -> Response {
    let session = match authorize(state, headers) {
        Ok(session) => session,
        Err(status) => return status.into_response(),
    };
    let mut changes = body.changes;
    if let Err(errors) = validate_agent_changes(&mut changes) {
        return invalid(state, &errors);
    }
    apply(state, &session, edit(body.version, changes)).await
}

/// Which Instance of the Work a place of publication goes to.
#[derive(Deserialize)]
pub(crate) struct LinkTarget {
    instancia: Option<i64>,
}

/// `GET /equipe/acervo/obra/{id}/autoridade/nova?instancia=`: the "+ Authority" form of a
/// Work, with the Instance in focus, if any, for a place of publication.
pub(crate) async fn link_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Query(target): Query<LinkTarget>,
) -> Response {
    if let Some(refused) = refused(&state, &headers) {
        return refused;
    }
    let t = &state.inner.i18n;
    render(&NewRecordForm {
        t,
        endpoint: match target.instancia {
            Some(instance) => format!("/equipe/acervo/obra/{id}/autoridades?instancia={instance}"),
            None => format!("/equipe/acervo/obra/{id}/autoridades"),
        },
        create: "authority",
        heading_key: "link-heading",
        submit_key: "link-submit",
        parts: vec![FormPart {
            name: "authority",
            heading_key: "link-heading",
            sections: vec![FormSection {
                heading_key: None,
                fields: vec![link_field(t)],
            }],
        }],
        station: None,
        draft: None,
    })
}

/// What the "+ Authority" form sends: its one field, as every form sends its changes.
#[derive(Deserialize)]
pub(crate) struct LinkBody {
    changes: Vec<LinkChange>,
}

#[derive(Deserialize)]
enum LinkChange {
    Link(LinkRequest),
}

/// `POST /equipe/acervo/obra/{id}/autoridades?instancia=`: links the authority by its role.
pub(crate) async fn link_authority(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Query(target): Query<LinkTarget>,
    Json(body): Json<LinkBody>,
) -> Response {
    let session = match authorize(&state, &headers) {
        Ok(session) => session,
        Err(status) => return status.into_response(),
    };
    let Some(LinkChange::Link(request)) = body.changes.into_iter().next() else {
        return invalid(
            &state,
            &[FieldError {
                field: "link",
                message_key: "edit-error-heading-empty",
            }],
        );
    };
    match link_request(WorkId(id), target.instancia.map(InstanceId), request) {
        Ok(link) => apply(&state, &session, Edit::Link(Box::new(link))).await,
        Err(errors) => invalid(&state, &errors),
    }
}

#[cfg(test)]
mod tests {
    use elzevir_domain::catalog::AuthorityLinkKind;

    use super::*;

    fn chip(value: &str) -> IdentifierChip {
        identifier_chip(&AuthorityLink {
            kind: AuthorityLinkKind::RealWorldObject,
            value: value.to_owned(),
        })
    }

    #[test]
    fn a_web_identifier_opens_under_its_dataset_and_a_number_shows_as_it_is() {
        let viaf = chip("http://viaf.org/viaf/108304418");
        assert_eq!(viaf.label, "VIAF");
        assert_eq!(viaf.href.as_deref(), Some("http://viaf.org/viaf/108304418"));
        assert_eq!(
            chip("https://www.wikidata.org/wiki/Q5360231").label,
            "Wikidata"
        );
        assert_eq!(
            chip("http://id.loc.gov/authorities/names/n79039850").label,
            "LC"
        );
        assert_eq!(chip("https://example.org/a/1").label, "example.org");
        let number = chip("(DLC)n  79021164");
        assert_eq!(number.label, "(DLC)n  79021164");
        assert_eq!(number.href, None);
        assert_eq!(chip("javascript:alert(1)").href, None);
    }
}
