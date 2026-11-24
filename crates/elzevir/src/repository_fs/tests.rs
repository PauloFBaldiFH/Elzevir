// SPDX-License-Identifier: GPL-3.0-or-later
use super::*;
use elzevir_app::{
    catalog::{CatalogCommand, WorkChange},
    commands::WriteCommand,
    creating::new_record,
    repository::{self as r, RepositoryRead, RepositoryWrite},
    staff::{Permission, StaffId, StaffIdentity},
};

use elzevir_store::repository::MetadataStore;
use std::time::{Duration, UNIX_EPOCH};
fn store(path: &Path, max: u64, floor: u64) -> FsObjectStore {
    FsObjectStore::new(path, crate::lock_data(path).unwrap(), max, floor).unwrap()
}
fn path_of(store: &FsObjectStore, h: ObjectHash) -> PathBuf {
    let (a, b) = shards(h);
    store.objects.join(a).join(b).join(h.to_string())
}

#[test]
fn chunked_install_limits_and_verified_no_clobber() {
    let dir = tempfile::tempdir().unwrap();
    let mut objects = store(dir.path(), CHUNK as u64 + 1, 0);
    let data = vec![99; CHUNK + 1];
    let installed = objects.install(&mut data.as_slice(), None, 0).unwrap();
    let info = installed.info();
    assert_eq!(info.hash, ObjectHash(Sha256::digest(&data).into()));
    assert_eq!(
        objects.install(&mut data.as_slice(), None, 0).err(),
        Some(Error::UploadBusy)
    );
    drop(installed);
    objects
        .install(&mut data.as_slice(), Some(info.hash), 0)
        .unwrap();
    let mut bytes = Vec::new();
    objects.open(info).unwrap().read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, data);
    assert_eq!(
        objects
            .install(&mut data.as_slice(), Some(ObjectHash([0; 32])), 0)
            .err(),
        Some(Error::ChecksumMismatch)
    );
    objects.max_bytes -= 1;
    assert_eq!(
        objects.install(&mut data.as_slice(), None, 0).err(),
        Some(Error::TooLarge)
    );
    objects.min_free = u64::MAX;
    assert_eq!(
        objects.install(&mut &b"small"[..], None, 0).err(),
        Some(Error::InsufficientSpace)
    );
    assert_eq!(objects.staging.read_dir().unwrap().count(), 0);

    objects.max_bytes = MAX_BYTES;
    objects.min_free = 0;
    let path = path_of(&objects, info.hash);
    let damaged = vec![88; data.len()];
    fs::write(&path, &damaged).unwrap();
    assert_eq!(objects.integrity(info), Integrity::Corrupt);
    assert_eq!(objects.open(info).err(), Some(Error::ObjectUnavailable));
    assert_eq!(
        objects.install(&mut data.as_slice(), None, 0).err(),
        Some(Error::ObjectUnavailable)
    );
    assert_eq!(fs::read(&path).unwrap(), damaged);
    fs::remove_file(&path).unwrap();
    assert_eq!(objects.integrity(info), Integrity::Missing);
}

#[test]
fn observed_grace_survives_reopens_and_active_transfers_exclude_maintenance() {
    const DAY: i64 = 86_400;
    let dir = tempfile::tempdir().unwrap();
    let meta = MetadataStore::open(&dir.path().join("elzevir.db")).unwrap();
    let mut fs = store(dir.path(), 100, 0);
    let pending = fs.install(&mut &b"orphan"[..], None, 0).unwrap();
    let info = pending.info();
    assert_eq!(fs.maintain(&meta, DAY, true).err(), Some(Error::UploadBusy));
    drop(pending); // installed durably, but process never submitted metadata
    let reader = fs.open(info).unwrap();
    assert_eq!(fs.maintain(&meta, DAY, true).err(), Some(Error::UploadBusy));
    drop(fs);
    assert!(crate::backup_cli::lock(dir.path()).is_err());
    drop(reader);
    let mut fs = store(dir.path(), 100, 0);
    let tmp = fs.staging.join("upload-interrupted");
    fs::write(&tmp, b"tmp").unwrap();
    File::options()
        .write(true)
        .open(&tmp)
        .unwrap()
        .set_modified(UNIX_EPOCH + Duration::from_secs(2))
        .unwrap();
    assert_eq!(fs.maintain(&meta, DAY + 1, false).unwrap().temporary, 0);
    let dry = fs.maintain(&meta, DAY + 2, false).unwrap();
    assert_eq!((dry.temporary, dry.observed, dry.bytes), (1, 1, 3));
    assert!(tmp.exists());
    assert_eq!(fs.inventory.read_dir().unwrap().count(), 0);
    let start = 10 * DAY;
    let record = fs.maintain(&meta, start, true).unwrap();
    assert_eq!(
        (record.temporary, record.observed, record.orphan),
        (1, 1, 0)
    );
    assert!(!tmp.exists());
    drop(fs);
    let mut fs = store(dir.path(), 100, 0);
    assert_eq!(
        fs.maintain(&meta, start + 7 * DAY - 1, true)
            .unwrap()
            .orphan,
        0
    );
    let dry = fs.maintain(&meta, start + 7 * DAY, false).unwrap();
    assert_eq!((dry.orphan, dry.bytes), (1, 6));
    assert_eq!(fs.integrity(info), Integrity::Ok);
    // Reinstallation cancels the prior observation even if the next commit is also lost.
    fs.install(&mut &b"orphan"[..], None, 0).unwrap();
    let reapplied = fs.maintain(&meta, start + 7 * DAY, true).unwrap();
    assert_eq!((reapplied.observed, reapplied.orphan), (1, 0));
    assert_eq!(
        fs.maintain(&meta, start + 14 * DAY, true).unwrap().orphan,
        1
    );
    assert_eq!(fs.integrity(info), Integrity::Missing);
}

#[test]
fn a_retained_backup_pins_bytes_across_a_catalog_restore() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let path = d.join("elzevir.db");
    catalog(d);
    let earlier = d.join("earlier.sqlite");
    elzevir_store::snapshot_offline(&path, &earlier).unwrap();
    let mut fs = store(d, 1024, 0);
    let metadata = MetadataStore::open(&path).unwrap();
    let actor = operator();
    let deposit = r::DepositId(1);
    let file = r::FileId(2);
    draft(&metadata, actor, deposit, file);
    let installed = fs.install(&mut &b"thesis"[..], None, 1).unwrap();
    let info = installed.info();
    metadata
        .commit_version(
            actor,
            2,
            r::Upload {
                operation: r::OperationId(3),
                deposit,
                file,
                expected_current_seq: None,
                display_name: "thesis.pdf".into(),
                media_type: "application/pdf".into(),
            },
            info,
        )
        .unwrap();
    drop(installed);
    drop(metadata);
    crate::backup_cli::make_directory(&d.join("backups")).unwrap();
    let archive = crate::backup_cli::artifact(d, "00000000000000000000000000000042").unwrap();
    crate::backup_archive::create(d, "config", &archive, None).unwrap();
    let expired = crate::backup_cli::artifact(d, "00000000000000000000000000000043").unwrap();
    crate::backup_archive::create(d, "config", &expired, None).unwrap();
    fs::remove_file(expired).unwrap();
    {
        let metadata = MetadataStore::open(&path).unwrap();
        fs.maintain(&metadata, 3, true).unwrap();
        let pins = elzevir_store::repository::backup::BackupPins::open(
            &fs.inventory.join("backup-pins.sqlite"),
            false,
        )
        .unwrap();
        assert_eq!(pins.archives(info.hash).unwrap().len(), 1);
    }
    // The earlier closed WAL snapshot has no repository links.
    fs::copy(earlier, &path).unwrap();
    let metadata = MetadataStore::open(&path).unwrap();
    assert!(!metadata.retains(info.hash).unwrap());
    assert_eq!(fs.maintain(&metadata, 0, true).unwrap().observed, 0);
    assert_eq!(fs.maintain(&metadata, 10 * 86_400, true).unwrap().orphan, 0);
    fs::remove_file(archive).unwrap();
    assert_eq!(
        fs.maintain(&metadata, 11 * 86_400, true).unwrap().observed,
        1
    );
    assert_eq!(fs.maintain(&metadata, 18 * 86_400, true).unwrap().orphan, 1);
}

fn catalog(d: &Path) {
    let data = crate::open_store(d).unwrap();
    let record = new_record(vec![WorkChange::Title("Thesis".into())], vec![]).unwrap();
    data.store
        .submit(WriteCommand::Catalog(CatalogCommand::AddRecord(Box::new(
            record,
        ))))
        .unwrap()
        .wait()
        .unwrap();
    drop(data);
}
fn operator() -> r::Operator {
    r::Operator::try_from(&StaffIdentity {
        id: StaffId(0),
        login: "test".into(),
        permissions: [Permission::StaffInterface, Permission::Cataloguing].into(),
    })
    .unwrap()
}
fn draft(metadata: &MetadataStore, actor: r::Operator, deposit: r::DepositId, file: r::FileId) {
    metadata
        .create_draft(actor, 0, deposit, r::InstanceId(1))
        .unwrap();
    metadata
        .edit(
            actor,
            1,
            deposit,
            r::Edit::AddFile(r::FileDescription {
                id: file,
                label: "PDF".into(),
                role: r::FileRole::Primary,
                access: r::FileAccess::Public,
            }),
        )
        .unwrap();
}

#[test]
fn killed_between_install_commit_and_reply_can_be_retried() {
    use std::{
        process::{Command, Stdio},
        thread,
        time::Instant,
    };

    let actor = operator();
    let deposit = r::DepositId(20);
    let file = r::FileId(21);
    let upload = r::Upload {
        deposit,
        file,
        operation: r::OperationId(2),
        expected_current_seq: Some(1),
        display_name: "thesis.pdf".into(),
        media_type: "application/pdf".into(),
    };
    if let Some(directory) = std::env::var_os(CRASH_DIRECTORY) {
        crash_child(&directory, upload);
    }
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    catalog(d);
    let path = d.join("elzevir.db");
    {
        let fs = store(d, 128, 0);
        let metadata = MetadataStore::open(&path).unwrap();
        draft(&metadata, actor, deposit, file);
        let original = fs.install(&mut &b"original"[..], None, 1).unwrap();
        metadata
            .commit_version(
                actor,
                1,
                r::Upload {
                    operation: r::OperationId(1),
                    expected_current_seq: None,
                    ..upload.clone()
                },
                original.info(),
            )
            .unwrap();
    }
    for phase in ["before", "after"] {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "repository_fs::tests::killed_between_install_commit_and_reply_can_be_retried",
            ])
            .env(CRASH_DIRECTORY, d)
            .env(CRASH_PHASE, phase)
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        while !d.join(phase).exists() {
            assert!(child.try_wait().unwrap().is_none());
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(10));
        }
        assert!(crate::backup_cli::lock(d).is_err());
        child.kill().unwrap();
        child.wait().unwrap();
        let mut fs = store(d, 128, 0);
        let metadata = MetadataStore::open(&path).unwrap();
        let head = metadata
            .snapshot(deposit)
            .unwrap()
            .0
            .primary()
            .unwrap()
            .clone();
        assert_eq!(fs.integrity(head.object), Integrity::Ok);
        if phase == "before" {
            assert_eq!(head.version.seq, 1);
            assert_eq!(fs.maintain(&metadata, 3, true).unwrap().observed, 1);
        } else {
            assert_eq!(head.version.seq, 2);
            assert_eq!(fs.maintain(&metadata, 4, true).unwrap().observed, 0);
            let guard = fs.install(&mut &b"replacement"[..], None, 5).unwrap();
            assert_eq!(
                metadata
                    .commit_version(actor, 5, upload.clone(), guard.info())
                    .unwrap(),
                head
            );
        }
    }
}

const CRASH_DIRECTORY: &str = "ELZEVIR_TEST_REPOSITORY_CRASH_DIR";
const CRASH_PHASE: &str = "ELZEVIR_TEST_REPOSITORY_CRASH_PHASE";
fn crash_child(directory: &std::ffi::OsStr, upload: r::Upload) -> ! {
    let directory = Path::new(directory);
    let fs = store(directory, 128, 0);
    let metadata = MetadataStore::open(&directory.join("elzevir.db")).unwrap();
    let guard = fs.install(&mut &b"replacement"[..], None, 2).unwrap();
    let phase = std::env::var(CRASH_PHASE).unwrap();
    if phase == "after" {
        metadata
            .commit_version(operator(), 2, upload, guard.info())
            .unwrap();
    }
    // A test-only rendezvous, leaving all live handles for the OS to close.
    fs::write(directory.join(phase), b"ready").unwrap();
    loop {
        std::thread::park();
    }
}

#[test]
fn backup_server_and_repository_contend_for_the_same_institution() {
    let dir = tempfile::tempdir().unwrap();
    let server = crate::open_store(dir.path()).unwrap();
    assert!(crate::backup_cli::lock(dir.path()).is_err());
    drop(server);
    let backup = crate::backup_cli::lock(dir.path()).unwrap();
    assert!(crate::open_store(dir.path()).is_err());
    assert!(crate::backup_cli::lock(dir.path()).is_err());
    drop(backup);
    let repository = store(dir.path(), 100, 0);
    assert!(crate::open_store(dir.path()).is_err());
    drop(repository);
    crate::open_store(dir.path()).unwrap();
}
