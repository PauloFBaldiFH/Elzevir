// SPDX-License-Identifier: GPL-3.0-or-later
//! Public views shared by the offline pilot and the OPAC.
use super::{
    Deposit, DepositId, DepositPage, DepositState, Download, FileAccess, FileId, InstanceId,
    ObjectInfo, RepositoryError, Result, UploadReceipt, VersionRef,
};
use std::io::Read;

/// The public HTTP interface can only request an authorized view or exact version.
pub trait PublicRepository: super::DiscoveryReader {
    /// Public deposit link for a visible catalog record.
    ///
    /// # Errors
    /// Metadata unavailable.
    fn for_instance(&self, instance: InstanceId) -> Result<Option<DepositId>>;
    /// # Errors
    /// Concealed, withdrawn or unavailable deposit.
    fn page(&self, deposit: DepositId) -> Result<DepositPage>;
    /// # Errors
    /// Concealed version or unavailable object.
    fn download(&self, version: VersionRef) -> Result<Download>;
}

impl Deposit {
    fn public_visibility(&self, visible: bool, page: bool) -> Result<()> {
        if !visible || self.state == DepositState::Draft {
            return Err(RepositoryError::NotVisible);
        }
        if self.state == DepositState::Withdrawn {
            return Err(if page {
                RepositoryError::Withdrawn
            } else {
                RepositoryError::NotVisible
            });
        }
        Ok(())
    }

    /// Project the published links of a single catalog snapshot.
    ///
    /// # Errors
    /// A hidden or withdrawn deposit.
    pub fn public_page(
        mut self,
        visible: bool,
        healthy: impl Fn(ObjectInfo) -> bool,
    ) -> Result<DepositPage> {
        self.public_visibility(visible, true)?;
        self.files.retain(|_, head| {
            head.description.access == FileAccess::Public
                && head.current.as_ref().is_some_and(|r| healthy(r.object))
        });
        Ok(self.into_page())
    }

    pub(super) fn into_page(self) -> DepositPage {
        DepositPage {
            id: self.id,
            instance: self.instance,
            state: self.state,
            collections: self.collections.into_iter().collect(),
            files: self
                .files
                .into_values()
                .map(|f| (f.description, f.current.map(|r| r.version)))
                .collect(),
        }
    }

    /// Open the current public link after checking visibility and optional exact sequence.
    ///
    /// # Errors
    /// A concealed version or unavailable object.
    pub fn public_download(
        &self,
        visible: bool,
        file: FileId,
        seq: Option<u64>,
        open: impl FnOnce(ObjectInfo) -> Result<Box<dyn Read + Send>>,
    ) -> Result<Download> {
        self.public_visibility(visible, false)?;
        let receipt = self
            .files
            .get(&file)
            .filter(|head| head.description.access == FileAccess::Public)
            .and_then(|head| head.current.as_ref())
            .filter(|r| seq.is_none_or(|s| r.version.seq == s))
            .ok_or(RepositoryError::NotVisible)?;
        Ok(Download::from_receipt(receipt, open(receipt.object)?))
    }
}
impl Download {
    pub(super) fn from_receipt(receipt: &UploadReceipt, reader: Box<dyn Read + Send>) -> Self {
        Self {
            version: receipt.version,
            name: receipt.upload.display_name.clone(),
            media_type: receipt.upload.media_type.clone(),
            bytes: receipt.object.bytes,
            reader,
        }
    }
}
