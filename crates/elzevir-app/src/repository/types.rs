// SPDX-License-Identifier: GPL-3.0-or-later
use crate::staff::{Permission, StaffId, StaffIdentity};
use std::{fmt, str::FromStr};

/// Stable public deposit identity, generated once by the institution.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct DepositId(pub u128);
/// Stable logical file identity, independent of the bytes or display name.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct FileId(pub u128);
/// Digital collection identity, independent of item collections.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct DigitalCollectionId(pub u128);
/// Institution-wide idempotency key.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct OperationId(pub u128);
/// Immutable address of an exact revision.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct VersionRef {
    /// Owning deposit.
    pub deposit: DepositId,
    /// Logical file.
    pub file: FileId,
    /// One-based sequence; never reused.
    pub seq: u64,
}
/// SHA-256 of a single immutable byte stream.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct ObjectHash(pub [u8; 32]);

impl fmt::Display for ObjectHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl FromStr for ObjectHash {
    type Err = RepositoryError;
    fn from_str(s: &str) -> Result<Self> {
        let mut hash = [0; 32];
        if s.len() != 64
            || !s
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            return Err(RepositoryError::InvalidState);
        }
        for (byte, pair) in hash.iter_mut().zip(s.as_bytes().chunks_exact(2)) {
            let digits = std::str::from_utf8(pair).map_err(|_| RepositoryError::InvalidState)?;
            *byte = u8::from_str_radix(digits, 16).map_err(|_| RepositoryError::InvalidState)?;
        }
        Ok(Self(hash))
    }
}

/// Editorial lifecycle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DepositState {
    /// Visible to staff only.
    Draft,
    /// Public while the instance is visible.
    Published,
    /// Formerly published; its stable page is a tombstone.
    Withdrawn,
}
/// Function of a logical file.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileRole {
    /// Principal representation.
    Primary,
    /// Additional research material.
    Supplement,
    /// Administrative record, always restricted.
    Administrative,
    /// Label accompanying the deposit.
    Label,
}
/// Who may open this link to an object.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileAccess {
    /// Available subject to publication and OPAC visibility.
    Public,
    /// Cataloguing staff only.
    Staff,
}
/// An operator authorized by the existing staff permission model.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Operator(StaffId);
impl TryFrom<&StaffIdentity> for Operator {
    type Error = RepositoryError;
    fn try_from(identity: &StaffIdentity) -> Result<Self> {
        if identity.can(Permission::StaffInterface) && identity.can(Permission::Cataloguing) {
            Ok(Self(identity.id))
        } else {
            Err(RepositoryError::PermissionDenied)
        }
    }
}
impl Operator {
    /// Trusted actor for the audit trail.
    #[must_use]
    pub fn id(self) -> StaffId {
        self.0
    }
}
/// Actor opening a page or download.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadAudience {
    /// Anonymous visitor.
    Public,
    /// Authenticated operator.
    Staff(Operator),
}
/// Idempotent upload parameters, compared together with the actual bytes and actor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Upload {
    /// Retry key.
    pub operation: OperationId,
    /// Owning deposit.
    pub deposit: DepositId,
    /// Destination logical file.
    pub file: FileId,
    /// Compare-and-swap precondition; None requires no current version.
    pub expected_current_seq: Option<u64>,
    /// Presentation name, never a storage path.
    pub display_name: String,
    /// Validated media type for the version.
    pub media_type: String,
}
/// Identity and length of installed bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectInfo {
    /// Content address.
    pub hash: ObjectHash,
    /// Verified stream length.
    pub bytes: u64,
}
/// Durable answer to the original operation, including its original time.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UploadReceipt {
    /// Immutable version identity.
    pub version: VersionRef,
    /// Content identity.
    pub object: ObjectInfo,
    /// Original upload parameters.
    pub upload: Upload,
    /// Trusted uploader.
    pub actor: StaffId,
    /// UTC seconds.
    pub at: i64,
}
impl UploadReceipt {
    /// Resolve a replay before consulting the current revision.
    ///
    /// # Errors
    /// A retry key was reused for different content, actor or parameters.
    pub fn replay(&self, upload: &Upload, object: ObjectInfo, actor: Operator) -> Result<Self> {
        if &self.upload == upload && self.object == object && self.actor == actor.id() {
            Ok(self.clone())
        } else {
            Err(RepositoryError::OperationConflict)
        }
    }
}
/// Object health used for staff diagnosis and public availability.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Integrity {
    /// Verified object.
    Ok,
    /// Object missing from storage.
    Missing,
    /// Object present but contents or length differ.
    Corrupt,
}
/// Result of an institutional repository operation.
pub type Result<T> = std::result::Result<T, RepositoryError>;

/// Stable application refusals; transport adapters translate them at the boundary.
#[derive(Clone, Copy, Debug, thiserror::Error, PartialEq, Eq)]
#[error("{}", self.code())]
pub enum RepositoryError {
    /// No such catalog instance.
    InstanceNotFound,
    /// A deposit retains the instance.
    InstanceHasDeposit,
    /// One deposit per instance.
    DepositAlreadyExists,
    /// Unknown or concealed resource.
    NotVisible,
    /// Public tombstone.
    Withdrawn,
    /// Missing management permissions.
    PermissionDenied,
    /// Invalid editorial transition or input.
    InvalidState,
    /// Stale current sequence.
    VersionConflict,
    /// Reused operation key.
    OperationConflict,
    /// Stream exceeds configured limit.
    TooLarge,
    /// Upload slot unavailable.
    UploadBusy,
    /// Free space exhausted.
    InsufficientSpace,
    /// Caller-supplied hash disagrees with content.
    ChecksumMismatch,
    /// Object absent or corrupt.
    ObjectUnavailable,
    /// Interrupted upload or unavailable metadata storage.
    Unavailable,
    /// Client lost the result; retry with the same operation key.
    CommitUnknown,
}
impl RepositoryError {
    /// Stable machine-readable code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::InstanceNotFound => "instance_not_found",
            Self::InstanceHasDeposit => "instance_has_deposit",
            Self::DepositAlreadyExists => "deposit_already_exists",
            Self::NotVisible => "not_visible",
            Self::Withdrawn => "withdrawn",
            Self::PermissionDenied => "permission_denied",
            Self::InvalidState => "invalid_state",
            Self::VersionConflict => "version_conflict",
            Self::OperationConflict => "operation_conflict",
            Self::TooLarge => "too_large",
            Self::UploadBusy => "upload_busy",
            Self::InsufficientSpace => "insufficient_space",
            Self::ChecksumMismatch => "checksum_mismatch",
            Self::ObjectUnavailable => "object_unavailable",
            Self::Unavailable => "unavailable",
            Self::CommitUnknown => "commit_unknown",
        }
    }
}
