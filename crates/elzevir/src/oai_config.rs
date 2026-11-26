// SPDX-License-Identifier: GPL-3.0-or-later
//! `[oai]` of `elzevir.toml` and the repository it opens (docs/specs/opac/oai-pmh.md,
//! items 1 and 2).

use std::sync::Arc;

use elzevir_app::oai::OaiSettings;
use elzevir_store::ElzevirStore;
use elzevir_web::{OaiEvent, OaiRepository};
use serde::Deserialize;

use crate::logging;

/// The default records per response (`OAI-PMH:MaxCount`).
const DEFAULT_MAX_COUNT: usize = 50;
/// The most records per response.
const MAX_MAX_COUNT: usize = 1_000;

/// `[oai]` as written.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OaiSection {
    #[serde(default)]
    enabled: bool,
    repository_identifier: Option<String>,
    base_url: Option<String>,
    repository_name: Option<String>,
    #[serde(default)]
    admin_email: Vec<String>,
    max_count: Option<usize>,
}

impl OaiSection {
    /// The settings when the repository is on, or what is wrong with them.
    pub(crate) fn settings(&self) -> Result<Option<OaiSettings>, String> {
        if !self.enabled {
            return Ok(None);
        }
        let identifier = self
            .repository_identifier
            .clone()
            .ok_or("repository_identifier is required")?;
        let valid_identifier = (1..=64).contains(&identifier.len())
            && identifier
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'));
        if !valid_identifier {
            return Err(
                "repository_identifier has 1 to 64 letters, digits, '.', '-' or '_'".to_owned(),
            );
        }
        let base_url = self.base_url.clone().ok_or("base_url is required")?;
        let valid_url = (base_url.starts_with("http://") || base_url.starts_with("https://"))
            && !base_url.contains(['?', '#', ' '])
            && base_url.len() <= 512;
        if !valid_url {
            return Err("base_url is an http:// or https:// address, without ? or #".to_owned());
        }
        let name = self
            .repository_name
            .as_deref()
            .map(str::trim)
            .filter(|name| (1..=200).contains(&name.chars().count()))
            .ok_or("repository_name has 1 to 200 characters")?;
        if self.admin_email.is_empty()
            || self
                .admin_email
                .iter()
                .any(|email| !email.contains('@') || email.len() > 254)
        {
            return Err("admin_email lists at least one e-mail address".to_owned());
        }
        let max_count = self.max_count.unwrap_or(DEFAULT_MAX_COUNT);
        if !(1..=MAX_MAX_COUNT).contains(&max_count) {
            return Err(format!("max_count is 1 to {MAX_MAX_COUNT}"));
        }
        Ok(Some(OaiSettings {
            repository_identifier: identifier,
            base_url,
            repository_name: name.to_owned(),
            admin_emails: self.admin_email.clone(),
            max_count,
        }))
    }
}

/// The repository of `settings` over `store`, logging as `instance`.
pub(crate) fn repository(
    settings: &OaiSettings,
    store: &Arc<ElzevirStore>,
    instance: &str,
) -> OaiRepository {
    let instance = instance.to_owned();
    OaiRepository {
        catalog: Arc::clone(store) as _,
        settings: Arc::new(settings.clone()),
        log: Arc::new(move |event: OaiEvent| logging::oai_request(&instance, &event)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(text: &str) -> Result<Option<OaiSettings>, String> {
        toml::from_str::<OaiSection>(text)
            .map_err(|e| e.to_string())?
            .settings()
    }

    const VALID: &str = "enabled = true\nrepository_identifier = \"ELZ-TEST\"\n\
        base_url = \"https://opac.example.org/oai\"\nrepository_name = \"Biblioteca\"\n\
        admin_email = [\"admin@example.org\"]\n";

    #[test]
    fn a_section_is_off_until_enabled_and_takes_the_defaults() {
        assert_eq!(settings("").unwrap(), None);
        assert_eq!(settings("enabled = false\nmax_count = 0").unwrap(), None);
        let on = settings(VALID).unwrap().unwrap();
        assert_eq!(on.max_count, 50);
        assert_eq!(on.admin_emails, ["admin@example.org"]);
    }

    #[test]
    fn a_wrong_section_says_what_is_wrong() {
        for (change, error) in [
            (
                "repository_identifier = \"ELZ-TEST\"",
                "repository_identifier = \"ELZ TEST\"",
            ),
            (
                "base_url = \"https://opac.example.org/oai\"",
                "base_url = \"ftp://x\"",
            ),
            (
                "repository_name = \"Biblioteca\"",
                "repository_name = \" \"",
            ),
            ("admin_email = [\"admin@example.org\"]", "admin_email = []"),
        ] {
            let text = VALID.replace(change, error);
            assert!(settings(&text).is_err(), "{text}");
        }
        assert!(settings(&format!("{VALID}max_count = 1001")).is_err());
        assert!(settings(&format!("{VALID}sets = true")).is_err());
    }
}
