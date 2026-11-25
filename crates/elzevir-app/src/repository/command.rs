// SPDX-License-Identifier: GPL-3.0-or-later
use super::{
    DepositId, DigitalCollectionId, Edit, InstanceId, Integrity, ObjectInfo, Operator, Upload,
    VersionRef,
};
/// Transaction requested by a trusted institution adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepositoryAction {
    /// Create a draft.
    Draft(DepositId, InstanceId),
    /// Create a collection.
    Collection(DigitalCollectionId, String),
    /// Change deposit metadata.
    Edit(DepositId, Edit),
    /// Commit an immutable version.
    Version(Upload, ObjectInfo),
    /// Audit a staff download.
    Download(VersionRef),
    /// Record observed object integrity.
    Verify(VersionRef, Integrity),
}
/// Authorized, dated writer command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryCommand {
    /// Authenticated operator.
    pub actor: Operator,
    /// UTC seconds.
    pub at: i64,
    /// Requested transaction.
    pub action: RepositoryAction,
}

impl RepositoryCommand {
    /// Stable audit kind.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match &self.action {
            RepositoryAction::Collection(..) => "repository.collection",
            RepositoryAction::Draft(..) => "repository.draft",
            RepositoryAction::Version(..) => "repository.version",
            RepositoryAction::Download(..) => "repository.download",
            RepositoryAction::Verify(..) => "repository.verify",
            RepositoryAction::Edit(_, edit) => match edit {
                Edit::AddFile(_) => "repository.file",
                Edit::Join(_) => "repository.join",
                Edit::Publish(_) => "repository.publish",
                Edit::Withdraw => "repository.withdraw",
                Edit::Retire(..) => "repository.retire",
            },
        }
    }
    /// Audit metadata excluding file paths and bytes.
    #[must_use]
    pub fn audit_payload(&self) -> serde_json::Value {
        use serde_json::json;
        match &self.action {
            RepositoryAction::Draft(id, i) => {
                json!({"deposit":format!("{:032x}",id.0),"instance":i.0})
            }
            RepositoryAction::Collection(id, _) => json!({"collection":format!("{:032x}",id.0)}),
            RepositoryAction::Edit(id, Edit::Join(c)) => {
                json!({"deposit":format!("{:032x}",id.0), "collection":format!("{:032x}",c.0)})
            }
            RepositoryAction::Edit(id, Edit::AddFile(f)) => {
                json!({"deposit":format!("{:032x}",id.0), "file":format!("{:032x}",f.id.0)})
            }
            RepositoryAction::Edit(_, Edit::Retire(v, withdraw)) => {
                json!({"file":format!("{:032x}",v.file.0), "seq":v.seq, "withdraw_deposit":withdraw})
            }
            RepositoryAction::Edit(id, _) => json!({"deposit":format!("{:032x}",id.0)}),
            RepositoryAction::Verify(v, status) => {
                json!({"file":format!("{:032x}",v.file.0),"seq":v.seq,"integrity":status.code()})
            }
            RepositoryAction::Version(u, _) => {
                json!({"file":format!("{:032x}",u.file.0),"operation":format!("{:032x}",u.operation.0)})
            }
            RepositoryAction::Download(v) => {
                json!({"file":format!("{:032x}",v.file.0),"seq":v.seq})
            }
        }
    }
}
