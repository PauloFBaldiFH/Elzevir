// SPDX-License-Identifier: GPL-3.0-or-later
//! Crash recovery retains a fixed set of original paths under the original lock.

use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    path::{Path, PathBuf},
};
const NAME: &str = "elzevir-restore.json";
const FILES: &[&str] = &[
    "elzevir.db",
    "elzevir.db-wal",
    "elzevir.db-shm",
    "index",
    "elzevir.toml",
    "operations.toml",
    "author-table.txt",
    "secrets",
    "server-config",
];
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Journal {
    schema: u8,
    id: String,
    config: PathBuf,
    entries: Vec<Entry>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    name: String,
    existed: bool,
}

pub(crate) fn pending(directory: &Path) -> bool {
    std::fs::symlink_metadata(directory.join(NAME)).is_ok()
}

fn absolute(path: &Path) -> Result<PathBuf, String> {
    let parent = path
        .parent()
        .ok_or("configuration needs a parent")?
        .canonicalize()
        .map_err(|_| "configuration parent unavailable")?;
    Ok(parent.join(path.file_name().ok_or("configuration filename required")?))
}

impl Journal {
    pub(crate) fn begin(directory: &Path, safety: &Path, config: &Path) -> Result<Self, String> {
        let config = absolute(config)?;
        let id = safety
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_prefix("restore-safety-"))
            .ok_or("invalid restore safety path")?
            .to_owned();
        let mut journal = Self {
            schema: 1,
            id,
            config,
            entries: Vec::new(),
        };
        for name in FILES {
            let target = journal.target(directory, name)?;
            let existed = match std::fs::symlink_metadata(target) {
                Ok(metadata)
                    if !metadata.is_symlink() && (metadata.is_file() || metadata.is_dir()) =>
                {
                    true
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                _ => return Err("restore target must be regular and without symlinks".to_owned()),
            };
            journal.entries.push(Entry {
                name: (*name).to_owned(),
                existed,
            });
        }
        let bytes = serde_json::to_vec(&journal).map_err(|_| "cannot encode restore journal")?;
        crate::backup_archive::write_new(&safety.join("journal.json"), &bytes)?;
        crate::backup_archive::write_new(&directory.join(NAME), &bytes)?;
        sync(directory)?;
        Ok(journal)
    }
    fn target(&self, directory: &Path, name: &str) -> Result<PathBuf, String> {
        if !FILES.contains(&name) {
            return Err("invalid restore target".to_owned());
        }
        Ok(if name == "server-config" {
            self.config.clone()
        } else {
            directory.join(name)
        })
    }
    fn saved(&self, directory: &Path, name: &str) -> PathBuf {
        directory
            .join(format!("restore-safety-{}", self.id))
            .join(format!("original-{name}"))
    }
    pub(crate) fn preserve(&self, directory: &Path, target: &Path) -> Result<(), String> {
        let target = absolute(target)?;
        let entry = self
            .entries
            .iter()
            .find(|entry| {
                self.target(directory, &entry.name)
                    .is_ok_and(|path| path == target)
            })
            .ok_or("undeclared restore target")?;
        if entry.existed {
            std::fs::rename(&target, self.saved(directory, &entry.name))
                .map_err(|_| "cannot retain restore original")?;
            sync(directory)?;
            sync(&directory.join(format!("restore-safety-{}", self.id)))?;
        }
        Ok(())
    }
    pub(crate) fn rollback(&self, directory: &Path) -> Result<(), String> {
        // Remove a replacement database/index before restoring their original sidecars.
        for entry in &self.entries {
            let target = self.target(directory, &entry.name)?;
            let saved = self.saved(directory, &entry.name);
            if std::fs::symlink_metadata(&saved).is_ok() {
                remove(&target)?;
                std::fs::rename(saved, target).map_err(
                    |_| "restore recovery failed; original retained in safety directory",
                )?;
            } else if !entry.existed {
                remove(&target)?;
            }
        }
        Self::commit(directory)
    }
    pub(crate) fn commit(directory: &Path) -> Result<(), String> {
        std::fs::remove_file(directory.join(NAME))
            .map_err(|_| "cannot finalize restore journal")?;
        sync(directory)
    }
}

fn remove(path: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.is_symlink() => {
            std::fs::remove_dir_all(path)
        }
        Ok(_) => std::fs::remove_file(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
    .map_err(|_| "cannot clear restore replacement".to_owned())
}

pub(crate) fn recover(directory: &Path, config: Option<&Path>) -> Result<(), String> {
    let directory = directory
        .canonicalize()
        .map_err(|_| "restore directory unavailable")?;
    let _lock = crate::backup_cli::lock_existing(&directory)?;
    let path = directory.join(NAME);
    if crate::backup_archive::regular(&path)? > 16_384 {
        return Err("invalid restore journal".to_owned());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(16_385).read_to_end(&mut bytes))
        .map_err(|_| "restore journal unavailable")?;
    let journal: Journal = serde_json::from_slice(&bytes).map_err(|_| "invalid restore journal")?;
    if journal.schema != 1
        || journal.id.len() != 32
        || !journal.id.bytes().all(|byte| byte.is_ascii_hexdigit())
        || journal.entries.len() != FILES.len()
        || journal
            .entries
            .iter()
            .zip(FILES)
            .any(|(entry, name)| entry.name != *name)
        || journal.config
            != absolute(&config.map_or_else(|| directory.join("server.toml"), Path::to_owned))?
    {
        return Err("incompatible restore journal or configuration path".to_owned());
    }
    let safety = directory.join(format!("restore-safety-{}", journal.id));
    let metadata =
        std::fs::symlink_metadata(safety).map_err(|_| "restore safety directory unavailable")?;
    if !metadata.is_dir() || metadata.is_symlink() {
        return Err("invalid restore safety directory".to_owned());
    }
    journal.rollback(&directory)
}

#[cfg(unix)]
fn sync(directory: &Path) -> Result<(), String> {
    std::fs::File::open(directory)
        .and_then(|file| file.sync_all())
        .map_err(|_| "cannot sync restore directory".to_owned())
}
#[cfg(not(unix))]
fn sync(_directory: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn crash_between_file_moves_is_recoverable_without_an_available_live_database() {
        let directory = tempfile::tempdir().unwrap();
        let data = crate::open_store(directory.path()).unwrap();
        data.store.shutdown().unwrap();
        let crate::DataDir {
            _lock: lock_file, ..
        } = data;
        drop(lock_file);
        let original = std::fs::read(directory.path().join("elzevir.db")).unwrap();
        let safety = directory
            .path()
            .join(format!("restore-safety-{}", "a".repeat(32)));
        std::fs::create_dir(&safety).unwrap();
        let journal = Journal::begin(
            directory.path(),
            &safety,
            &directory.path().join("server.toml"),
        )
        .unwrap();
        journal
            .preserve(directory.path(), &directory.path().join("elzevir.db"))
            .unwrap();
        assert!(crate::open_store(directory.path()).is_err());
        assert!(!directory.path().join("elzevir.db").exists());
        recover(directory.path(), None).unwrap();
        assert_eq!(
            std::fs::read(directory.path().join("elzevir.db")).unwrap(),
            original
        );
        assert!(!pending(directory.path()));
    }
}
