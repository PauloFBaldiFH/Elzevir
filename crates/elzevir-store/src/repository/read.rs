// SPDX-License-Identifier: GPL-3.0-or-later
use super::{MetadataStore, db, integer};
use elzevir_app::{repository as r, staff::StaffId};
use rusqlite::{Connection, OptionalExtension, Row, params};
fn unsigned(v: i64) -> rusqlite::Result<u64> {
    u64::try_from(v).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, v))
}

const VERSION: &str = "SELECT f.deposit_id,v.file_id,v.seq,v.operation_id,v.expected_seq,
 v.display_name,v.media_type,v.actor,v.created_at,v.object_hash,o.bytes
 FROM repo_versions v JOIN repo_files f ON f.id=v.file_id JOIN repo_objects o ON o.sha256=v.object_hash";

fn id(row: &Row<'_>, column: usize) -> rusqlite::Result<u128> {
    Ok(u128::from_be_bytes(row.get(column)?))
}

fn decode(row: &Row<'_>) -> rusqlite::Result<r::UploadReceipt> {
    let version = r::VersionRef {
        deposit: r::DepositId(id(row, 0)?),
        file: r::FileId(id(row, 1)?),
        seq: unsigned(row.get(2)?)?,
    };
    Ok(r::UploadReceipt {
        version,
        upload: r::Upload {
            deposit: version.deposit,
            file: version.file,
            operation: r::OperationId(id(row, 3)?),
            expected_current_seq: row.get::<_, Option<i64>>(4)?.map(unsigned).transpose()?,
            display_name: row.get(5)?,
            media_type: row.get(6)?,
        },
        actor: StaffId(row.get(7)?),
        at: row.get(8)?,
        object: r::ObjectInfo {
            hash: r::ObjectHash(row.get(9)?),
            bytes: unsigned(row.get(10)?)?,
        },
    })
}
pub(super) fn by_operation(
    c: &Connection,
    key: r::OperationId,
) -> r::Result<Option<r::UploadReceipt>> {
    c.query_row(
        &format!("{VERSION} WHERE v.operation_id=?1"),
        [key.0.to_be_bytes()],
        decode,
    )
    .optional()
    .map_err(db)
}
pub(super) fn receipt(c: &Connection, v: r::VersionRef) -> r::Result<r::UploadReceipt> {
    c.query_row(
        &format!("{VERSION} WHERE f.deposit_id=?1 AND v.file_id=?2 AND v.seq=?3"),
        params![
            v.deposit.0.to_be_bytes(),
            v.file.0.to_be_bytes(),
            integer(v.seq)?
        ],
        decode,
    )
    .optional()
    .map_err(db)?
    .ok_or(r::RepositoryError::NotVisible)
}
pub(super) fn snapshot(c: &Connection, deposit: r::DepositId) -> r::Result<(r::Deposit, bool)> {
    let key = deposit.0.to_be_bytes();
    let (instance, state, hidden): (i64, String, bool) = c
        .query_row(
            "SELECT d.instance_id,d.state,i.opac_hidden
        FROM repo_deposits d JOIN instances i ON i.id=d.instance_id WHERE d.id=?1",
            [key],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(db)?
        .ok_or(r::RepositoryError::NotVisible)?;
    let mut view = r::Deposit::draft(deposit, r::InstanceId(instance));
    view.state = match state.as_str() {
        "draft" => r::DepositState::Draft,
        "published" => r::DepositState::Published,
        "withdrawn" => r::DepositState::Withdrawn,
        _ => return Err(r::RepositoryError::Unavailable),
    };
    let mut collections = c
        .prepare("SELECT collection_id FROM repo_collection_members WHERE deposit_id=?1")
        .map_err(db)?;
    for row in collections.query_map([key], |row| id(row, 0)).map_err(db)? {
        view.collections
            .insert(r::DigitalCollectionId(row.map_err(db)?));
    }
    let mut files = c
        .prepare(
            "SELECT id,label,role,access,current_seq,
        coalesce((SELECT max(seq) FROM repo_versions WHERE file_id=f.id),0)
        FROM repo_files f WHERE deposit_id=?1",
        )
        .map_err(db)?;
    let mut rows = files.query([key]).map_err(db)?;
    while let Some(row) = rows.next().map_err(db)? {
        let file = r::FileId(id(row, 0).map_err(db)?);
        let role = match row.get::<_, String>(2).map_err(db)?.as_str() {
            "primary" => r::FileRole::Primary,
            "supplement" => r::FileRole::Supplement,
            "administrative" => r::FileRole::Administrative,
            "label" => r::FileRole::Label,
            _ => return Err(r::RepositoryError::Unavailable),
        };
        let access = match row.get::<_, String>(3).map_err(db)?.as_str() {
            "public" => r::FileAccess::Public,
            "staff" => r::FileAccess::Staff,
            _ => return Err(r::RepositoryError::Unavailable),
        };
        let seq: Option<u64> = row
            .get::<_, Option<i64>>(4)
            .map_err(db)?
            .map(unsigned)
            .transpose()
            .map_err(db)?;
        view.files.insert(
            file,
            r::FileHead {
                description: r::FileDescription {
                    id: file,
                    role,
                    access,
                    label: row.get(1).map_err(db)?,
                },
                last_seq: unsigned(row.get(5).map_err(db)?).map_err(db)?,
                current: seq
                    .map(|seq| receipt(c, r::VersionRef { deposit, file, seq }))
                    .transpose()?,
            },
        );
    }
    Ok((view, !hidden))
}

impl r::RepositoryRead for MetadataStore {
    fn snapshot(&self, id: r::DepositId) -> r::Result<(r::Deposit, bool)> {
        self.0.readers().read(|c| {
            let tx = c.unchecked_transaction().map_err(db)?;
            snapshot(&tx, id)
        })
    }
    fn receipt(&self, v: r::VersionRef) -> r::Result<r::UploadReceipt> {
        self.0.readers().read(|c| receipt(c, v))
    }
    fn references(&self, hash: r::ObjectHash) -> r::Result<Vec<r::VersionRef>> {
        self.0.readers().read(|connection| {
            let mut query = connection
                .prepare(
                    "SELECT f.deposit_id, v.file_id, v.seq FROM repo_versions v
                 JOIN repo_files f ON f.id=v.file_id WHERE v.object_hash=?1
                 ORDER BY f.deposit_id,v.file_id,v.seq",
                )
                .map_err(db)?;
            let rows = query
                .query_map([hash.0], |row| {
                    Ok(r::VersionRef {
                        deposit: r::DepositId(id(row, 0)?),
                        file: r::FileId(id(row, 1)?),
                        seq: unsigned(row.get(2)?)?,
                    })
                })
                .map_err(db)?;
            rows.collect::<rusqlite::Result<_>>().map_err(db)
        })
    }
    fn ensure_deletable(&self, instance: r::InstanceId) -> r::Result<()> {
        self.0.readers().read(|connection| {
            let linked: bool = connection
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM repo_deposits WHERE instance_id=i.id)
                 FROM instances i WHERE i.id=?1",
                    [instance.0],
                    |row| row.get(0),
                )
                .optional()
                .map_err(db)?
                .ok_or(r::RepositoryError::InstanceNotFound)?;
            if linked {
                Err(r::RepositoryError::InstanceHasDeposit)
            } else {
                Ok(())
            }
        })
    }
}

impl MetadataStore {
    /// Whether any version, including withdrawn or superseded versions, retains this hash.
    ///
    /// # Errors
    /// The metadata database cannot be queried.
    pub fn retains(&self, hash: r::ObjectHash) -> r::Result<bool> {
        self.0.readers().read(|c| {
            c.query_row(
                "SELECT EXISTS(SELECT 1 FROM repo_versions WHERE object_hash=?1)",
                [hash.0],
                |row| row.get(0),
            )
            .map_err(db)
        })
    }
}
