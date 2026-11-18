// SPDX-License-Identifier: GPL-3.0-or-later
//! Explicit, bounded server configuration. CLI overrides never rewrite the file.

use std::io::Read;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const MAX_BYTES: usize = 64 * 1024;

/// The existing server flags plus an optional configuration file.
#[derive(clap::Args)]
pub(crate) struct ServeArgs {
    /// TOML configuration file (paths inside it are relative to its directory).
    #[arg(long)]
    pub(crate) config: Option<PathBuf>,
    /// Institution data directory (overrides the configuration file).
    #[arg(long)]
    pub(crate) data_dir: Option<PathBuf>,
    /// Address to listen on (default: 127.0.0.1:8080).
    #[arg(long)]
    pub(crate) listen: Option<SocketAddr>,
    /// Allow plain-HTTP cookies; use =false to override a true configuration value.
    #[arg(long, num_args = 0..=1, require_equals = true, default_missing_value = "true")]
    pub(crate) insecure_cookies: Option<bool>,
    /// Base of the BIBFRAME export IRIs; defaults to the requested host's /bibframe.
    #[arg(long)]
    iri_base: Option<String>,
    /// Override the operating-system language.
    #[arg(long, value_parser = ["en", "pt-BR"])]
    pub(crate) lang: Option<String>,
}

/// Validated options used to start one institution's server.
pub(crate) struct Settings {
    pub(crate) institution_name: Option<String>,
    pub(crate) lang: String,
    pub(crate) staff: crate::access_config::StaffMode,
    pub(crate) data_dir: PathBuf,
    pub(crate) listen: SocketAddr,
    pub(crate) insecure_cookies: bool,
    pub(crate) iri_base: Option<String>,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct FileConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    institution: Option<Institution>,
    server: Server,
    staff: crate::access_config::StaffMode,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Institution {
    name: String,
}

#[derive(Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct Server {
    data_dir: Option<PathBuf>,
    listen: SocketAddr,
    insecure_cookies: bool,
    iri_base: Option<String>,
}

impl Default for Server {
    fn default() -> Self {
        Self {
            data_dir: None,
            listen: SocketAddr::from(([127, 0, 0, 1], 8080)),
            insecure_cookies: false,
            iri_base: None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ConfigError {
    #[error("invalid staff access configuration")]
    InvalidAccess,
    #[error("cannot read configuration {}: {source}", path.display())]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("configuration {} exceeds 64 KiB", .0.display())]
    TooLarge(PathBuf),
    #[error("invalid configuration {} at line {line}, column {column}; check supported keys and value types", path.display())]
    Invalid {
        path: PathBuf,
        line: usize,
        column: usize,
    },
    #[error("set server.data_dir in the configuration or pass --data-dir")]
    MissingDataDir,
    #[error("data directory must not be empty")]
    EmptyDataDir,
    #[error("institution name must contain 1 to 200 characters and no control characters")]
    InvalidName,
    #[error("cannot serialize initialization configuration")]
    Serialize,
}

impl ServeArgs {
    pub(crate) fn for_status(config: Option<PathBuf>, data_dir: Option<PathBuf>) -> Self {
        Self {
            config,
            data_dir,
            listen: None,
            insecure_cookies: None,
            iri_base: None,
            lang: None,
        }
    }

    pub(crate) fn resolve(self) -> Result<Settings, ConfigError> {
        let mut file = match &self.config {
            Some(path) => load(path)?,
            None => FileConfig::default(),
        };
        file.staff
            .validate(Some(self.listen.unwrap_or(file.server.listen)))
            .map_err(|_| ConfigError::InvalidAccess)?;
        let server = file.server;
        let from_file = server.data_dir.map(|directory| {
            if directory.is_absolute() {
                directory
            } else {
                self.config
                    .as_deref()
                    .and_then(Path::parent)
                    .unwrap_or_else(|| Path::new("."))
                    .join(directory)
            }
        });
        let data_dir = self
            .data_dir
            .or(from_file)
            .ok_or(ConfigError::MissingDataDir)?;
        if data_dir.as_os_str().is_empty() {
            return Err(ConfigError::EmptyDataDir);
        }
        Ok(Settings {
            lang: crate::first_use::language(self.lang.as_deref()).to_owned(),
            institution_name: file.institution.map(|institution| institution.name),
            staff: file.staff,
            data_dir,
            listen: self.listen.unwrap_or(server.listen),
            insecure_cookies: self.insecure_cookies.unwrap_or(server.insecure_cookies),
            iri_base: self.iri_base.or(server.iri_base),
        })
    }
}

fn load(path: &Path) -> Result<FileConfig, ConfigError> {
    let read_error = |source| ConfigError::Read {
        path: path.to_owned(),
        source,
    };
    let file = std::fs::File::open(path).map_err(read_error)?;
    let mut source = String::new();
    file.take(u64::try_from(MAX_BYTES + 1).unwrap_or(u64::MAX))
        .read_to_string(&mut source)
        .map_err(read_error)?;
    if source.len() > MAX_BYTES {
        return Err(ConfigError::TooLarge(path.to_owned()));
    }
    let mut config: FileConfig = toml::from_str(&source).map_err(|error: toml::de::Error| {
        // TOML's Display includes source excerpts and values. Only report location.
        let offset = error.span().map_or(0, |span| span.start);
        let prefix = source.get(..offset).unwrap_or_default();
        ConfigError::Invalid {
            path: path.to_owned(),
            line: prefix.bytes().filter(|byte| *byte == b'\n').count() + 1,
            column: prefix
                .rsplit('\n')
                .next()
                .map_or(1, |line| line.chars().count() + 1),
        }
    })?;
    config.validate()?;
    Ok(config)
}

impl FileConfig {
    fn validate(&mut self) -> Result<(), ConfigError> {
        self.staff
            .validate(None)
            .map_err(|_| ConfigError::InvalidAccess)?;
        if let Some(institution) = &self.institution {
            validate_name(&institution.name)?;
        }
        if self
            .server
            .data_dir
            .as_ref()
            .is_some_and(|dir| dir.as_os_str().is_empty())
        {
            return Err(ConfigError::EmptyDataDir);
        }
        Ok(())
    }
}

pub(crate) fn validate_name(name: &str) -> Result<(), ConfigError> {
    if name.trim().is_empty()
        || name.trim().chars().count() > 200
        || name.chars().any(char::is_control)
    {
        return Err(ConfigError::InvalidName);
    }
    Ok(())
}

/// Serializes the same schema that `serve --config` validates.
pub(crate) fn render_init(
    name: &str,
    data_dir: &Path,
    listen: SocketAddr,
    insecure_cookies: bool,
) -> Result<String, ConfigError> {
    validate_name(name)?;
    let mut config = FileConfig {
        institution: Some(Institution {
            name: name.trim().to_owned(),
        }),
        staff: crate::access_config::StaffMode::default(),
        server: Server {
            data_dir: Some(data_dir.to_owned()),
            listen,
            insecure_cookies,
            iri_base: None,
        },
    };
    config.validate()?;
    let source = toml::to_string_pretty(&config).map_err(|_| ConfigError::Serialize)?;
    if source.len() > MAX_BYTES {
        return Err(ConfigError::Serialize);
    }
    Ok(source)
}

/// Captures the effective server configuration, including command-line overrides.
pub(crate) fn snapshot(settings: &Settings) -> Result<String, ConfigError> {
    toml::to_string_pretty(&FileConfig {
        institution: settings
            .institution_name
            .as_ref()
            .map(|name| Institution { name: name.clone() }),
        staff: settings.staff.clone(),
        server: Server {
            data_dir: Some(settings.data_dir.clone()),
            listen: settings.listen,
            insecure_cookies: settings.insecure_cookies,
            iri_base: settings.iri_base.clone(),
        },
    })
    .map_err(|_| ConfigError::Serialize)
}

/// Relocates a validated backup without losing the institution's access policy.
pub(crate) fn relocate(source: &str, directory: &Path) -> Result<String, String> {
    let mut config: FileConfig =
        toml::from_str(source).map_err(|_| "invalid restored server configuration")?;
    config
        .validate()
        .map_err(|_| "invalid restored server configuration")?;
    config
        .staff
        .validate(Some(config.server.listen))
        .map_err(|_| "invalid restored access policy")?;
    config.server.data_dir = Some(directory.to_owned());
    toml::to_string_pretty(&config).map_err(|_| "cannot relocate server configuration".to_owned())
}

/// Sets the host boundary required by a public tunnel while retaining server settings.
pub(crate) fn with_hosts(source: &str, catalog: &str, staff: &str) -> Result<String, String> {
    let mut config: FileConfig =
        toml::from_str(source).map_err(|_| "invalid tunnel server configuration")?;
    config.staff = crate::access_config::StaffMode::Hostname {
        catalog_host: catalog.to_owned(),
        staff_host: staff.to_owned(),
    };
    config
        .validate()
        .map_err(|_| "invalid tunnel server configuration")?;
    config
        .staff
        .validate(Some(config.server.listen))
        .map_err(|_| "invalid tunnel host boundary")?;
    toml::to_string_pretty(&config)
        .map_err(|_| "cannot encode tunnel server configuration".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args() -> ServeArgs {
        ServeArgs {
            config: None,
            data_dir: Some("local-data".into()),
            listen: None,
            insecure_cookies: None,
            iri_base: None,
            lang: None,
        }
    }

    #[test]
    fn absent_options_keep_loopback_and_secure_defaults() {
        let settings = args().resolve().unwrap();
        assert_eq!(settings.listen, "127.0.0.1:8080".parse().unwrap());
        assert!(!settings.insecure_cookies);
        assert_eq!(settings.data_dir, Path::new("local-data"));
    }

    #[test]
    fn ipv6_and_partial_files_are_typed() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("elzevir.toml");
        std::fs::write(&path, "[server]\nlisten = '[::1]:8081'").unwrap();
        let settings = ServeArgs {
            config: Some(path.clone()),
            ..args()
        }
        .resolve()
        .unwrap();
        assert_eq!(settings.listen, "[::1]:8081".parse().unwrap());
        assert_eq!(settings.data_dir, Path::new("local-data"));
        assert!(!settings.insecure_cookies);
        assert!(matches!(
            ServeArgs {
                config: Some(path),
                data_dir: None,
                ..args()
            }
            .resolve(),
            Err(ConfigError::MissingDataDir)
        ));
    }

    #[test]
    fn empty_cli_directory_is_rejected() {
        assert!(matches!(
            ServeArgs {
                data_dir: Some(PathBuf::new()),
                ..args()
            }
            .resolve(),
            Err(ConfigError::EmptyDataDir)
        ));
    }
    #[test]
    fn iri_base_uses_explicit_cli_then_file_then_request_default() {
        assert!(args().resolve().unwrap().iri_base.is_none());
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("server.toml");
        std::fs::write(
            &path,
            "[server]\niri_base = 'https://catalog.example/bibframe'",
        )
        .unwrap();
        for (override_base, expected) in [
            (None, "https://catalog.example/bibframe"),
            (
                Some("https://other.example/bibframe"),
                "https://other.example/bibframe",
            ),
        ] {
            let settings = ServeArgs {
                config: Some(path.clone()),
                iri_base: override_base.map(str::to_owned),
                ..args()
            }
            .resolve()
            .unwrap();
            assert_eq!(settings.iri_base.as_deref(), Some(expected));
        }
    }
}
