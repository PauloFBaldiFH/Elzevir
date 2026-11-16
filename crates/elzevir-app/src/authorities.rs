// SPDX-License-Identifier: GPL-3.0-or-later
//! Authorities: the agents that records share, edited once for all of them
//! (docs/specs/catalogo/autoridades.md).

use elzevir_domain::catalog::{AgentId, AgentKind, SubjectId, SubjectKind, WorkId};
use serde::Deserialize;

use crate::catalog::{AgentChange, AuthorityRole, CatalogReadError, WorkAuthority};
use crate::editing::FieldError;

/// Most characters of an agent's heading.
pub const MAX_HEADING: usize = 2000;

/// An agent as its focused block shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentView {
    /// The agent.
    pub id: AgentId,
    /// Its kind.
    pub kind: AgentKind,
    /// Its authorized heading.
    pub heading: String,
    /// Its version, which an edit names.
    pub version: i64,
    /// How many Works name it as a contributor.
    pub works: usize,
    /// How many Instances those Works have: the records an edit changes.
    pub instances: usize,
}

/// A subject as its focused block shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubjectView {
    /// The subject.
    pub id: SubjectId,
    /// What it names.
    pub kind: SubjectKind,
    /// Its heading, its subdivisions after ` -- `.
    pub heading: String,
    /// Its thesaurus code, such as `lcsh`.
    pub thesaurus: Option<String>,
    /// Its version, which an edit names.
    pub version: i64,
    /// How many Works have it as subject.
    pub works: usize,
    /// How many Instances those Works have: the records an edit changes.
    pub instances: usize,
}

/// Port: reads the authorities records share. The store implements it.
pub trait AuthorityReader: Send + Sync {
    /// The agent, or `None` when no agent has this id.
    ///
    /// # Errors
    ///
    /// When storage fails.
    fn agent(&self, id: AgentId) -> Result<Option<AgentView>, CatalogReadError>;

    /// The subject, or `None` when no subject has this id.
    ///
    /// # Errors
    ///
    /// When storage fails.
    fn subject(&self, id: SubjectId) -> Result<Option<SubjectView>, CatalogReadError>;
}

/// Validates and normalizes the changes of an agent edit; every error at once.
///
/// # Errors
///
/// The fields that cannot be stored.
pub fn validate_agent_changes(changes: &mut [AgentChange]) -> Result<(), Vec<FieldError>> {
    let mut errors = Vec::new();
    for change in changes.iter_mut() {
        match change {
            AgentChange::Heading(heading) => {
                let trimmed = heading.trim();
                if trimmed.is_empty() || trimmed.chars().count() > MAX_HEADING {
                    errors.push(FieldError {
                        field: "heading",
                        message_key: "edit-error-heading-empty",
                    });
                }
                *heading = trimmed.to_owned();
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// What the "+ Authority" form sends: the authority by its class and heading, and the
/// role it takes in the Work.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct LinkRequest {
    /// The class of the authority; the agent classes are also subject classes.
    pub kind: SubjectKind,
    /// The heading, stored or new.
    pub heading: String,
    /// The role in the Work.
    pub role: LinkRoleName,
    /// The relator code of a contributor.
    #[serde(default)]
    pub relator: Option<String>,
    /// The thesaurus code of a subject.
    #[serde(default)]
    pub thesaurus: Option<String>,
}

/// The roles the form offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkRoleName {
    /// Contributor of the Work.
    Contribution,
    /// Subject of the Work.
    Subject,
}

/// The link of `request` to `work`, validated by role: a contributor must be an agent
/// (docs/specs/catalogo/autoridades.md). Every error at once.
///
/// # Errors
///
/// The fields that cannot be stored.
pub fn link_request(work: WorkId, request: LinkRequest) -> Result<WorkAuthority, Vec<FieldError>> {
    let mut errors = Vec::new();
    let heading = request.heading.trim().to_owned();
    if heading.is_empty() || heading.chars().count() > MAX_HEADING {
        errors.push(FieldError {
            field: "link",
            message_key: "edit-error-heading-empty",
        });
    }
    let code = |value: Option<String>| {
        value
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    };
    let role = match (request.role, request.kind.agent()) {
        (LinkRoleName::Contribution, Some(kind)) => Some(AuthorityRole::Contribution {
            kind,
            heading,
            relator: code(request.relator),
        }),
        (LinkRoleName::Contribution, None) => {
            errors.push(FieldError {
                field: "link",
                message_key: "link-error-not-an-agent",
            });
            None
        }
        (LinkRoleName::Subject, _) => Some(AuthorityRole::Subject {
            kind: request.kind,
            heading,
            thesaurus: code(request.thesaurus),
        }),
    };
    match role {
        Some(role) if errors.is_empty() => Ok(WorkAuthority { work, role }),
        _ => Err(errors),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(kind: SubjectKind, heading: &str, role: LinkRoleName) -> LinkRequest {
        LinkRequest {
            kind,
            heading: heading.to_owned(),
            role,
            relator: Some(" ill ".to_owned()),
            thesaurus: Some(String::new()),
        }
    }

    #[test]
    fn a_contributor_must_be_an_agent_and_a_subject_can_be_anything() {
        let work = WorkId(7);
        let link = link_request(
            work,
            request(
                SubjectKind::Person,
                " Portinari, Candido ",
                LinkRoleName::Contribution,
            ),
        )
        .unwrap();
        assert_eq!(
            link.role,
            AuthorityRole::Contribution {
                kind: AgentKind::Person,
                heading: "Portinari, Candido".to_owned(),
                relator: Some("ill".to_owned()),
            }
        );
        let topic = link_request(
            work,
            request(SubjectKind::Topic, "Cafe", LinkRoleName::Subject),
        )
        .unwrap();
        assert_eq!(
            topic.role,
            AuthorityRole::Subject {
                kind: SubjectKind::Topic,
                heading: "Cafe".to_owned(),
                thesaurus: None,
            }
        );
        let errors = link_request(
            work,
            request(SubjectKind::Topic, " ", LinkRoleName::Contribution),
        )
        .unwrap_err();
        let keys: Vec<_> = errors.iter().map(|error| error.message_key).collect();
        assert_eq!(
            keys,
            ["edit-error-heading-empty", "link-error-not-an-agent"]
        );
    }

    #[test]
    fn a_heading_is_trimmed_and_cannot_be_empty() {
        let mut changes = vec![AgentChange::Heading("  Assis, Machado de ".to_owned())];
        assert!(validate_agent_changes(&mut changes).is_ok());
        assert_eq!(
            changes,
            [AgentChange::Heading("Assis, Machado de".to_owned())]
        );
        let mut empty = vec![AgentChange::Heading(" ".to_owned())];
        let errors = validate_agent_changes(&mut empty).unwrap_err();
        assert_eq!(errors[0].field, "heading");
    }
}
