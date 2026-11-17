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
