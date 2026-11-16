// SPDX-License-Identifier: GPL-3.0-or-later
//! Authorities: the agents that records share, edited once for all of them
//! (docs/specs/catalogo/autoridades.md).

use elzevir_domain::catalog::{AgentId, AgentKind};

use crate::catalog::{AgentChange, CatalogReadError};
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

/// Port: reads an agent. The store implements it.
pub trait AgentReader: Send + Sync {
    /// The agent, or `None` when no agent has this id.
    ///
    /// # Errors
    ///
    /// When storage fails.
    fn agent(&self, id: AgentId) -> Result<Option<AgentView>, CatalogReadError>;
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

#[cfg(test)]
mod tests {
    use super::*;

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
