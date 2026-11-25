// SPDX-License-Identifier: GPL-3.0-or-later
//! Read-only server adapter using the running institution's pool and lock.
use cap_std::fs::Dir;
use elzevir_app::repository::{self as r, PublicRepository};
use elzevir_store::ElzevirStore;
use std::{fs::File, path::Path, sync::Arc};

pub(crate) struct Public {
    store: Arc<ElzevirStore>,
    scope: Dir,
}
impl Public {
    pub(crate) fn new(store: Arc<ElzevirStore>, data: &Path) -> Result<Self, String> {
        let scope =
            Dir::open_ambient_dir(data, cap_std::ambient_authority()).map_err(|e| e.to_string())?;
        Ok(Self { store, scope })
    }
    fn verified(&self, object: r::ObjectInfo) -> r::Result<File> {
        let file = crate::repository_fs::object_source(&self.scope, object)
            .map_err(|_| r::RepositoryError::ObjectUnavailable)?;
        crate::repository_fs::verify_bytes(file, object)
            .map_err(|_| r::RepositoryError::ObjectUnavailable)
    }
}
impl PublicRepository for Public {
    fn for_instance(&self, instance: r::InstanceId) -> r::Result<Option<r::DepositId>> {
        self.store.readers().published_deposit(instance)
    }
    fn page(&self, id: r::DepositId) -> r::Result<r::DepositPage> {
        let (deposit, visible) = self.store.readers().repository_snapshot(id)?;
        deposit.public_page(visible, |obj| self.verified(obj).is_ok())
    }
    fn download(&self, version: r::VersionRef) -> r::Result<r::Download> {
        let (deposit, visible) = self.store.readers().repository_snapshot(version.deposit)?;
        deposit.public_download(visible, version.file, Some(version.seq), |obj| {
            Ok(Box::new(self.verified(obj)?))
        })
    }
}

impl r::DiscoveryReader for Public {
    fn published_deposits(
        &self,
        instances: &[r::InstanceId],
    ) -> r::Result<std::collections::BTreeMap<r::InstanceId, r::PublishedDeposit>> {
        self.store.published_deposits(instances)
    }
}
