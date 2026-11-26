// SPDX-License-Identifier: GPL-3.0-or-later
//! `[sru]` of `elzevir.toml` and the server it opens (docs/specs/opac/sru.md, items 1
//! and 2).

use std::sync::Arc;

use elzevir_app::sru::SruSettings;
use elzevir_search::TantivyIndex;
use elzevir_store::ElzevirStore;
use elzevir_web::{SruEvent, SruServer};
use serde::Deserialize;

use crate::logging;

/// Records per response when the client asks for none.
const DEFAULT_RECORDS: usize = 10;
/// The default most records per response.
const DEFAULT_MAX_RECORDS: usize = 50;
/// The highest `max_records`.
const MAX_MAX_RECORDS: usize = 1_000;

/// `[sru]` as written.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SruSection {
    #[serde(default)]
    enabled: bool,
    base_url: Option<String>,
    title: Option<String>,
    default_records: Option<usize>,
    max_records: Option<usize>,
}

impl SruSection {
    /// The settings when the server is on, or what is wrong with them.
    pub(crate) fn settings(&self) -> Result<Option<SruSettings>, String> {
        if !self.enabled {
            return Ok(None);
        }
        let base_url = self.base_url.as_deref().ok_or("base_url is required")?;
        let (host, port, database) = address(base_url)
            .ok_or("base_url is an http:// or https:// address with a host, without ? or #")?;
        let title = self
            .title
            .as_deref()
            .map(str::trim)
            .filter(|title| (1..=200).contains(&title.chars().count()))
            .ok_or("title has 1 to 200 characters")?;
        let max_records = self.max_records.unwrap_or(DEFAULT_MAX_RECORDS);
        if !(1..=MAX_MAX_RECORDS).contains(&max_records) {
            return Err(format!("max_records is 1 to {MAX_MAX_RECORDS}"));
        }
        let default_records = self
            .default_records
            .unwrap_or_else(|| DEFAULT_RECORDS.min(max_records));
        if !(1..=max_records).contains(&default_records) {
            return Err("default_records is 1 to max_records".to_owned());
        }
        Ok(Some(SruSettings {
            host,
            port,
            database,
            title: title.to_owned(),
            default_records,
            max_records,
        }))
    }
}

/// The host, port and path (without its leading `/`) of an `http(s)://` address.
fn address(url: &str) -> Option<(String, u16, String)> {
    if url.len() > 512 || url.contains(['?', '#', ' ', '@']) {
        return None;
    }
    let (rest, default_port) = url
        .strip_prefix("https://")
        .map(|rest| (rest, 443))
        .or_else(|| url.strip_prefix("http://").map(|rest| (rest, 80)))?;
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !host.ends_with(']') || authority.starts_with('[') => {
            (host, port.parse().ok()?)
        }
        _ => (authority, default_port),
    };
    if host.is_empty() {
        return None;
    }
    Some((host.to_owned(), port, path.trim_end_matches('/').to_owned()))
}

/// The server of `settings` over `store` and `index`, logging as `instance`.
pub(crate) fn server(
    settings: &SruSettings,
    store: &Arc<ElzevirStore>,
    index: &Arc<TantivyIndex>,
    instance: &str,
) -> SruServer {
    let instance = instance.to_owned();
    SruServer {
        catalog: Arc::clone(store) as _,
        index: Arc::clone(index) as _,
        settings: Arc::new(settings.clone()),
        log: Arc::new(move |event: SruEvent| logging::sru_request(&instance, &event)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(text: &str) -> Result<Option<SruSettings>, String> {
        toml::from_str::<SruSection>(text)
            .map_err(|e| e.to_string())?
            .settings()
    }

    const VALID: &str =
        "enabled = true\nbase_url = \"https://opac.example.org/sru\"\ntitle = \"Biblioteca\"\n";

    #[test]
    fn a_section_is_off_until_enabled_and_takes_the_defaults() {
        assert_eq!(settings("").unwrap(), None);
        assert_eq!(settings("enabled = false\nmax_records = 0").unwrap(), None);
        let on = settings(VALID).unwrap().unwrap();
        assert_eq!(
            on,
            SruSettings {
                host: "opac.example.org".to_owned(),
                port: 443,
                database: "sru".to_owned(),
                title: "Biblioteca".to_owned(),
                default_records: 10,
                max_records: 50,
            }
        );
        let small = settings(&format!("{VALID}max_records = 5"))
            .unwrap()
            .unwrap();
        assert_eq!(small.default_records, 5);
    }

    #[test]
    fn the_address_gives_host_port_and_database() {
        assert_eq!(
            address("http://catalogo.example:8080/opac/sru/"),
            Some(("catalogo.example".to_owned(), 8080, "opac/sru".to_owned()))
        );
        assert_eq!(
            address("http://[::1]:9000/sru"),
            Some(("[::1]".to_owned(), 9000, "sru".to_owned()))
        );
        assert_eq!(
            address("https://catalogo.example"),
            Some(("catalogo.example".to_owned(), 443, String::new()))
        );
        for wrong in [
            "ftp://x/sru",
            "https:///sru",
            "https://x/sru?a=1",
            "https://u@x/sru",
            "https://x:port/sru",
        ] {
            assert_eq!(address(wrong), None, "{wrong}");
        }
    }

    #[test]
    fn a_wrong_section_says_what_is_wrong() {
        for text in [
            VALID.replace("https://opac.example.org/sru", "opac"),
            VALID.replace("\"Biblioteca\"", "\" \""),
            format!("{VALID}max_records = 1001"),
            format!("{VALID}default_records = 51"),
            format!("{VALID}default_records = 0"),
            format!("{VALID}scan = true"),
            "enabled = true\ntitle = \"x\"".to_owned(),
        ] {
            assert!(settings(&text).is_err(), "{text}");
        }
    }
}
