// SPDX-License-Identifier: GPL-3.0-or-later
use super::{
    DepositId, DepositState, DigitalCollectionId, FileAccess, FileId, FileRole, InstanceId,
    ObjectInfo, Operator, RepositoryError, Result, Upload, UploadReceipt, VersionRef,
};
use std::collections::{BTreeMap, BTreeSet};

/// Editorial file description. Access applies to the logical link, even after deduplication.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileDescription {
    /// Stable identity.
    pub id: FileId,
    /// User-facing label.
    pub label: String,
    /// Function within the deposit.
    pub role: FileRole,
    /// Read policy.
    pub access: FileAccess,
}
/// File head; old versions are fetched individually.
#[derive(Clone, Debug)]
pub struct FileHead {
    /// Editorial description.
    pub description: FileDescription,
    /// Highest committed sequence, retained on withdrawal.
    pub last_seq: u64,
    /// Version currently designated by the operator.
    pub current: Option<UploadReceipt>,
}
/// Metadata snapshot for a single deposit; never includes object bytes or historical versions.
#[derive(Clone, Debug)]
pub struct Deposit {
    /// Stable public identity.
    pub id: DepositId,
    /// Catalog instance, independent of collection membership.
    pub instance: InstanceId,
    /// Publication state.
    pub state: DepositState,
    /// Collection memberships.
    pub collections: BTreeSet<DigitalCollectionId>,
    /// Logical files.
    pub files: BTreeMap<FileId, FileHead>,
}
/// Atomic editorial command for the metadata writer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Edit {
    /// Add a reference to an existing digital collection.
    Join(DigitalCollectionId),
    /// Create a logical file.
    AddFile(FileDescription),
    /// Publish conditional on the exact principal version verified by the object store.
    Publish(VersionRef),
    /// Withdraw a published deposit.
    Withdraw,
    /// Retire the exact version; optionally withdraw the deposit in the same commit.
    Retire(VersionRef, bool),
}

impl Deposit {
    /// Construct after validating the instance and both uniqueness constraints.
    #[must_use]
    pub fn draft(id: DepositId, instance: InstanceId) -> Self {
        Self {
            id,
            instance,
            state: DepositState::Draft,
            collections: BTreeSet::new(),
            files: BTreeMap::new(),
        }
    }

    /// # Errors
    /// Publication requires a current, public primary file.
    pub fn primary(&self) -> Result<&UploadReceipt> {
        self.files
            .values()
            .find(|f| {
                f.description.role == FileRole::Primary
                    && f.description.access == FileAccess::Public
            })
            .and_then(|f| f.current.as_ref())
            .ok_or(RepositoryError::InvalidState)
    }

    /// Apply inside the writer transaction. Returns whether the public projection changed.
    /// The adapter checks collection existence, persists version withdrawals and appends audit and effects.
    ///
    /// # Errors
    /// Invalid transition, unknown file or stale principal.
    pub fn apply(&mut self, edit: &Edit, opac_visible: bool) -> Result<bool> {
        let public = opac_visible && self.state == DepositState::Published;
        match edit {
            Edit::Join(id) => Ok(self.collections.insert(*id) && public),
            Edit::AddFile(description) => {
                if description.label.trim().is_empty()
                    || description.role == FileRole::Administrative
                        && description.access != FileAccess::Staff
                    || description.role == FileRole::Primary
                        && self
                            .files
                            .values()
                            .any(|f| f.description.role == FileRole::Primary)
                    || self.files.contains_key(&description.id)
                {
                    return Err(RepositoryError::InvalidState);
                }
                self.files.insert(
                    description.id,
                    FileHead {
                        description: description.clone(),
                        last_seq: 0,
                        current: None,
                    },
                );
                Ok(false)
            }
            Edit::Publish(expected) => {
                if !opac_visible || self.state == DepositState::Published {
                    return Err(RepositoryError::InvalidState);
                }
                if self.primary()?.version != *expected {
                    return Err(RepositoryError::VersionConflict);
                }
                self.state = DepositState::Published;
                Ok(true)
            }
            Edit::Withdraw => {
                if self.state != DepositState::Published {
                    return Err(RepositoryError::InvalidState);
                }
                self.state = DepositState::Withdrawn;
                Ok(public)
            }
            Edit::Retire(version, withdraw) => {
                let file = self
                    .files
                    .get_mut(&version.file)
                    .ok_or(RepositoryError::NotVisible)?;
                if version.deposit != self.id || version.seq == 0 || version.seq > file.last_seq {
                    return Err(RepositoryError::NotVisible);
                }
                let current = file.current.as_ref().is_some_and(|r| r.version == *version);
                let needs_withdrawal = current
                    && file.description.role == FileRole::Primary
                    && self.state == DepositState::Published;
                if *withdraw != needs_withdrawal {
                    return Err(RepositoryError::InvalidState);
                }
                if *withdraw {
                    self.state = DepositState::Withdrawn;
                }
                if current {
                    file.current = None;
                }
                Ok(current && public && file.description.access == FileAccess::Public)
            }
        }
    }

    /// Advance a file head in the same transaction as recording the immutable receipt.
    /// The writer resolves an existing operation key before calling this function.
    ///
    /// # Errors
    /// Invalid metadata, stale revision, or unknown destination.
    pub fn attach(
        &mut self,
        upload: Upload,
        object: ObjectInfo,
        actor: Operator,
        at: i64,
    ) -> Result<UploadReceipt> {
        if self.id != upload.deposit {
            return Err(RepositoryError::NotVisible);
        }
        if object.bytes == 0
            || upload.display_name.trim().is_empty()
            || upload.media_type.trim().is_empty()
        {
            return Err(RepositoryError::InvalidState);
        }
        let file = self
            .files
            .get_mut(&upload.file)
            .ok_or(RepositoryError::NotVisible)?;
        if file.current.as_ref().map(|r| r.version.seq) != upload.expected_current_seq {
            return Err(RepositoryError::VersionConflict);
        }
        let seq = file
            .last_seq
            .checked_add(1)
            .ok_or(RepositoryError::InvalidState)?;
        let version = VersionRef {
            deposit: self.id,
            file: upload.file,
            seq,
        };
        let receipt = UploadReceipt {
            version,
            object,
            upload,
            actor: actor.id(),
            at,
        };
        file.last_seq = seq;
        file.current = Some(receipt.clone());
        Ok(receipt)
    }
}
