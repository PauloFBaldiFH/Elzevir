// SPDX-License-Identifier: GPL-3.0-or-later
//! Institutional repository. Metadata commits and immutable object installation have
//! distinct durability boundaries. A failed metadata commit may leave a collectable object.
mod deposit;
mod types;

pub use deposit::*;
pub use elzevir_domain::catalog::InstanceId;
use std::io::Read;
pub use types::*;

/// Time supplied by the host, in UTC seconds.
pub trait Clock {
    /// Current instant.
    fn now(&self) -> i64;
}

/// Consistent institution-scoped metadata reads.
pub trait RepositoryRead {
    /// Deposit and current OPAC visibility from a single snapshot.
    ///
    /// # Errors
    /// Unknown deposit or read failure.
    fn snapshot(&self, id: DepositId) -> Result<(Deposit, bool)>;
    /// Exact immutable receipt including historical and retired versions.
    ///
    /// # Errors
    /// Unknown address or read failure.
    fn receipt(&self, version: VersionRef) -> Result<UploadReceipt>;
    /// All retained references to an object for a staff integrity report.
    ///
    /// # Errors
    /// Read failure.
    fn references(&self, hash: ObjectHash) -> Result<Vec<VersionRef>>;
    /// Check for a deposit in any state. The eventual deletion must repeat the guard atomically.
    ///
    /// # Errors
    /// `InstanceHasDeposit` for a linked instance; `InstanceNotFound` for an unknown one.
    fn ensure_deletable(&self, instance: InstanceId) -> Result<()>;
}

/// Each call is one audited writer command; effects are committed with metadata.
pub trait RepositoryWrite {
    /// # Errors
    /// Unknown instance or duplicate deposit/identity.
    fn create_draft(
        &self,
        actor: Operator,
        at: i64,
        id: DepositId,
        instance: InstanceId,
    ) -> Result<()>;
    /// Apply the transition to current metadata, with current OPAC visibility.
    ///
    /// # Errors
    /// Invalid membership, version, publication or withdrawal.
    fn edit(&self, actor: Operator, at: i64, id: DepositId, edit: Edit) -> Result<()>;
    /// Record staff retrieval of an exact revision, including the trusted actor.
    ///
    /// # Errors
    /// Audit write failure; a failed audit must prevent delivery.
    fn audit_download(&self, actor: Operator, at: i64, version: VersionRef) -> Result<()>;
    /// Resolve idempotency first; then compare the head, insert the version, move the
    /// head, audit and enqueue reindexing for visible public files. Reply after commit.
    ///
    /// # Errors
    /// Key or revision conflict, commit failure, or lost acknowledgement.
    fn commit_version(
        &self,
        actor: Operator,
        at: i64,
        upload: Upload,
        object: ObjectInfo,
    ) -> Result<UploadReceipt>;
}

/// Installation guard. Holding it prevents maintenance until the metadata commit completes.
pub trait InstalledObject {
    /// Verified content address and size.
    fn info(&self) -> ObjectInfo;
}

/// Streaming byte storage scoped to the current institution.
pub trait ObjectStore {
    /// Installation keeps the upload reservation alive.
    type Installed: InstalledObject;
    /// # Errors
    /// Busy, interrupted, too large, out of space, hash mismatch or damaged existing object.
    fn install(
        &self,
        source: &mut dyn Read,
        expected: Option<ObjectHash>,
        now: i64,
    ) -> Result<Self::Installed>;
    /// Latest known integrity, without buffering the file for a listing.
    fn integrity(&self, object: ObjectInfo) -> Integrity;
    /// # Errors
    /// Missing or damaged bytes; a successful reader pins the object until dropped.
    fn open(&self, object: ObjectInfo) -> Result<Box<dyn Read + Send>>;
}
/// Public or staff page projection. Internal upload parameters are never rendered.
#[derive(Debug)]
pub struct DepositPage {
    /// Stable identity.
    pub id: DepositId,
    /// Catalog link.
    pub instance: InstanceId,
    /// Editorial state.
    pub state: DepositState,
    /// Digital collection links.
    pub collections: Vec<DigitalCollectionId>,
    /// Authorized file descriptions with current version, when present.
    pub files: Vec<(FileDescription, Option<VersionRef>)>,
}
/// Authorized stream and presentation metadata, independent of filesystem paths.
pub struct Download {
    /// Version served.
    pub version: VersionRef,
    /// Name for presentation.
    pub name: String,
    /// Media type.
    pub media_type: String,
    /// Size in bytes.
    pub bytes: u64,
    /// Read guard, valid until the transfer ends.
    pub reader: Box<dyn Read + Send>,
}
/// Composition of the institution's metadata, bytes and clock.
pub struct Repository<M, O, C> {
    /// Read/write ports of the same institution.
    pub metadata: M,
    /// Object port.
    pub objects: O,
    /// Injected clock.
    pub clock: C,
}

impl<M: RepositoryRead + RepositoryWrite, O: ObjectStore, C: Clock> Repository<M, O, C> {
    /// # Errors
    /// A draft needs an existing, unbound instance and a fresh public ID.
    pub fn create_draft(&self, actor: Operator, id: DepositId, instance: InstanceId) -> Result<()> {
        self.metadata
            .create_draft(actor, self.clock.now(), id, instance)
    }
    /// # Errors
    /// Collection or deposit missing.
    pub fn associate_collection(
        &self,
        actor: Operator,
        id: DepositId,
        collection: DigitalCollectionId,
    ) -> Result<()> {
        self.metadata
            .edit(actor, self.clock.now(), id, Edit::Join(collection))
    }
    /// # Errors
    /// Duplicate primary or invalid access policy.
    pub fn add_file(&self, actor: Operator, id: DepositId, file: FileDescription) -> Result<()> {
        self.metadata
            .edit(actor, self.clock.now(), id, Edit::AddFile(file))
    }
    /// # Errors
    /// Stream installation or atomic version commit failed. Retry an unknown outcome with the same key.
    pub fn upload(
        &self,
        actor: Operator,
        upload: Upload,
        source: &mut dyn Read,
        expected_hash: Option<ObjectHash>,
    ) -> Result<UploadReceipt> {
        let pending = self
            .objects
            .install(source, expected_hash, self.clock.now())?;
        self.metadata
            .commit_version(actor, self.clock.now(), upload, pending.info())
    }
    /// # Errors
    /// Hidden instance, missing principal, unhealthy object or changed principal revision.
    pub fn publish(&self, actor: Operator, id: DepositId) -> Result<()> {
        let (deposit, visible) = self.metadata.snapshot(id)?;
        if !visible {
            return Err(RepositoryError::InvalidState);
        }
        let primary = deposit.primary()?;
        let _guard = self.objects.open(primary.object)?;
        self.metadata
            .edit(actor, self.clock.now(), id, Edit::Publish(primary.version))
    }
    /// # Errors
    /// Deposit is not published.
    pub fn withdraw(&self, actor: Operator, id: DepositId) -> Result<()> {
        self.metadata
            .edit(actor, self.clock.now(), id, Edit::Withdraw)
    }
    /// # Errors
    /// A current public principal requires simultaneous deposit withdrawal.
    pub fn withdraw_version(
        &self,
        actor: Operator,
        version: VersionRef,
        withdraw_deposit: bool,
    ) -> Result<()> {
        self.metadata.edit(
            actor,
            self.clock.now(),
            version.deposit,
            Edit::Retire(version, withdraw_deposit),
        )
    }

    fn visible_deposit(
        &self,
        id: DepositId,
        audience: ReadAudience,
        page: bool,
    ) -> Result<Deposit> {
        let (deposit, visible) = self.metadata.snapshot(id)?;
        if audience == ReadAudience::Public {
            if !visible || deposit.state == DepositState::Draft {
                return Err(RepositoryError::NotVisible);
            }
            if deposit.state == DepositState::Withdrawn {
                return Err(if page {
                    RepositoryError::Withdrawn
                } else {
                    RepositoryError::NotVisible
                });
            }
        }
        Ok(deposit)
    }
    /// # Errors
    /// Hidden, unknown, or withdrawn public page.
    pub fn page(&self, id: DepositId, audience: ReadAudience) -> Result<DepositPage> {
        let deposit = self.visible_deposit(id, audience, true)?;
        let files = deposit
            .files
            .into_values()
            .filter(|file| {
                audience != ReadAudience::Public
                    || (file.description.access == FileAccess::Public
                        && file
                            .current
                            .as_ref()
                            .is_some_and(|r| self.objects.integrity(r.object) == Integrity::Ok))
            })
            .map(|file| (file.description, file.current.map(|r| r.version)))
            .collect();
        Ok(DepositPage {
            id,
            instance: deposit.instance,
            state: deposit.state,
            collections: deposit.collections.into_iter().collect(),
            files,
        })
    }
    /// Open the current version or a specific sequence; every opening, including a
    /// future HEAD/range request, goes through the same metadata authorization.
    ///
    /// # Errors
    /// Concealed or absent link, or an authorized version with unavailable bytes.
    pub fn download(
        &self,
        deposit: DepositId,
        file: FileId,
        seq: Option<u64>,
        audience: ReadAudience,
    ) -> Result<Download> {
        let snapshot = self.visible_deposit(deposit, audience, false)?;
        let head = snapshot
            .files
            .get(&file)
            .ok_or(RepositoryError::NotVisible)?;
        let receipt = if let (ReadAudience::Staff(_), Some(seq)) = (audience, seq) {
            self.metadata.receipt(VersionRef { deposit, file, seq })?
        } else {
            if audience == ReadAudience::Public && head.description.access != FileAccess::Public {
                return Err(RepositoryError::NotVisible);
            }
            head.current
                .as_ref()
                .filter(|r| seq.is_none_or(|s| s == r.version.seq))
                .ok_or(RepositoryError::NotVisible)?
                .clone()
        };
        let reader = self.objects.open(receipt.object)?;
        if let ReadAudience::Staff(actor) = audience {
            self.metadata
                .audit_download(actor, self.clock.now(), receipt.version)?;
        }
        Ok(Download {
            version: receipt.version,
            name: receipt.upload.display_name,
            media_type: receipt.upload.media_type,
            bytes: receipt.object.bytes,
            reader,
        })
    }
    /// Inspect health and all affected links with staff authorization.
    ///
    /// # Errors
    /// Unknown version or metadata read failure.
    pub fn diagnose(
        &self,
        _actor: Operator,
        version: VersionRef,
    ) -> Result<(Integrity, Vec<VersionRef>)> {
        let object = self.metadata.receipt(version)?.object;
        Ok((
            self.objects.integrity(object),
            self.metadata.references(object.hash)?,
        ))
    }
}

#[cfg(test)]
mod memory;
#[cfg(test)]
mod tests;
