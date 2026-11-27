// SPDX-License-Identifier: GPL-3.0-or-later
//! The registry of additional patron fields
//! (docs/specs/circulacao/patron-attributes.md, items 1 to 7).

use std::sync::Arc;

use axum::Form;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use elzevir_app::patron_fields::{FieldChange, FieldKind, FieldOption, PatronFieldSpec};
use elzevir_app::patrons::{PatronCommand, PatronDesk, PatronDeskError, PatronRefusal};
use elzevir_app::staff::Permission;
use elzevir_domain::patron::{PatronCategoryId, PatronFieldId, PatronFieldOptionId};
use elzevir_web_templates::{PatronFieldForm, StaffPatronFields};

use crate::patrons::{CategoriesParams, failed, open};
use crate::security::tokens_match;
use crate::session::ActiveSession;
use crate::{AppState, render};

const FIELDS: &str = "/equipe/leitores/campos";

/// `GET /equipe/leitores/campos`.
pub(crate) async fn page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<CategoriesParams>,
) -> Response {
    match open(&state, &headers, |session| {
        session.identity.can(Permission::Patrons)
    }) {
        Ok((session, desk)) => fields_page(&state, &session, desk, params.saved(), None).await,
        Err(response) => *response,
    }
}

/// What one posted form asks for; `None` for a form that makes no sense.
fn field_change(pairs: &[(String, String)]) -> Option<FieldChange> {
    let value = |key: &str| {
        pairs
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    };
    let checked = |key: &str| value(key) == Some("1");
    let id = value("id")
        .and_then(|id| id.parse().ok())
        .map(PatronFieldId);
    let action = value("action")?;
    if action == "delete" {
        return Some(FieldChange::Delete(id?));
    }
    let kind = match value("kind")? {
        "text" => FieldKind::Text,
        "date" => FieldKind::Date,
        "list" => {
            // Existing options come as `option.ID`; an empty one is removed.
            let mut options: Vec<FieldOption> = pairs
                .iter()
                .filter_map(|(name, label)| {
                    let id = name.strip_prefix("option.")?.parse().ok()?;
                    (!label.trim().is_empty()).then(|| FieldOption {
                        id: Some(PatronFieldOptionId(id)),
                        label: label.clone(),
                    })
                })
                .collect();
            options.extend(
                value("new_options")
                    .unwrap_or_default()
                    .lines()
                    .filter(|line| !line.trim().is_empty())
                    .map(|line| FieldOption {
                        id: None,
                        label: line.to_owned(),
                    }),
            );
            FieldKind::List(options)
        }
        _ => return None,
    };
    let spec = PatronFieldSpec {
        name: value("name").unwrap_or_default().to_owned(),
        kind,
        repeatable: checked("repeatable"),
        unique: checked("unique"),
        required: checked("required"),
        category: value("category")
            .and_then(|category| category.parse().ok())
            .filter(|category| *category != 0)
            .map(PatronCategoryId),
        opac_shown: checked("opac_shown"),
        searchable: checked("searchable"),
    };
    match action {
        "create" => Some(FieldChange::Create(spec)),
        "update" => Some(FieldChange::Update { field: id?, spec }),
        _ => None,
    }
}

/// `POST /equipe/leitores/campos`.
pub(crate) async fn change(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(pairs): Form<Vec<(String, String)>>,
) -> Response {
    let (session, desk) = match open(&state, &headers, |session| {
        session.identity.can(Permission::Patrons)
    }) {
        Ok(opened) => opened,
        Err(response) => return *response,
    };
    let csrf = pairs
        .iter()
        .find(|(name, _)| name == "csrf")
        .map_or("", |(_, value)| value.as_str());
    if !tokens_match(&session.csrf, csrf) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(change) = field_change(&pairs) else {
        return (StatusCode::BAD_REQUEST, "unknown change").into_response();
    };
    let actor = session.identity.id;
    let writer = Arc::clone(&desk);
    let outcome = tokio::task::spawn_blocking(move || {
        writer.apply(actor, PatronCommand::ChangeField(Box::new(change)))
    })
    .await;
    match outcome {
        Ok(Ok(_)) => Redirect::to(&format!("{FIELDS}?saved=1")).into_response(),
        Ok(Err(PatronDeskError::Refused(refusal))) => {
            let mut response =
                fields_page(&state, &session, desk, false, Some(refusal.message_key())).await;
            if response.status() == StatusCode::OK {
                *response.status_mut() = status_of(refusal);
            }
            response
        }
        Ok(Err(error)) => failed(&error),
        Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

const fn status_of(refusal: PatronRefusal) -> StatusCode {
    match refusal {
        PatronRefusal::FieldNameTaken
        | PatronRefusal::FieldHasRepeats
        | PatronRefusal::FieldHasShared
        | PatronRefusal::FieldOptionInUse
        | PatronRefusal::FieldInUse
        | PatronRefusal::FieldKindInUse => StatusCode::CONFLICT,
        PatronRefusal::UnknownField | PatronRefusal::UnknownCategory => StatusCode::NOT_FOUND,
        _ => StatusCode::UNPROCESSABLE_ENTITY,
    }
}

async fn fields_page(
    state: &AppState,
    session: &ActiveSession,
    desk: Arc<dyn PatronDesk>,
    saved: bool,
    refused: Option<&'static str>,
) -> Response {
    let read = tokio::task::spawn_blocking(move || Ok((desk.fields()?, desk.categories()?))).await;
    let (fields, categories) = match read {
        Ok(Ok(read)) => read,
        Ok(Err(error)) => return failed(&error),
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    let t = &state.inner.i18n;
    render(&StaffPatronFields {
        t,
        csrf: &session.csrf,
        fields: fields
            .into_iter()
            .map(|line| {
                let (kind, options) = match line.spec.kind {
                    FieldKind::Text => ("text", Vec::new()),
                    FieldKind::Date => ("date", Vec::new()),
                    FieldKind::List(options) => (
                        "list",
                        options
                            .into_iter()
                            .filter_map(|option| Some((option.id?.0, option.label)))
                            .collect(),
                    ),
                };
                PatronFieldForm {
                    id: line.id.0,
                    name: line.spec.name,
                    kind,
                    options,
                    repeatable: line.spec.repeatable,
                    unique: line.spec.unique,
                    required: line.spec.required,
                    category: line.spec.category.map_or(0, |category| category.0),
                    opac_shown: line.spec.opac_shown,
                    searchable: line.spec.searchable,
                    patrons: t.get_numbers("patron-category-patrons", &[("count", line.patrons)]),
                }
            })
            .collect(),
        categories: categories
            .into_iter()
            .map(|category| (category.id.0, category.name))
            .collect(),
        saved,
        refused,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(raw: &[(&str, &str)]) -> Vec<(String, String)> {
        raw.iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[test]
    fn a_list_form_keeps_ids_drops_empty_options_and_adds_new_lines() {
        let change = field_change(&pairs(&[
            ("action", "update"),
            ("id", "4"),
            ("name", "Shift"),
            ("kind", "list"),
            ("category", "0"),
            ("repeatable", "1"),
            ("option.7", "Morning"),
            ("option.8", " "),
            ("new_options", "Evening\r\n\r\nNight"),
        ]))
        .unwrap();
        let FieldChange::Update { field, spec } = change else {
            panic!("not an update");
        };
        assert_eq!(field, PatronFieldId(4));
        assert!(spec.repeatable && !spec.unique && spec.category.is_none());
        let FieldKind::List(options) = spec.kind else {
            panic!("not a list");
        };
        let ids: Vec<_> = options.iter().map(|option| option.id).collect();
        assert_eq!(ids, [Some(PatronFieldOptionId(7)), None, None]);
        assert_eq!(options[2].label, "Night");
    }

    #[test]
    fn forms_that_make_no_sense_are_none() {
        assert!(field_change(&pairs(&[("action", "delete")])).is_none());
        assert!(field_change(&pairs(&[("action", "create"), ("kind", "colour")])).is_none());
        assert!(field_change(&pairs(&[("action", "update"), ("kind", "text")])).is_none());
    }
}
