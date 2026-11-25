// SPDX-License-Identifier: GPL-3.0-or-later
//! Repository service shared by the offline CLI and staff operations in the server.
use crate::repository_fs::FsObjectStore;
use elzevir_app::repository::{self as r, Clock, RepositoryRead};
use elzevir_store::repository::MetadataStore;
use std::io::Read;
pub(crate) struct WallClock;
impl Clock for WallClock {
    fn now(&self) -> i64 {
        jiff::Timestamp::now().as_second()
    }
}

pub(crate) type InstitutionRepository = r::Repository<MetadataStore, FsObjectStore, WallClock>;

pub(crate) struct ServerDesk(pub InstitutionRepository);
impl r::RepositoryDesk for ServerDesk {
    fn find(&self, _: r::Operator, instance: r::InstanceId) -> r::Result<r::DeskInstance> {
        self.0.metadata.staff_deposit(instance)
    }
    fn draft(
        &self,
        actor: r::Operator,
        id: r::DepositId,
        instance: r::InstanceId,
    ) -> r::Result<()> {
        self.0.create_draft(actor, id, instance)
    }
    fn view(&self, _: r::Operator, id: r::DepositId, filter: &str) -> r::Result<r::DeskView> {
        self.0.metadata.desk_view(id, filter)
    }
    fn change(&self, actor: r::Operator, id: r::DepositId, edit: r::DeskEdit) -> r::Result<()> {
        match edit {
            r::DeskEdit::File(file) => self.0.add_file(actor, id, file),
            r::DeskEdit::Collection(key, name) => {
                self.0.metadata.snapshot(id)?;
                self.0
                    .metadata
                    .create_collection(actor, self.0.clock.now(), key, name)
            }
            r::DeskEdit::Join(key) => self.0.associate_collection(actor, id, key),
            r::DeskEdit::Publish => self.0.publish(actor, id),
            r::DeskEdit::Withdraw => self.0.withdraw(actor, id),
            r::DeskEdit::Retire(file, seq, together) => self.0.withdraw_version(
                actor,
                r::VersionRef {
                    deposit: id,
                    file,
                    seq,
                },
                together,
            ),
        }
    }
    fn upload(
        &self,
        actor: r::Operator,
        upload: r::Upload,
        bytes: &mut dyn Read,
    ) -> r::Result<r::UploadReceipt> {
        self.0.upload(actor, upload, bytes, None)
    }
    fn download(&self, actor: r::Operator, v: r::VersionRef) -> r::Result<r::Download> {
        self.0.download(
            v.deposit,
            v.file,
            Some(v.seq),
            r::ReadAudience::Staff(actor),
        )
    }
}
