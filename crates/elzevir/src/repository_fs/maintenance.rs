// SPDX-License-Identifier: GPL-3.0-or-later
use super::{Error, FsObjectStore, ObjectHash, Result, failure, shards, sync_dir};
use elzevir_store::repository::{
    MetadataStore,
    backup::{self, BackupPins},
};
use std::{
    fs,
    io::{self, Read, Write},
    path::Path,
    sync::Arc,
    time::UNIX_EPOCH,
};

const DAY: i64 = 86_400;
#[derive(Debug, Default)]
pub(crate) struct Report {
    pub(crate) observed: u64,
    pub(crate) temporary: u64,
    pub(crate) orphan: u64,
    pub(crate) bytes: u64,
}

fn shard_dir(entry: &fs::DirEntry) -> Result<String> {
    let name = entry
        .file_name()
        .into_string()
        .map_err(|_| Error::InvalidState)?;
    if name.len() != 2
        || !name
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || !entry.file_type().map_err(failure)?.is_dir()
    {
        return Err(Error::InvalidState);
    }
    Ok(name)
}
fn list(path: &Path) -> Result<impl Iterator<Item = Result<fs::DirEntry>>> {
    Ok(fs::read_dir(path)
        .map_err(failure)?
        .map(|entry| entry.map_err(failure)))
}
impl FsObjectStore {
    fn observation(&self, hash: ObjectHash) -> Result<Option<i64>> {
        let path = self.inventory.join(hash.to_string());
        let meta = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(failure(e)),
        };
        if !meta.is_file() || meta.len() != 8 {
            return Err(Error::InvalidState);
        }
        let mut bytes = [0; 8];
        fs::File::open(path)
            .and_then(|mut f| f.read_exact(&mut bytes))
            .map_err(failure)?;
        Ok(Some(i64::from_be_bytes(bytes)))
    }
    fn observe(&self, hash: ObjectHash, now: i64) -> Result<()> {
        let mut tmp = tempfile::Builder::new()
            .prefix("upload-")
            .tempfile_in(&self.staging)
            .map_err(failure)?;
        tmp.write_all(&now.to_be_bytes()).map_err(failure)?;
        tmp.as_file().sync_all().map_err(failure)?;
        tmp.persist_noclobber(self.inventory.join(hash.to_string()))
            .map_err(|e| failure(e.error))?;
        sync_dir(&self.inventory)
    }
    fn collect_temporaries(&self, now: i64, apply: bool, report: &mut Report) -> Result<()> {
        for entry in list(&self.staging)? {
            let entry = entry?;
            if !entry.file_name().to_string_lossy().starts_with("upload-")
                || !entry.file_type().map_err(failure)?.is_file()
            {
                continue;
            }
            let meta = entry.metadata().map_err(failure)?;
            let age = meta
                .modified()
                .map_err(failure)?
                .duration_since(UNIX_EPOCH)
                .map_err(|_| Error::InvalidState)?;
            let modified = i64::try_from(age.as_secs()).map_err(|_| Error::InvalidState)?;
            if now.saturating_sub(modified) >= DAY {
                report.temporary += 1;
                report.bytes += meta.len();
                if apply {
                    fs::remove_file(entry.path()).map_err(failure)?;
                }
            }
        }
        if apply && report.temporary > 0 {
            sync_dir(&self.staging)?;
        }
        Ok(())
    }
    pub(crate) fn maintain(
        &mut self,
        metadata: &MetadataStore,
        now: i64,
        apply: bool,
    ) -> Result<Report> {
        // &mut excludes new transfers. Detached readers/installations retain their own lease.
        if Arc::get_mut(&mut self.session).is_none() {
            return Err(Error::UploadBusy);
        }
        let index = self.inventory.join(PINS);
        let pins = if regular_if_present(&index)? {
            Some(BackupPins::open(&index, apply).map_err(|_| Error::Unavailable)?)
        } else {
            None
        };
        let mut report = Report::default();
        self.collect_temporaries(now, apply, &mut report)?;
        for first in list(&self.objects)? {
            let first = first?;
            let a = shard_dir(&first)?;
            for second in list(&first.path())? {
                let second = second?;
                let b = shard_dir(&second)?;
                for entry in list(&second.path())? {
                    let entry = entry?;
                    let hash: ObjectHash = entry
                        .file_name()
                        .to_str()
                        .ok_or(Error::InvalidState)?
                        .parse()?;
                    let (x, y) = shards(hash);
                    if x != a || y != b || !entry.file_type().map_err(failure)?.is_file() {
                        return Err(Error::InvalidState);
                    }
                    if metadata.retains(hash)? {
                        if apply {
                            self.forget_observation(hash)?;
                        }
                        continue;
                    }
                    let first_seen = if let Some(pins) = &pins {
                        match self.backup_observation(pins, hash, apply)? {
                            BackupObservation::Retained => continue,
                            BackupObservation::Expired => None,
                            BackupObservation::Absent => self.observation(hash)?,
                        }
                    } else {
                        self.observation(hash)?
                    };
                    match first_seen {
                        None => {
                            report.observed += 1;
                            if apply {
                                self.observe(hash, now)?;
                            }
                        }
                        Some(when) if now.saturating_sub(when) >= 7 * DAY => {
                            report.orphan += 1;
                            report.bytes += entry.metadata().map_err(failure)?.len();
                            if apply {
                                // Clear first. A crash can extend grace but cannot shorten it.
                                self.forget_observation(hash)?;
                                fs::remove_file(entry.path()).map_err(failure)?;
                                sync_dir(&second.path())?;
                            }
                        }
                        Some(_) => {}
                    }
                }
            }
        }
        // All observations now reflect the current backup set. Retention can
        // release obsolete rows even for objects still held by the catalog.
        if apply && let Some(pins) = pins {
            for id in pins.local_archives().map_err(|_| Error::Unavailable)? {
                let archive = crate::backup_cli::artifact(&self.data, &id)
                    .map_err(|_| Error::InvalidState)?;
                if !regular_if_present(&archive)? {
                    pins.forget_archive(&id).map_err(|_| Error::Unavailable)?;
                }
            }
        }
        Ok(report)
    }
}

const PINS: &str = "backup-pins.sqlite";

fn regular_if_present(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() => Ok(true),
        Ok(_) => Err(Error::Unavailable),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(failure(e)),
    }
}

/// Called under the server or offline backup lease, against the exact image being published.
pub(crate) fn pin_snapshot(
    data: &Path,
    snapshot: &Path,
    archive: &Path,
    id: &str,
) -> std::result::Result<(), String> {
    if !backup::snapshot_contains_objects(snapshot).map_err(|e| e.to_string())? {
        return Ok(());
    }
    let repository = super::directory(data, "repository").map_err(|e| e.to_string())?;
    let directory = super::directory(&repository, "maintenance").map_err(|e| e.to_string())?;
    let index = directory.join(PINS);
    regular_if_present(&index).map_err(|e| e.to_string())?;
    let managed = crate::backup_cli::artifact(data, id)?;
    let local = archive.file_name() == managed.file_name()
        && archive.parent().and_then(|p| p.canonicalize().ok()) == Some(data.join("backups"));
    backup::pin_backup(snapshot, &index, id, local).map_err(|e| e.to_string())?;
    sync_dir(&directory).map_err(|e| e.to_string())
}
enum BackupObservation {
    Retained,
    Expired,
    Absent,
}

impl FsObjectStore {
    fn backup_observation(
        &self,
        pins: &BackupPins,
        hash: ObjectHash,
        apply: bool,
    ) -> Result<BackupObservation> {
        let archives = pins.archives(hash).map_err(|_| Error::Unavailable)?;
        if archives.is_empty() {
            return Ok(BackupObservation::Absent);
        }
        for (id, local) in archives {
            if !local
                || regular_if_present(
                    &crate::backup_cli::artifact(&self.data, &id)
                        .map_err(|_| Error::InvalidState)?,
                )?
            {
                if apply {
                    self.forget_observation(hash)?;
                }
                return Ok(BackupObservation::Retained);
            }
        }
        if apply {
            // A pre-backup observation cannot age out an object just released by retention.
            self.forget_observation(hash)?;
            pins.release_expired(hash).map_err(|_| Error::Unavailable)?;
        }
        Ok(BackupObservation::Expired)
    }
}
