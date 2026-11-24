// SPDX-License-Identifier: GPL-3.0-or-later
//! Test adapters with independent metadata and byte durability boundaries.
use super::*;
use crate::{search::Effect, staff::StaffId};
use sha2::{Digest, Sha256};
use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Read},
    sync::{Arc, Mutex},
};

#[derive(Default)]
pub(super) struct TestClock(pub Cell<i64>);
impl Clock for TestClock {
    fn now(&self) -> i64 {
        self.0.get()
    }
}

#[derive(Clone, Default)]
struct Rows {
    instances: BTreeMap<InstanceId, bool>,
    collections: BTreeSet<DigitalCollectionId>,
    deposits: BTreeMap<DepositId, Deposit>,
    operations: BTreeMap<OperationId, UploadReceipt>,
    retired: BTreeMap<VersionRef, i64>,
    audit: Vec<(StaffId, i64)>,
    outbox: Vec<Effect>,
    downloads: Vec<(StaffId, i64, VersionRef)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Crash {
    BeforeCommit,
    AfterCommit,
}

#[derive(Default)]
pub(super) struct MemoryMetadata {
    rows: Mutex<Rows>,
    crash: Mutex<Option<Crash>>,
}

impl MemoryMetadata {
    pub(super) fn instance(&self, id: InstanceId, visible: bool) {
        self.rows.lock().unwrap().instances.insert(id, visible);
    }
    pub(super) fn collection(&self, id: DigitalCollectionId) {
        self.rows.lock().unwrap().collections.insert(id);
    }
    pub(super) fn crash(&self, crash: Crash) {
        *self.crash.lock().unwrap() = Some(crash);
    }
    pub(super) fn counts(&self) -> (usize, usize, usize) {
        let rows = self.rows.lock().unwrap();
        (rows.operations.len(), rows.audit.len(), rows.outbox.len())
    }
    /// A private copy represents a writer savepoint. Only the final assignment is durable.
    fn transaction<T>(&self, work: impl FnOnce(&mut Rows) -> Result<T>) -> Result<T> {
        let mut rows = self.rows.lock().unwrap();
        let mut candidate = rows.clone();
        let result = work(&mut candidate)?;
        let crash = self.crash.lock().unwrap().take();
        if crash == Some(Crash::BeforeCommit) {
            return Err(RepositoryError::Unavailable);
        }
        *rows = candidate;
        if crash == Some(Crash::AfterCommit) {
            return Err(RepositoryError::CommitUnknown);
        }
        Ok(result)
    }
}
impl RepositoryRead for MemoryMetadata {
    fn snapshot(&self, id: DepositId) -> Result<(Deposit, bool)> {
        let rows = self.rows.lock().unwrap();
        let deposit = rows.deposits.get(&id).ok_or(RepositoryError::NotVisible)?;
        Ok((deposit.clone(), rows.instances[&deposit.instance]))
    }
    fn receipt(&self, version: VersionRef) -> Result<UploadReceipt> {
        self.rows
            .lock()
            .unwrap()
            .operations
            .values()
            .find(|r| r.version == version)
            .cloned()
            .ok_or(RepositoryError::NotVisible)
    }
    fn references(&self, hash: ObjectHash) -> Result<Vec<VersionRef>> {
        Ok(self
            .rows
            .lock()
            .unwrap()
            .operations
            .values()
            .filter(|r| r.object.hash == hash)
            .map(|r| r.version)
            .collect())
    }
    fn ensure_deletable(&self, instance: InstanceId) -> Result<()> {
        let rows = self.rows.lock().unwrap();
        if !rows.instances.contains_key(&instance) {
            return Err(RepositoryError::InstanceNotFound);
        }
        if rows.deposits.values().any(|d| d.instance == instance) {
            return Err(RepositoryError::InstanceHasDeposit);
        }
        Ok(())
    }
}
impl RepositoryWrite for MemoryMetadata {
    fn create_draft(
        &self,
        actor: Operator,
        at: i64,
        id: DepositId,
        instance: InstanceId,
    ) -> Result<()> {
        self.transaction(|rows| {
            if !rows.instances.contains_key(&instance) {
                return Err(RepositoryError::InstanceNotFound);
            }
            if rows.deposits.contains_key(&id)
                || rows.deposits.values().any(|d| d.instance == instance)
            {
                return Err(RepositoryError::DepositAlreadyExists);
            }
            rows.deposits.insert(id, Deposit::draft(id, instance));
            rows.audit.push((actor.id(), at));
            Ok(())
        })
    }
    fn edit(&self, actor: Operator, at: i64, id: DepositId, edit: Edit) -> Result<()> {
        self.transaction(|rows| {
            if let Edit::Join(collection) = &edit
                && !rows.collections.contains(collection)
            {
                return Err(RepositoryError::NotVisible);
            }
            if let Edit::AddFile(file) = &edit
                && rows
                    .deposits
                    .values()
                    .any(|d| d.files.contains_key(&file.id))
            {
                return Err(RepositoryError::InvalidState);
            }
            let deposit = rows
                .deposits
                .get_mut(&id)
                .ok_or(RepositoryError::NotVisible)?;
            if deposit.apply(&edit, rows.instances[&deposit.instance])? {
                rows.outbox.push(Effect::ReindexInstance(deposit.instance));
            }
            if let Edit::Retire(version, _) = edit {
                rows.retired.entry(version).or_insert(at);
            }
            rows.audit.push((actor.id(), at));
            Ok(())
        })
    }
    fn audit_download(&self, actor: Operator, at: i64, version: VersionRef) -> Result<()> {
        self.transaction(|rows| {
            rows.downloads.push((actor.id(), at, version));
            Ok(())
        })
    }
    fn commit_version(
        &self,
        actor: Operator,
        at: i64,
        upload: Upload,
        object: ObjectInfo,
    ) -> Result<UploadReceipt> {
        self.transaction(|rows| {
            if let Some(r) = rows.operations.get(&upload.operation) {
                return r.replay(&upload, object, actor);
            }
            let deposit = rows
                .deposits
                .get_mut(&upload.deposit)
                .ok_or(RepositoryError::NotVisible)?;
            let receipt = deposit.attach(upload, object, actor, at)?;
            if rows.instances[&deposit.instance]
                && deposit.state == DepositState::Published
                && deposit.files[&receipt.version.file].description.access == FileAccess::Public
            {
                rows.outbox.push(Effect::ReindexInstance(deposit.instance));
            }
            rows.operations
                .insert(receipt.upload.operation, receipt.clone());
            rows.audit.push((actor.id(), at));
            Ok(receipt)
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Activity {
    Read,
    Upload,
    Backup,
}

#[derive(Default)]
struct Bytes {
    objects: BTreeMap<ObjectHash, Arc<[u8]>>,
    abandoned: Vec<(i64, Vec<u8>)>,
    first_orphan: BTreeMap<ObjectHash, i64>,
    active: usize,
    uploading: bool,
}

#[derive(Default)]
pub(super) struct MemoryObjects {
    bytes: Arc<Mutex<Bytes>>,
    pub max_bytes: Cell<u64>,
    pub no_space: Cell<bool>,
}

pub(super) struct Guard {
    bytes: Arc<Mutex<Bytes>>,
    activity: Activity,
}
impl Drop for Guard {
    fn drop(&mut self) {
        let mut bytes = self.bytes.lock().unwrap();
        bytes.active -= 1;
        if self.activity == Activity::Upload {
            bytes.uploading = false;
        }
    }
}

pub(super) struct Installed {
    _guard: Guard,
    info: ObjectInfo,
}
impl InstalledObject for Installed {
    fn info(&self) -> ObjectInfo {
        self.info
    }
}
struct PinnedReader {
    _guard: Guard,
    cursor: Cursor<Arc<[u8]>>,
}
impl Read for PinnedReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.cursor.read(buf)
    }
}

impl MemoryObjects {
    pub(super) fn hold(&self, activity: Activity) -> Result<Guard> {
        let mut bytes = self.bytes.lock().unwrap();
        if activity == Activity::Upload {
            if bytes.uploading {
                return Err(RepositoryError::UploadBusy);
            }
            bytes.uploading = true;
        }
        bytes.active += 1;
        Ok(Guard {
            bytes: Arc::clone(&self.bytes),
            activity,
        })
    }
    pub(super) fn remove(&self, hash: ObjectHash) {
        self.bytes.lock().unwrap().objects.remove(&hash);
    }
    pub(super) fn corrupt(&self, hash: ObjectHash) {
        self.bytes
            .lock()
            .unwrap()
            .objects
            .insert(hash, Arc::from(b"damaged".as_slice()));
    }
    pub(super) fn counts(&self) -> (usize, usize) {
        let bytes = self.bytes.lock().unwrap();
        (bytes.abandoned.len(), bytes.objects.len())
    }
    pub(super) fn collect(&self, now: i64, metadata: &impl RepositoryRead) -> Result<()> {
        let mut bytes = self.bytes.lock().unwrap();
        if bytes.active != 0 {
            return Err(RepositoryError::UploadBusy);
        }
        bytes
            .abandoned
            .retain(|(last, _)| now.saturating_sub(*last) < 86_400);
        let hashes: Vec<_> = bytes.objects.keys().copied().collect();
        for hash in hashes {
            if metadata.references(hash)?.is_empty() {
                let first = *bytes.first_orphan.entry(hash).or_insert(now);
                if now.saturating_sub(first) >= 7 * 86_400 {
                    bytes.objects.remove(&hash);
                    bytes.first_orphan.remove(&hash);
                }
            } else {
                bytes.first_orphan.remove(&hash);
            }
        }
        Ok(())
    }
}

fn digest(bytes: &[u8]) -> ObjectHash {
    ObjectHash(Sha256::digest(bytes).into())
}
fn health(bytes: Option<&Arc<[u8]>>, object: ObjectInfo) -> Integrity {
    match bytes {
        None => Integrity::Missing,
        Some(b) if b.len() as u64 == object.bytes && digest(b) == object.hash => Integrity::Ok,
        Some(_) => Integrity::Corrupt,
    }
}
impl ObjectStore for MemoryObjects {
    type Installed = Installed;
    fn install(
        &self,
        source: &mut dyn Read,
        expected: Option<ObjectHash>,
        now: i64,
    ) -> Result<Installed> {
        let guard = self.hold(Activity::Upload)?;
        if self.no_space.get() {
            return Err(RepositoryError::InsufficientSpace);
        }
        let mut buffer = Vec::new();
        let limit = self.max_bytes.get();
        if source
            .take(limit.saturating_add(1))
            .read_to_end(&mut buffer)
            .is_err()
        {
            self.bytes.lock().unwrap().abandoned.push((now, buffer));
            return Err(RepositoryError::Unavailable);
        }
        let bytes = buffer.len() as u64;
        if bytes > limit {
            return Err(RepositoryError::TooLarge);
        }
        if bytes == 0 {
            return Err(RepositoryError::InvalidState);
        }
        let hash = digest(&buffer);
        if expected.is_some_and(|h| h != hash) {
            return Err(RepositoryError::ChecksumMismatch);
        }
        let info = ObjectInfo { hash, bytes };
        let mut state = self.bytes.lock().unwrap();
        match health(state.objects.get(&hash), info) {
            Integrity::Missing => {
                state.objects.insert(hash, Arc::from(buffer));
            }
            Integrity::Corrupt => return Err(RepositoryError::ObjectUnavailable),
            Integrity::Ok => {}
        }
        Ok(Installed {
            _guard: guard,
            info,
        })
    }
    fn integrity(&self, object: ObjectInfo) -> Integrity {
        health(self.bytes.lock().unwrap().objects.get(&object.hash), object)
    }
    fn open(&self, object: ObjectInfo) -> Result<Box<dyn Read + Send>> {
        let guard = self.hold(Activity::Read)?;
        let bytes = self.bytes.lock().unwrap();
        let stored = bytes.objects.get(&object.hash);
        if health(stored, object) != Integrity::Ok {
            return Err(RepositoryError::ObjectUnavailable);
        }
        let cursor = Cursor::new(Arc::clone(stored.unwrap()));
        Ok(Box::new(PinnedReader {
            _guard: guard,
            cursor,
        }))
    }
}
