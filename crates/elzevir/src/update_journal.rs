// SPDX-License-Identifier: GPL-3.0-or-later
//! Durable recovery information belongs to the operator, outside service-writable data.

use crate::{backup_archive as archive, installation};
use serde::{Deserialize, Serialize};
use std::{
    io::Read as _,
    path::{Path, PathBuf},
};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Phase {
    Prepared,
    BackedUp,
    Swapping,
    Starting,
    Recovering,
    Restored,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Backup {
    safety_id: String,
    sha256: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Journal {
    schema: u8,
    id: String,
    pub executable: PathBuf,
    pub directory: PathBuf,
    pub config: PathBuf,
    pub service: String,
    pub old_version: String,
    pub new_version: String,
    pub old_hash: String,
    pub new_hash: String,
    pub phase: Phase,
    backup: Option<Backup>,
}
fn digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
pub(crate) fn path(executable: &Path) -> Result<PathBuf, String> {
    let binding = installation::record_path(executable)?;
    let name = binding
        .file_name()
        .and_then(|value| value.to_str())
        .and_then(|value| value.strip_prefix(".elzevir-instance-"))
        .ok_or("invalid program record name")?;
    Ok(binding.with_file_name(format!(".elzevir-update-{name}")))
}
pub(crate) fn pending(executable: &Path) -> Result<bool, String> {
    Ok(std::fs::symlink_metadata(path(executable)?).is_ok())
}
impl Journal {
    pub(crate) fn begin(
        executable: &Path,
        directory: &Path,
        config: &Path,
        service: &str,
        previous: &Path,
        version: &str,
        new_hash: String,
    ) -> Result<Self, String> {
        installation::verify(executable, directory, config, service)?;
        if pending(executable)? {
            return Err("an interrupted update requires recovery before another update".to_owned());
        }
        let id = previous
            .file_stem()
            .and_then(|value| value.to_str())
            .and_then(|value| value.strip_prefix("elzevir-previous-"))
            .ok_or("invalid previous executable name")?
            .to_owned();
        let journal = Self {
            schema: 1,
            id,
            executable: installation::absolute(executable)?,
            directory: directory
                .canonicalize()
                .map_err(|_| "institution unavailable")?,
            config: installation::absolute(config)?,
            service: service.to_owned(),
            old_version: env!("CARGO_PKG_VERSION").to_owned(),
            new_version: version.trim_start_matches('v').to_owned(),
            old_hash: crate::release::hash(executable)?,
            new_hash,
            phase: Phase::Prepared,
            backup: None,
        };
        if installation::absolute(previous)? != journal.previous()? {
            return Err("previous executable must belong to this update".to_owned());
        }
        journal.save()?;
        Ok(journal)
    }
    fn validate(&self) -> Result<(), String> {
        if self.schema != 1
            || !crate::cloud::valid_id(&self.id)
            || !digest(&self.old_hash)
            || !digest(&self.new_hash)
            || !crate::release::valid_version(&format!("v{}", self.new_version))
            || self.old_version.is_empty()
            || self.old_version.len() > 64
            || self.old_version.chars().any(char::is_control)
            || self.backup.as_ref().is_some_and(|backup| {
                !crate::cloud::valid_id(&backup.safety_id) || !digest(&backup.sha256)
            })
        {
            return Err("invalid update recovery journal".to_owned());
        }
        installation::verify(
            &self.executable,
            &self.directory,
            &self.config,
            &self.service,
        )
    }
    fn save(&self) -> Result<(), String> {
        self.validate()?;
        let bytes =
            serde_json::to_vec(self).map_err(|_| "cannot encode update recovery journal")?;
        if bytes.len() > 16_384 {
            return Err("update recovery journal exceeds limit".to_owned());
        }
        installation::publish_private(&path(&self.executable)?, &bytes)
    }
    pub(crate) fn load(executable: &Path) -> Result<Self, String> {
        let journal: Self =
            serde_json::from_slice(&installation::read_private(&path(executable)?, 16_384)?)
                .map_err(|_| "invalid update recovery journal")?;
        if journal.executable != installation::absolute(executable)? {
            return Err("update journal belongs to another executable".to_owned());
        }
        journal.validate()?;
        Ok(journal)
    }
    pub(crate) fn previous(&self) -> Result<PathBuf, String> {
        Ok(self
            .executable
            .parent()
            .ok_or("program parent required")?
            .join(format!(
                "elzevir-previous-{}{}",
                self.id,
                if cfg!(windows) { ".exe" } else { "" }
            )))
    }
    pub(crate) fn phase(&mut self, phase: Phase) -> Result<(), String> {
        self.phase = phase;
        self.save()
    }
    pub(crate) fn protect_backup(&mut self, source: &Path) -> Result<(), String> {
        let maximum = 1024 * 1024 * 1024 * 1024_u64;
        if archive::regular(source)? > maximum {
            return Err("update backup exceeds limit".to_owned());
        }
        let source_hash = crate::release::hash(source)?;
        let safety = crate::restore_journal::create_safety(&self.directory)?;
        let id = safety
            .file_name()
            .and_then(|value| value.to_str())
            .and_then(|value| value.rsplit('-').next())
            .ok_or("invalid private update backup")?
            .to_owned();
        let target = safety.join("update.age");
        let mut input = std::fs::File::open(source)
            .map_err(|_| "update backup unavailable")?
            .take(maximum + 1);
        archive::write_new(&target, &[])?;
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .open(&target)
            .map_err(|_| "cannot stage update backup")?;
        let count =
            std::io::copy(&mut input, &mut output).map_err(|_| "cannot stage update backup")?;
        output.sync_all().map_err(|_| "cannot sync update backup")?;
        if count > maximum || crate::release::hash(&target)? != source_hash {
            return Err("update backup changed while staging".to_owned());
        }
        self.backup = Some(Backup {
            safety_id: id,
            sha256: source_hash,
        });
        self.phase(Phase::BackedUp)
    }
    pub(crate) fn has_backup(&self) -> bool {
        self.backup.is_some()
    }
    pub(crate) fn backup(&self) -> Result<PathBuf, String> {
        let backup = self
            .backup
            .as_ref()
            .ok_or("update has no final cold backup")?;
        let path = crate::restore_journal::safety_path(&self.directory, &backup.safety_id)?
            .join("update.age");
        if crate::release::hash(&path)? != backup.sha256 {
            return Err("private update backup checksum mismatch".to_owned());
        }
        Ok(path)
    }
    pub(crate) fn rollback_binary(&self) -> Result<(), String> {
        if self.executable.exists() {
            let found = crate::release::hash(&self.executable)?;
            if found == self.old_hash {
                return Ok(());
            }
            if found != self.new_hash {
                return Err(
                    "installed executable differs from both update versions; it was preserved"
                        .to_owned(),
                );
            }
        }
        let previous = self.previous()?;
        if crate::release::hash(&previous)? != self.old_hash {
            return Err("previous executable checksum mismatch".to_owned());
        }
        if self.executable.exists() {
            let failed = self.executable.with_file_name(format!(
                "elzevir-failed-{}{}",
                self.id,
                if cfg!(windows) { ".exe" } else { "" }
            ));
            if std::fs::symlink_metadata(&failed).is_ok() {
                return Err("failed-version destination must be new".to_owned());
            }
            std::fs::rename(&self.executable, failed)
                .map_err(|_| "cannot retain failed update executable")?;
        }
        std::fs::rename(previous, &self.executable)
            .map_err(|_| "cannot recover previous executable")?;
        installation::sync_directory(self.executable.parent().ok_or("program parent required")?)
    }
    pub(crate) fn finish(&self) -> Result<(), String> {
        std::fs::remove_file(path(&self.executable)?)
            .map_err(|_| "cannot finalize update recovery journal")?;
        installation::sync_directory(self.executable.parent().ok_or("program parent required")?)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn previous_executable_accepts_a_parent_alias_but_rejects_another_directory() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("library");
        let alias = root.path().join("alias");
        let outside = root.path().join("outside");
        for path in [&directory, &alias, &outside] {
            std::fs::create_dir(path).unwrap();
        }
        let config = root.path().join("server.toml");
        let executable = root.path().join("elzevir");
        std::fs::write(&config, b"fictional config").unwrap();
        std::fs::write(&executable, b"old executable").unwrap();
        installation::claim(&executable, &directory, &config, "elzevir").unwrap();
        let name = format!(
            "elzevir-previous-{}{}",
            "a".repeat(32),
            if cfg!(windows) { ".exe" } else { "" }
        );
        assert!(
            Journal::begin(
                &executable,
                &directory,
                &config,
                "elzevir",
                &outside.join(&name),
                "v1.0.0",
                "b".repeat(64)
            )
            .unwrap_err()
            .contains("must belong")
        );
        assert!(!pending(&executable).unwrap());
        let previous = alias.join("..").join(&name);
        let journal = Journal::begin(
            &executable,
            &directory,
            &config,
            "elzevir",
            &previous,
            "v1.0.0",
            "b".repeat(64),
        )
        .unwrap();
        assert_eq!(
            journal.previous().unwrap(),
            root.path().canonicalize().unwrap().join(name)
        );
    }

    #[test]
    fn interruption_between_binary_renames_recovers_the_owned_previous_file() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("library");
        std::fs::create_dir(&directory).unwrap();
        let config = root.path().join("server.toml");
        std::fs::write(&config, b"fictional config").unwrap();
        let executable = root.path().join(if cfg!(windows) {
            "elzevir.exe"
        } else {
            "elzevir"
        });
        std::fs::write(&executable, b"old executable").unwrap();
        installation::claim(&executable, &directory, &config, "elzevir").unwrap();
        let previous = root.path().join(format!(
            "elzevir-previous-{}{}",
            "a".repeat(32),
            if cfg!(windows) { ".exe" } else { "" }
        ));
        let new_file = root.path().join("new");
        std::fs::write(&new_file, b"new executable").unwrap();
        let mut journal = Journal::begin(
            &executable,
            &directory,
            &config,
            "elzevir",
            &previous,
            "v1.0.0",
            crate::release::hash(&new_file).unwrap(),
        )
        .unwrap();
        let age = directory.join(format!("elzevir-{}.age", "b".repeat(32)));
        std::fs::write(&age, b"fictional encrypted backup").unwrap();
        journal.protect_backup(&age).unwrap();
        journal.phase(Phase::Swapping).unwrap();
        std::fs::rename(&executable, &previous).unwrap();
        let recovered = Journal::load(&executable).unwrap();
        recovered.rollback_binary().unwrap();
        assert_eq!(std::fs::read(&executable).unwrap(), b"old executable");
        assert!(recovered.backup().unwrap().is_file());
        recovered.finish().unwrap();
        assert!(!pending(&executable).unwrap());
    }
}
