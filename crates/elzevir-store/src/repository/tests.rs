// SPDX-License-Identifier: GPL-3.0-or-later
use super::{MetadataStore, r};
use elzevir_app::staff::{Permission, StaffId, StaffIdentity};
use r::{RepositoryError as E, RepositoryRead, RepositoryWrite};
use rusqlite::Connection;
use std::{sync::Barrier, thread};
const D: r::DepositId = r::DepositId(u128::MAX);
const I: r::InstanceId = r::InstanceId(1);
const F: r::FileId = r::FileId(100);
fn actor() -> r::Operator {
    r::Operator::try_from(&StaffIdentity {
        id: StaffId(2),
        login: "test".into(),
        permissions: [Permission::Cataloguing, Permission::StaffInterface].into(),
    })
    .unwrap()
}
fn fixture() -> (tempfile::TempDir, MetadataStore, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("elzevir.db");
    let store = MetadataStore::open(&path).unwrap();
    let c = Connection::open(path).unwrap();
    c.execute_batch("PRAGMA foreign_keys=ON; INSERT INTO works(id,title) VALUES(1,'Thesis'); INSERT INTO instances(id,work_id,title) VALUES (1,1,'Thesis');").unwrap();
    (dir, store, c)
}
fn add_primary(store: &MetadataStore) {
    store.create_draft(actor(), 1, D, I).unwrap();
    store
        .edit(
            actor(),
            2,
            D,
            r::Edit::AddFile(r::FileDescription {
                id: F,
                role: r::FileRole::Primary,
                access: r::FileAccess::Public,
                label: "Thesis".into(),
            }),
        )
        .unwrap();
}
fn object(byte: u8) -> r::ObjectInfo {
    r::ObjectInfo {
        hash: r::ObjectHash([byte; 32]),
        bytes: 5,
    }
}
fn upload(op: u128, seq: Option<u64>) -> r::Upload {
    r::Upload {
        operation: r::OperationId(op),
        deposit: D,
        file: F,
        expected_current_seq: seq,
        display_name: "Thesis.pdf".into(),
        media_type: "application/pdf".into(),
    }
}
fn count(c: &Connection, query: &str) -> i64 {
    c.query_row(query, [], |r| r.get(0)).unwrap()
}

#[test]
fn wal_schema_retention_and_memberships() {
    let (_dir, store, c) = fixture();
    assert_eq!(
        c.query_row("PRAGMA journal_mode", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "wal"
    );
    assert_eq!(
        count(
            &c,
            "SELECT count(*) FROM pragma_table_list WHERE name GLOB 'repo_*' AND strict=1"
        ),
        6
    );
    store.ensure_deletable(I).unwrap();
    assert_eq!(
        store.create_draft(actor(), 0, D, r::InstanceId(99)),
        Err(E::InstanceNotFound)
    );
    add_primary(&store);
    assert_eq!(
        store.create_draft(actor(), 0, r::DepositId(2), I),
        Err(E::DepositAlreadyExists)
    );
    assert_eq!(store.ensure_deletable(I), Err(E::InstanceHasDeposit));
    assert_eq!(
        c.execute("DELETE FROM instances WHERE id=1", [])
            .unwrap_err()
            .to_string(),
        "instance_has_deposit"
    );
    for id in [r::DigitalCollectionId(1), r::DigitalCollectionId(2)] {
        assert_eq!(
            store.edit(actor(), 2, D, r::Edit::Join(id)),
            Err(E::NotVisible)
        );
        store
            .create_collection(actor(), 2, id, "Theses".into())
            .unwrap();
        store.edit(actor(), 2, D, r::Edit::Join(id)).unwrap();
    }
    assert_eq!(store.snapshot(D).unwrap().0.collections.len(), 2);
    c.execute(
        "DELETE FROM repo_collections WHERE id=?1",
        [1u128.to_be_bytes()],
    )
    .unwrap();
    assert_eq!(store.snapshot(D).unwrap().0.collections.len(), 1);
    let receipt = store
        .commit_version(actor(), 3, upload(1, None), object(1))
        .unwrap();
    assert!(c.execute("DELETE FROM repo_objects", []).is_err());
    assert!(
        c.execute("UPDATE repo_versions SET display_name='bad'", [])
            .is_err()
    );
    assert!(c.execute("DELETE FROM repo_versions", []).is_err());
    store
        .edit(actor(), 4, D, r::Edit::Publish(receipt.version))
        .unwrap();
    c.execute("UPDATE instances SET opac_hidden=1 WHERE id=1", [])
        .unwrap();
    assert!(!store.snapshot(D).unwrap().1);
    store
        .edit(actor(), 5, D, r::Edit::Retire(receipt.version, true))
        .unwrap();
    assert!(store.snapshot(D).unwrap().0.primary().is_err());
    assert!(store.retains(receipt.object.hash).unwrap());
    assert_eq!(store.receipt(receipt.version).unwrap(), receipt);
    assert_eq!(
        count(&c, "SELECT count(*) FROM pragma_foreign_key_check"),
        0
    );
}
#[test]
fn stale_competitor_and_replay_have_no_audit_or_outbox_side_effects() {
    let (dir, store, c) = fixture();
    add_primary(&store);
    let first = store
        .commit_version(actor(), 3, upload(1, None), object(1))
        .unwrap();
    store
        .edit(actor(), 4, D, r::Edit::Publish(first.version))
        .unwrap();
    let gate = Barrier::new(2);
    let results = thread::scope(|scope| {
        let submit = |op| {
            gate.wait();
            store.commit_version(actor(), 5, upload(op, Some(1)), object(2))
        };
        let other = scope.spawn(move || submit(2));
        [submit(3), other.join().unwrap()]
    });
    assert_eq!(
        results
            .iter()
            .filter(|result| **result == Err(E::VersionConflict))
            .count(),
        1
    );
    let winner = results.into_iter().find_map(Result::ok).unwrap();
    assert_eq!(winner.version.seq, 2);
    drop(store);
    let store = MetadataStore::open(&dir.path().join("elzevir.db")).unwrap();
    assert_eq!(
        store.commit_version(actor(), 99, winner.upload.clone(), object(2)),
        Ok(winner.clone())
    );
    assert_eq!(
        store.commit_version(actor(), 99, winner.upload.clone(), object(9)),
        Err(E::OperationConflict)
    );
    let changed = r::Upload {
        display_name: "different".into(),
        ..winner.upload
    };
    assert_eq!(
        store.commit_version(actor(), 99, changed, object(2)),
        Err(E::OperationConflict)
    );
    assert_eq!(count(&c, "SELECT count(*) FROM repo_versions"), 2);
    assert_eq!(
        count(
            &c,
            "SELECT count(*) FROM audit_log WHERE kind='repository.version' AND actor=2"
        ),
        2
    );
    assert_eq!(
        count(
            &c,
            "SELECT count(*) FROM effect_outbox WHERE json_extract(payload,'$.instance')=1"
        ),
        2
    );
}
#[test]
fn writer_failure_rolls_back_object_version_head_audit_and_effect() {
    let (_dir, store, c) = fixture();
    add_primary(&store);
    let first = store
        .commit_version(actor(), 3, upload(1, None), object(1))
        .unwrap();
    store
        .edit(actor(), 4, D, r::Edit::Publish(first.version))
        .unwrap();
    c.execute_batch("CREATE TRIGGER fail_effect BEFORE INSERT ON effect_outbox BEGIN SELECT RAISE(ABORT,'injected failure'); END;").unwrap();
    assert_eq!(
        store.commit_version(actor(), 5, upload(2, Some(1)), object(2)),
        Err(E::Unavailable)
    );
    assert_eq!(store.snapshot(D).unwrap().0.primary().unwrap(), &first);
    assert_eq!(count(&c, "SELECT count(*) FROM repo_objects"), 1);
    assert_eq!(count(&c, "SELECT count(*) FROM repo_versions"), 1);
    assert_eq!(count(&c, "SELECT count(*) FROM effect_outbox"), 1);
    assert_eq!(
        count(
            &c,
            "SELECT count(*) FROM audit_log WHERE kind='repository.version'"
        ),
        1
    );
    c.execute_batch("DROP TRIGGER fail_effect").unwrap();
    assert_eq!(
        store
            .commit_version(actor(), 6, upload(2, Some(1)), object(2))
            .unwrap()
            .version
            .seq,
        2
    );
}

#[test]
fn upgrading_version_61_keeps_the_existing_instance_and_starts_without_deposits() {
    let (directory, store, connection) = fixture();
    drop(store);
    connection
        .execute_batch(include_str!("../../tests/data/rewind_repository.sql"))
        .unwrap();
    connection.execute_batch("PRAGMA user_version=61").unwrap();
    let store = MetadataStore::open(&directory.path().join("elzevir.db")).unwrap();
    assert_eq!(count(&connection, "SELECT count(*) FROM repo_deposits"), 0);
    assert_eq!(
        count(
            &connection,
            "SELECT count(*) FROM instances WHERE title='Thesis'"
        ),
        1
    );
    store.create_draft(actor(), 1, D, I).unwrap();
    assert_eq!(store.snapshot(D).unwrap().0.instance, I);
}

#[test]
fn staff_verification_only_reindexes_a_current_public_link_when_health_changes() {
    let (_dir, store, c) = fixture();
    add_primary(&store);
    let first = store
        .commit_version(actor(), 1, upload(1, None), object(1))
        .unwrap();
    store
        .edit(actor(), 2, D, r::Edit::Publish(first.version))
        .unwrap();
    for status in [
        r::Integrity::Corrupt,
        r::Integrity::Corrupt,
        r::Integrity::Ok,
    ] {
        store
            .record_integrity(actor(), 3, first.version, status)
            .unwrap();
    }
    assert_eq!(
        count(
            &c,
            "SELECT count(*) FROM audit_log WHERE kind='repository.verify'"
        ),
        3
    );
    assert_eq!(count(&c, "SELECT count(*) FROM effect_outbox"), 3);
    let current = store
        .commit_version(actor(), 4, upload(2, Some(1)), object(2))
        .unwrap();
    store
        .record_integrity(actor(), 5, first.version, r::Integrity::Missing)
        .unwrap();
    assert_eq!(count(&c, "SELECT count(*) FROM effect_outbox"), 4);
    assert!(
        c.execute("UPDATE repo_files SET current_seq=1", [])
            .is_err()
    );
    store
        .edit(actor(), 6, D, r::Edit::Retire(current.version, true))
        .unwrap();
    assert!(
        c.execute("UPDATE repo_files SET current_seq=2", [])
            .is_err()
    );
}
