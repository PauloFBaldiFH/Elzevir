// SPDX-License-Identifier: GPL-3.0-or-later
//! Staff operations. Filesystem handles and SQLite transactions stay behind this port.
use super::{
    Deposit, DepositId, DigitalCollectionId, Download, FileDescription, InstanceId, Operator,
    Result, Upload, UploadReceipt, VersionRef,
};
use std::io::Read;

/// Catalogue title and deposit linked to an existing instance, independent of publication.
pub struct DeskInstance {
    /// Display title for checking the target before editing.
    pub title: String,
    /// Deposit in any state.
    pub deposit: Option<DepositId>,
}

/// Staff view of one deposit, including current heads and last sequence numbers.
pub struct DeskView {
    /// Metadata for the authorized operator.
    pub deposit: Deposit,
    /// Display title of the linked catalogue instance.
    pub title: String,
    /// Current visibility of the instance in the public catalogue.
    pub opac_visible: bool,
    /// Linked collections and matching choices for association.
    pub collections: Vec<(DigitalCollectionId, String)>,
}

/// Editor intent. Publication resolves and verifies the current primary server-side.
pub enum DeskEdit {
    /// New logical file.
    File(FileDescription),
    /// Create a collection independently of its membership.
    Collection(DigitalCollectionId, String),
    /// Associate an existing collection.
    Join(DigitalCollectionId),
    /// Publish the deposit.
    Publish,
    /// Withdraw the whole deposit.
    Withdraw,
    /// Withdraw an exact version, optionally with its deposit.
    Retire(super::FileId, u64, bool),
}

/// Operations available to authenticated cataloguers.
pub trait RepositoryDesk: Send + Sync {
    /// # Errors
    /// Unknown instance or failed read.
    fn find(&self, actor: Operator, instance: InstanceId) -> Result<DeskInstance>;
    /// # Errors
    /// Duplicate draft, unknown instance or writer failure.
    fn draft(&self, actor: Operator, id: DepositId, instance: InstanceId) -> Result<()>;
    /// # Errors
    /// Unknown deposit or failed read.
    fn view(&self, actor: Operator, id: DepositId, filter: &str) -> Result<DeskView>;
    /// # Errors
    /// Invalid transition, metadata or storage failure.
    fn change(&self, actor: Operator, id: DepositId, edit: DeskEdit) -> Result<()>;
    /// # Errors
    /// Interrupted input, failed installation, conflict or writer failure.
    fn upload(
        &self,
        actor: Operator,
        upload: Upload,
        bytes: &mut dyn Read,
    ) -> Result<UploadReceipt>;
    /// # Errors
    /// Missing version, damaged bytes or failed audit.
    fn download(&self, actor: Operator, version: VersionRef) -> Result<Download>;
}
