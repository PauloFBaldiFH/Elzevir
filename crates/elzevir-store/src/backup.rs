// SPDX-License-Identifier: GPL-3.0-or-later
//! Consistent images from read-only sources; destinations are new standalone files.

use crate::{ReadPool, StoreError, schema};
use rusqlite::{
    Connection, OpenFlags,
    backup::{Backup, StepResult},
};
use std::{
    path::Path,
    time::{Duration, Instant},
};

/// Verified properties of a standalone Elzevir database image.
#[derive(Debug, Clone, Copy)]
pub struct SnapshotInfo {
    /// Schema version stored in the image.
    pub schema_version: i64,
    /// Image size in bytes.
    pub bytes: u64,
}

fn failure(message: &str) -> StoreError {
    StoreError::Backup(message.to_owned())
}

/// Newest database schema this binary can restore.
#[must_use]
pub fn supported_schema_version() -> i64 {
    schema::latest_version()
}

/// Verifies integrity and supported schema without opening a writable connection.
///
/// # Errors
/// Refuses links, non-files, corrupt/foreign images and newer schemas.
pub fn inspect_snapshot(path: &Path) -> Result<SnapshotInfo, StoreError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| failure("image unavailable"))?;
    if !metadata.is_file() || metadata.is_symlink() {
        return Err(failure("image must be a regular file"));
    }
    let connection = schema::open_reader(path)?;
    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version > supported_schema_version() {
        return Err(StoreError::SchemaTooNew {
            found: version,
            supported: supported_schema_version(),
        });
    }
    let tables: bool = connection.query_row("SELECT COUNT(*) = 2 FROM sqlite_schema WHERE type='table' AND name IN ('audit_log','effect_outbox')", [], |row| row.get(0))?;
    if version < 1 || !tables {
        return Err(failure("image is not an Elzevir database"));
    }
    let result: String = connection.query_row("PRAGMA integrity_check(1)", [], |row| row.get(0))?;
    if result != "ok" {
        return Err(failure("image integrity failed"));
    }
    Ok(SnapshotInfo {
        schema_version: version,
        bytes: metadata.len(),
    })
}

fn snapshot(connection: &Connection, path: &Path) -> Result<SnapshotInfo, StoreError> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    drop(
        options
            .open(path)
            .map_err(|_| failure("snapshot destination must be new"))?,
    );
    let result = (|| {
        let mut destination = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        {
            let backup = Backup::new(connection, &mut destination)?;
            let started = Instant::now();
            loop {
                if started.elapsed() > Duration::from_mins(15) {
                    return Err(failure("snapshot deadline exceeded"));
                }
                match backup.step(128)? {
                    StepResult::Done => break,
                    StepResult::More => {}
                    StepResult::Busy | StepResult::Locked => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    _ => return Err(failure("unsupported snapshot state")),
                }
            }
        }
        // The artifact must not need a WAL or SHM file to be verified/restored.
        destination.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL;")?;
        drop(destination);
        inspect_snapshot(path)
    })();
    if result.is_err() {
        // Only the file exclusively created above belongs to this operation.
        if std::fs::remove_file(path).is_err() {
            return Err(failure("snapshot failed and cleanup failed"));
        }
    }
    result
}

impl ReadPool {
    /// Copies a live WAL database through a pooled reader into a new standalone file.
    ///
    /// # Errors
    /// Returns SQLite, integrity, deadline or exclusive-destination failures.
    pub fn snapshot(&self, path: &Path) -> Result<SnapshotInfo, StoreError> {
        self.read(|connection| snapshot(connection, path))
    }
}

/// Makes a cold snapshot while the caller holds the institution lock.
///
/// # Errors
/// Refuses missing/invalid sources and existing destinations; never migrates the source.
pub fn snapshot_offline(source: &Path, destination: &Path) -> Result<SnapshotInfo, StoreError> {
    let metadata = std::fs::symlink_metadata(source).map_err(|_| failure("source unavailable"))?;
    if !metadata.is_file() || metadata.is_symlink() {
        return Err(failure("source must be a regular file"));
    }
    let connection = schema::open_reader(source)?;
    snapshot(&connection, destination)
}
