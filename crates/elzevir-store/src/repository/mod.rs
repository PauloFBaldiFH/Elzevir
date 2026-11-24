// SPDX-License-Identifier: GPL-3.0-or-later
//! Offline institution metadata adapter. Its command handler uses the standard
//! single writer, savepoints, read pool, audit and transactional effect outbox.
pub mod backup;
mod read;
use crate::{CommandFailed, CommandHandler, Store, StoreConfig, StoreError, WriteTx};
use elzevir_app::repository as r;
use rusqlite::{Connection, params};
use std::path::Path;

fn integer(v: u64) -> r::Result<i64> {
    i64::try_from(v).map_err(|_| r::RepositoryError::InvalidState)
}
fn db(_: rusqlite::Error) -> r::RepositoryError {
    r::RepositoryError::Unavailable
}
fn constraint(e: rusqlite::Error, refusal: r::RepositoryError) -> r::RepositoryError {
    if e.sqlite_error_code() == Some(rusqlite::ErrorCode::ConstraintViolation) {
        refusal
    } else {
        db(e)
    }
}

/// An offline store owned by the process holding the institution lock.
pub struct MetadataStore(Store<Handler>);
struct Handler;

enum Action {
    Draft(r::DepositId, r::InstanceId),
    Collection(r::DigitalCollectionId, String),
    Edit(r::DepositId, r::Edit),
    Version(r::Upload, r::ObjectInfo),
    Download(r::VersionRef),
    Verify(r::VersionRef, r::Integrity),
}
struct Command {
    actor: r::Operator,
    at: i64,
    action: Action,
}

enum Refusal {
    Error(r::RepositoryError),
    Replay(Box<r::UploadReceipt>),
}
impl From<r::RepositoryError> for Refusal {
    fn from(e: r::RepositoryError) -> Self {
        Self::Error(e)
    }
}

impl MetadataStore {
    /// Open the institution database after acquiring exclusive offline access.
    ///
    /// # Errors
    /// Opening or migration failed.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        Store::open(path, &StoreConfig::default(), Handler).map(Self)
    }
    fn submit(
        &self,
        actor: r::Operator,
        at: i64,
        action: Action,
    ) -> r::Result<Option<r::UploadReceipt>> {
        let pending = self
            .0
            .submit_as(actor.id().0, Command { actor, at, action })
            .map_err(|_| r::RepositoryError::Unavailable)?;
        match pending.wait() {
            Ok(output) => Ok(output),
            Err(CommandFailed::Rejected(Refusal::Replay(receipt))) => Ok(Some(*receipt)),
            Err(CommandFailed::Rejected(Refusal::Error(e))) => Err(e),
            Err(CommandFailed::WriterGone) => Err(r::RepositoryError::CommitUnknown),
            Err(CommandFailed::Storage(_)) => Err(r::RepositoryError::Unavailable),
        }
    }
    /// Record an explicit staff verification and reindex affected public instances
    /// if the observed state changed.
    ///
    /// # Errors
    /// Unknown version or failed transaction.
    pub fn record_integrity(
        &self,
        actor: r::Operator,
        at: i64,
        version: r::VersionRef,
        status: r::Integrity,
    ) -> r::Result<()> {
        self.submit(actor, at, Action::Verify(version, status))
            .map(drop)
    }
    /// Register a digital collection with an institution-scoped public ID.
    ///
    /// # Errors
    /// Invalid or duplicate collection, or a writer failure.
    pub fn create_collection(
        &self,
        actor: r::Operator,
        at: i64,
        id: r::DigitalCollectionId,
        name: String,
    ) -> r::Result<()> {
        self.submit(actor, at, Action::Collection(id, name))
            .map(drop)
    }
}

impl r::RepositoryWrite for MetadataStore {
    fn create_draft(
        &self,
        actor: r::Operator,
        at: i64,
        id: r::DepositId,
        instance: r::InstanceId,
    ) -> r::Result<()> {
        self.submit(actor, at, Action::Draft(id, instance))
            .map(drop)
    }
    fn edit(&self, actor: r::Operator, at: i64, id: r::DepositId, edit: r::Edit) -> r::Result<()> {
        self.submit(actor, at, Action::Edit(id, edit)).map(drop)
    }
    fn commit_version(
        &self,
        actor: r::Operator,
        at: i64,
        upload: r::Upload,
        object: r::ObjectInfo,
    ) -> r::Result<r::UploadReceipt> {
        self.submit(actor, at, Action::Version(upload, object))?
            .ok_or(r::RepositoryError::Unavailable)
    }
    fn audit_download(&self, actor: r::Operator, at: i64, version: r::VersionRef) -> r::Result<()> {
        self.submit(actor, at, Action::Download(version)).map(drop)
    }
}

impl CommandHandler for Handler {
    type Command = Command;
    type Output = Option<r::UploadReceipt>;
    type Rejection = Refusal;
    fn kind(cmd: &Command) -> &'static str {
        match &cmd.action {
            Action::Collection(..) => "repository.collection",
            Action::Draft(..) => "repository.draft",
            Action::Version(..) => "repository.version",
            Action::Download(..) => "repository.download",
            Action::Verify(..) => "repository.verify",
            Action::Edit(_, edit) => match edit {
                r::Edit::AddFile(_) => "repository.file",
                r::Edit::Join(_) => "repository.join",
                r::Edit::Publish(_) => "repository.publish",
                r::Edit::Withdraw => "repository.withdraw",
                r::Edit::Retire(..) => "repository.retire",
            },
        }
    }
    fn audit_payload(cmd: &Command) -> impl serde::Serialize {
        use serde_json::json;
        match &cmd.action {
            Action::Draft(id, i) => json!({"deposit":format!("{:032x}",id.0),"instance":i.0}),
            Action::Collection(id, _) => json!({"collection":format!("{:032x}",id.0)}),
            Action::Edit(id, r::Edit::Join(c)) => {
                json!({"deposit":format!("{:032x}",id.0), "collection":format!("{:032x}",c.0)})
            }
            Action::Edit(id, r::Edit::AddFile(f)) => {
                json!({"deposit":format!("{:032x}",id.0), "file":format!("{:032x}",f.id.0)})
            }
            Action::Edit(_, r::Edit::Retire(v, withdraw)) => {
                json!({"file":format!("{:032x}",v.file.0), "seq":v.seq, "withdraw_deposit":withdraw})
            }
            Action::Edit(id, _) => json!({"deposit":format!("{:032x}",id.0)}),
            Action::Verify(v, status) => {
                json!({"file":format!("{:032x}",v.file.0),"seq":v.seq,"integrity":integrity_code(*status)})
            }
            Action::Version(u, _) => {
                json!({"file":format!("{:032x}",u.file.0),"operation":format!("{:032x}",u.operation.0)})
            }
            Action::Download(v) => json!({"file":format!("{:032x}",v.file.0),"seq":v.seq}),
        }
    }
    fn apply(
        &mut self,
        tx: &WriteTx<'_>,
        cmd: &Command,
    ) -> Result<Self::Output, CommandFailed<Refusal>> {
        apply(tx, cmd).map_err(CommandFailed::Rejected)
    }
}
fn apply(tx: &WriteTx<'_>, cmd: &Command) -> Result<Option<r::UploadReceipt>, Refusal> {
    let c = tx.connection();
    match &cmd.action {
        Action::Collection(id, name) => {
            c.execute(
                "INSERT INTO repo_collections(id,name) VALUES(?1,?2)",
                params![id.0.to_be_bytes(), name],
            )
            .map_err(|e| constraint(e, r::RepositoryError::InvalidState))?;
        }
        Action::Draft(id, instance) => {
            let exists: bool = c
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM instances WHERE id=?1)",
                    [instance.0],
                    |row| row.get(0),
                )
                .map_err(db)?;
            if !exists {
                return Err(r::RepositoryError::InstanceNotFound.into());
            }
            c.execute(
                "INSERT INTO repo_deposits(id,instance_id,state,created_at) VALUES(?1,?2,'draft',?3)",
                params![id.0.to_be_bytes(), instance.0, cmd.at],
            ).map_err(|e| constraint(e, r::RepositoryError::DepositAlreadyExists))?;
        }
        Action::Download(v) => {
            read::receipt(c, *v)?;
        }
        Action::Verify(v, status) => {
            let object = read::receipt(c, *v)?.object;
            remember_integrity(tx, object, *status, cmd.at, None)?;
        }
        Action::Edit(id, edit) => {
            let (mut deposit, visible) = read::snapshot(c, *id)?;
            let changed = deposit.apply(edit, visible)?;
            persist_edit(c, *id, edit, cmd.at)?;
            if changed {
                reindex(tx, deposit.instance)?;
            }
        }
        Action::Version(upload, object) => {
            if let Some(previous) = read::by_operation(c, upload.operation)? {
                // Returning a refusal rolls back even the automatic audit entry.
                return Err(Refusal::Replay(Box::new(
                    previous.replay(upload, *object, cmd.actor)?,
                )));
            }
            let (mut deposit, visible) = read::snapshot(c, upload.deposit)?;
            let receipt = deposit.attach(upload.clone(), *object, cmd.actor, cmd.at)?;
            let public = visible
                && deposit.state == r::DepositState::Published
                && deposit
                    .files
                    .get(&upload.file)
                    .is_some_and(|f| f.description.access == r::FileAccess::Public);
            persist_version(c, &receipt)?;
            remember_integrity(
                tx,
                *object,
                r::Integrity::Ok,
                cmd.at,
                public.then_some(deposit.instance),
            )?;
            if public {
                reindex(tx, deposit.instance)?;
            }
            return Ok(Some(receipt));
        }
    }
    Ok(None)
}

fn reindex(tx: &WriteTx<'_>, id: r::InstanceId) -> r::Result<()> {
    crate::catalog::reindex(tx, id).map_err(|_| r::RepositoryError::Unavailable)
}
fn persist_version(c: &Connection, r: &r::UploadReceipt) -> r::Result<()> {
    let u = &r.upload;
    let hash = r.object.hash.0;
    c.execute(
        "INSERT INTO repo_objects(sha256,bytes,created_at,last_verified_at,integrity)
         VALUES(?1,?2,?3,?3,'ok') ON CONFLICT(sha256) DO NOTHING",
        params![hash, integer(r.object.bytes)?, r.at],
    )
    .map_err(db)?;
    let file = u.file.0.to_be_bytes();
    c.execute(
        "INSERT INTO repo_versions(file_id,seq,object_hash,display_name,media_type,created_at,
         actor,operation_id,expected_seq) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![
            file,
            integer(r.version.seq)?,
            hash,
            u.display_name,
            u.media_type,
            r.at,
            r.actor.0,
            u.operation.0.to_be_bytes(),
            u.expected_current_seq.map(integer).transpose()?,
        ],
    )
    .map_err(db)?;
    c.execute(
        "UPDATE repo_files SET current_seq=?1,row_version=row_version+1 WHERE id=?2",
        params![integer(r.version.seq)?, file],
    )
    .map_err(db)?;
    Ok(())
}
fn integrity_code(status: r::Integrity) -> &'static str {
    match status {
        r::Integrity::Ok => "ok",
        r::Integrity::Missing => "missing",
        r::Integrity::Corrupt => "corrupt",
    }
}

fn remember_integrity(
    tx: &WriteTx<'_>,
    object: r::ObjectInfo,
    status: r::Integrity,
    at: i64,
    already_reindexed: Option<r::InstanceId>,
) -> r::Result<()> {
    let c = tx.connection();
    let (size, old): (i64, String) = c
        .query_row(
            "SELECT bytes,integrity FROM repo_objects WHERE sha256=?1",
            [object.hash.0],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(db)?;
    if size != integer(object.bytes)? {
        return Err(r::RepositoryError::ObjectUnavailable);
    }
    let code = integrity_code(status);
    c.execute(
        "UPDATE repo_objects SET last_verified_at=?1,integrity=?2 WHERE sha256=?3",
        params![at, code, object.hash.0],
    )
    .map_err(db)?;
    if code != old {
        let mut query=c.prepare("SELECT DISTINCT d.instance_id FROM repo_versions v
            JOIN repo_files f ON f.id=v.file_id AND f.current_seq=v.seq
            JOIN repo_deposits d ON d.id=f.deposit_id JOIN instances i ON i.id=d.instance_id
            WHERE v.object_hash=?1 AND f.access='public' AND d.state='published' AND i.opac_hidden=0").map_err(db)?;
        for row in query
            .query_map([object.hash.0], |row| row.get::<_, i64>(0))
            .map_err(db)?
        {
            let instance = r::InstanceId(row.map_err(db)?);
            if already_reindexed != Some(instance) {
                reindex(tx, instance)?;
            }
        }
    }
    Ok(())
}
fn persist_edit(c: &Connection, id: r::DepositId, edit: &r::Edit, at: i64) -> r::Result<()> {
    let key = id.0.to_be_bytes();
    match edit {
        r::Edit::Join(collection) => {
            c.execute(
                "INSERT INTO repo_collection_members VALUES(?1,?2) ON CONFLICT DO NOTHING",
                params![collection.0.to_be_bytes(), key],
            )
            .map_err(|e| constraint(e, r::RepositoryError::NotVisible))?;
        }
        r::Edit::AddFile(f) => {
            let role = match f.role {
                r::FileRole::Primary => "primary",
                r::FileRole::Supplement => "supplement",
                r::FileRole::Administrative => "administrative",
                r::FileRole::Label => "label",
            };
            let access = match f.access {
                r::FileAccess::Staff => "staff",
                r::FileAccess::Public => "public",
            };
            c.execute(
                "INSERT INTO repo_files(id,deposit_id,label,role,access) VALUES(?1,?2,?3,?4,?5)",
                params![f.id.0.to_be_bytes(), key, f.label, role, access],
            )
            .map_err(|e| constraint(e, r::RepositoryError::InvalidState))?;
        }
        r::Edit::Publish(_) => {
            c.execute(
                "UPDATE repo_deposits SET state='published',published_at=?1,withdrawn_at=NULL,
                 row_version=row_version+1 WHERE id=?2",
                params![at, key],
            )
            .map_err(db)?;
        }
        r::Edit::Withdraw => withdraw(c, key, at)?,
        r::Edit::Retire(v, also_withdraw) => {
            let file = v.file.0.to_be_bytes();
            c.execute(
                "UPDATE repo_versions SET withdrawn_at=?1 WHERE file_id=?2 AND seq=?3 AND withdrawn_at IS NULL",
                params![at,file,integer(v.seq)?],
            ).map_err(db)?;
            c.execute(
                "UPDATE repo_files SET current_seq=NULL,row_version=row_version+1 WHERE id=?1 AND current_seq=?2",
                params![file,integer(v.seq)?],
            ).map_err(db)?;
            if *also_withdraw {
                withdraw(c, key, at)?;
            }
        }
    }
    Ok(())
}
fn withdraw(c: &Connection, key: [u8; 16], at: i64) -> r::Result<()> {
    c.execute(
        "UPDATE repo_deposits SET state='withdrawn',withdrawn_at=?1,row_version=row_version+1 WHERE id=?2",
        params![at,key],
    ).map_err(db)?;
    Ok(())
}

#[cfg(test)]
mod tests;
