// SPDX-License-Identifier: GPL-3.0-or-later
//! Private maintenance index, independent of the catalog being restored.
use crate::StoreError;
use elzevir_app::repository::{ObjectHash, ObjectInfo};
use rusqlite::{Connection, OpenFlags, params};
use std::path::Path;

/// Whether the verified snapshot includes retained repository versions.
///
/// # Errors
/// The snapshot cannot be inspected.
pub fn snapshot_contains_objects(snapshot: &Path) -> Result<bool, StoreError> {
    let source = crate::schema::open_reader(snapshot)?;
    let available: bool = source.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='repo_versions')",
        [],
        |row| row.get(0),
    )?;
    if !available {
        return Ok(false);
    }
    Ok(
        source.query_row("SELECT EXISTS(SELECT 1 FROM repo_versions)", [], |row| {
            row.get(0)
        })?,
    )
}

/// Unique objects referenced by any version in an immutable database snapshot,
/// in digest order. Enforces the archive writer's manifest budget before allocation grows.
///
/// # Errors
/// Fails on a broken reference, invalid size, unreadable schema or excessive inventory.
pub fn snapshot_objects(snapshot: &Path, limit: usize) -> Result<Vec<ObjectInfo>, StoreError> {
    let source = crate::schema::open_reader(snapshot)?;
    let version: i64 = source.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version < 62 {
        return Ok(Vec::new());
    }
    let mut query = source.prepare(
        "SELECT v.object_hash,o.bytes
        FROM (SELECT DISTINCT object_hash FROM repo_versions) v
        LEFT JOIN repo_objects o ON o.sha256=v.object_hash ORDER BY v.object_hash",
    )?;
    let mut rows = query.query([])?;
    let mut objects = Vec::new();
    while let Some(row) = rows.next()? {
        if objects.len() >= limit {
            return Err(StoreError::Backup(
                "repository manifest exceeds object limit".into(),
            ));
        }
        let bytes = u64::try_from(row.get::<_, i64>(1)?)
            .map_err(|_| StoreError::Backup("negative repository object size".into()))?;
        if bytes == 0 {
            return Err(StoreError::Backup("invalid repository object size".into()));
        }
        objects.push(ObjectInfo {
            hash: ObjectHash(row.get(0)?),
            bytes,
        });
    }
    Ok(objects)
}

/// Persist the exact set of hashes retained by a verified SQLite snapshot before
/// publishing its archive. The catalog and its restore never overwrite this index.
///
/// # Errors
/// An unreadable snapshot or a failed index transaction prevents backup publication.
pub fn pin_backup(snapshot: &Path, index: &Path, id: &str, local: bool) -> Result<(), StoreError> {
    let source = crate::schema::open_reader(snapshot)?;
    let mut hashes =
        source.prepare("SELECT DISTINCT object_hash FROM repo_versions ORDER BY object_hash")?;
    let mut rows = hashes.query([])?;
    let mut index = Connection::open(index)?;
    index.execute_batch(
        "PRAGMA synchronous=FULL;
        CREATE TABLE IF NOT EXISTS pins(
          hash BLOB NOT NULL CHECK(length(hash)=32), backup TEXT NOT NULL, local INTEGER NOT NULL,
          PRIMARY KEY(hash,backup)
        ) STRICT, WITHOUT ROWID;
        CREATE INDEX IF NOT EXISTS pins_by_backup ON pins(local,backup);",
    )?;
    let tx = index.transaction()?;
    {
        let mut pin =
            tx.prepare("INSERT OR IGNORE INTO pins(hash,backup,local) VALUES(?1,?2,?3)")?;
        while let Some(row) = rows.next()? {
            pin.execute(params![row.get::<_, [u8; 32]>(0)?, id, local])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Private index scoped to one exclusive maintenance session.
pub struct BackupPins(Connection);
impl BackupPins {
    /// # Errors
    /// Cannot read the private index.
    pub fn open(path: &Path, apply: bool) -> Result<Self, StoreError> {
        Ok(Self(Connection::open_with_flags(
            path,
            if apply {
                OpenFlags::SQLITE_OPEN_READ_WRITE
            } else {
                OpenFlags::SQLITE_OPEN_READ_ONLY
            },
        )?))
    }
    /// Forget expired archive references after their last managed file disappeared.
    ///
    /// # Errors
    /// The index is read-only or the deletion failed.
    pub fn release_expired(&self, hash: ObjectHash) -> Result<(), StoreError> {
        self.0.execute("DELETE FROM pins WHERE hash=?1", [hash.0])?;
        Ok(())
    }
    /// Managed archives recorded in the private index.
    ///
    /// # Errors
    /// The index cannot be read.
    pub fn local_archives(&self) -> Result<Vec<String>, StoreError> {
        let mut stmt = self
            .0
            .prepare("SELECT DISTINCT backup FROM pins WHERE local=1")?;
        Ok(stmt
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }
    /// Release an expired local archive after a complete inventory pass.
    ///
    /// # Errors
    /// The index is read-only or the deletion failed.
    pub fn forget_archive(&self, id: &str) -> Result<(), StoreError> {
        self.0
            .execute("DELETE FROM pins WHERE local=1 AND backup=?1", [id])?;
        Ok(())
    }
    /// Snapshot IDs retaining the hash and whether their archive is managed locally.
    ///
    /// # Errors
    /// Unreadable or malformed index.
    pub fn archives(&self, hash: ObjectHash) -> Result<Vec<(String, bool)>, StoreError> {
        let mut stmt = self
            .0
            .prepare("SELECT backup,local FROM pins WHERE hash=?1")?;
        let rows = stmt.query_map([hash.0], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}
