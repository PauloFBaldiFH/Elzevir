// SPDX-License-Identifier: GPL-3.0-or-later
//! Snapshot artifacts never open another writer on the active database.

use elzevir_store::{ElzevirHandler, ElzevirStore, StoreConfig, inspect_snapshot};

#[test]
fn online_snapshot_is_consistent_and_never_replaces_existing_files() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("live.db");
    let store = ElzevirStore::open(&source, &StoreConfig::default(), ElzevirHandler).unwrap();
    let image = root.path().join("snapshot.db");
    let report = store.readers().snapshot(&image).unwrap();
    assert!(report.schema_version > 0);
    assert_eq!(
        inspect_snapshot(&image).unwrap().schema_version,
        report.schema_version
    );
    let bytes = std::fs::read(&image).unwrap();
    assert!(store.readers().snapshot(&image).is_err());
    assert_eq!(std::fs::read(&image).unwrap(), bytes);
    assert!(store.readers().snapshot(&source).is_err());
    assert!(!image.with_extension("db-wal").exists());
    store.shutdown().unwrap();
}

#[test]
fn verification_refuses_foreign_images_without_modifying_them() {
    let root = tempfile::tempdir().unwrap();
    let image = root.path().join("foreign.db");
    std::fs::write(&image, "user content").unwrap();
    assert!(inspect_snapshot(&image).is_err());
    assert_eq!(std::fs::read_to_string(&image).unwrap(), "user content");
}

#[test]
fn interrupted_snapshot_leaves_no_image_and_the_live_pool_remains_usable() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("live.db");
    let store = ElzevirStore::open(&source, &StoreConfig::default(), ElzevirHandler).unwrap();
    let image = root.path().join("cancelled.db");
    assert!(
        store
            .readers()
            .snapshot_controlled(&image, std::sync::Arc::new(|| true))
            .is_err()
    );
    assert!(!image.exists());
    store
        .readers()
        .snapshot(&root.path().join("valid.db"))
        .unwrap();
    store.shutdown().unwrap();
}

#[test]
fn online_snapshot_reads_committed_data_during_a_wal_write() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("live.db");
    let store = ElzevirStore::open(&source, &StoreConfig::default(), ElzevirHandler).unwrap();
    let mut writer = rusqlite::Connection::open(source).unwrap();
    assert_eq!(
        writer
            .query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))
            .unwrap(),
        "wal"
    );
    writer
        .execute_batch("CREATE TABLE fixture(n INTEGER); INSERT INTO fixture VALUES(1)")
        .unwrap();
    let tx = writer.transaction().unwrap();
    tx.execute("UPDATE fixture SET n=2", []).unwrap();
    let image = root.path().join("snapshot.db");
    store.readers().snapshot(&image).unwrap();
    tx.commit().unwrap();
    let copy = rusqlite::Connection::open(image).unwrap();
    assert_eq!(
        copy.query_row("SELECT n FROM fixture", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        writer
            .query_row("SELECT n FROM fixture", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        2
    );
    store.shutdown().unwrap();
}
