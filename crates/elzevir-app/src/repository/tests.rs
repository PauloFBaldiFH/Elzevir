// SPDX-License-Identifier: GPL-3.0-or-later
use super::*;

#[test]
fn hash_uses_canonical_hex() {
    let hash = ObjectHash([0xab; 32]);
    let text = "ab".repeat(32);
    assert_eq!(hash.to_string(), text);
    assert_eq!(text.parse(), Ok(hash));
    assert!(text.to_uppercase().parse::<ObjectHash>().is_err());
    assert!("ab".parse::<ObjectHash>().is_err());
}

use super::memory::{Activity, Crash, MemoryMetadata, MemoryObjects, TestClock};
use crate::staff::{Permission, StaffId, StaffIdentity};
use std::io::{self, Read};

type Pilot = Repository<MemoryMetadata, MemoryObjects, TestClock>;
const THESIS: DepositId = DepositId(17);
const INSTANCE: InstanceId = InstanceId(91);
const PDF: FileId = FileId(1);
const SHEET: FileId = FileId(2);
const FORM: FileId = FileId(3);
const PUBLIC: ReadAudience = ReadAudience::Public;
const DAY: i64 = 86_400;

fn operator() -> Operator {
    Operator::try_from(&StaffIdentity {
        id: StaffId(7),
        login: "cataloguer".into(),
        permissions: [Permission::StaffInterface, Permission::Cataloguing].into(),
    })
    .unwrap()
}
fn setup() -> Pilot {
    let repo = Repository {
        metadata: MemoryMetadata::default(),
        objects: MemoryObjects::default(),
        clock: TestClock::default(),
    };
    repo.clock.0.set(100);
    repo.objects.max_bytes.set(1024);
    repo.metadata.instance(INSTANCE, true);
    repo.create_draft(operator(), THESIS, INSTANCE).unwrap();
    for id in [DigitalCollectionId(5), DigitalCollectionId(6)] {
        repo.metadata.collection(id);
        repo.associate_collection(operator(), THESIS, id).unwrap();
    }
    for (id, label, role, access) in [
        (PDF, "Thesis", FileRole::Primary, FileAccess::Public),
        (SHEET, "Data", FileRole::Supplement, FileAccess::Public),
        (
            FORM,
            "Authorization",
            FileRole::Administrative,
            FileAccess::Staff,
        ),
    ] {
        repo.add_file(
            operator(),
            THESIS,
            FileDescription {
                id,
                label: label.into(),
                role,
                access,
            },
        )
        .unwrap();
    }
    repo
}
fn request(file: FileId, operation: u128, seq: Option<u64>) -> Upload {
    let (name, media_type) = if file == SHEET {
        ("data.csv", "text/csv")
    } else {
        ("document.pdf", "application/pdf")
    };
    Upload {
        operation: OperationId(operation),
        deposit: THESIS,
        file,
        expected_current_seq: seq,
        display_name: name.into(),
        media_type: media_type.into(),
    }
}
fn send(
    r: &Pilot,
    file: FileId,
    key: u128,
    expected: Option<u64>,
    mut bytes: &[u8],
) -> Result<UploadReceipt> {
    r.upload(operator(), request(file, key, expected), &mut bytes, None)
}
fn published() -> Pilot {
    let r = setup();
    send(&r, PDF, 1, None, b"shared PDF").unwrap();
    send(&r, SHEET, 2, None, b"1,2").unwrap();
    send(&r, FORM, 3, None, b"shared PDF").unwrap();
    r.publish(operator(), THESIS).unwrap();
    r
}
fn read(r: &Pilot, file: FileId, seq: Option<u64>, audience: ReadAudience) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    r.download(THESIS, file, seq, audience)?
        .reader
        .read_to_end(&mut bytes)
        .unwrap();
    Ok(bytes)
}
#[test]
fn draft_identity_membership_and_publication_preconditions() {
    let r = setup();
    let actor = operator();
    assert_eq!(
        r.create_draft(actor, DepositId(8), InstanceId(0)),
        Err(RepositoryError::InstanceNotFound)
    );
    assert_eq!(
        r.create_draft(actor, DepositId(8), INSTANCE),
        Err(RepositoryError::DepositAlreadyExists)
    );
    assert_eq!(
        r.associate_collection(actor, THESIS, DigitalCollectionId(42)),
        Err(RepositoryError::NotVisible)
    );
    let collection = DigitalCollectionId(5);
    r.associate_collection(actor, THESIS, collection).unwrap();
    r.metadata.instance(InstanceId(92), true);
    r.create_draft(actor, DepositId(18), InstanceId(92))
        .unwrap();
    r.associate_collection(actor, DepositId(18), collection)
        .unwrap();
    assert_eq!(
        r.metadata.ensure_deletable(INSTANCE),
        Err(RepositoryError::InstanceHasDeposit)
    );
    assert_eq!(
        r.page(THESIS, PUBLIC).unwrap_err(),
        RepositoryError::NotVisible
    );
    assert_eq!(r.publish(actor, THESIS), Err(RepositoryError::InvalidState));
    for (role, access) in [
        (FileRole::Administrative, FileAccess::Public),
        (FileRole::Primary, FileAccess::Public),
    ] {
        assert_eq!(
            r.add_file(
                actor,
                THESIS,
                FileDescription {
                    id: FileId(99),
                    label: "other".into(),
                    role,
                    access
                }
            ),
            Err(RepositoryError::InvalidState)
        );
    }
    let primary = send(&r, PDF, 1, None, b"primary").unwrap();
    r.metadata.instance(INSTANCE, false);
    assert_eq!(r.publish(actor, THESIS), Err(RepositoryError::InvalidState));
    r.metadata.instance(INSTANCE, true);
    r.objects.corrupt(primary.object.hash);
    assert_eq!(
        r.publish(actor, THESIS),
        Err(RepositoryError::ObjectUnavailable)
    );
    assert_eq!(r.metadata.counts().2, 0);
}

#[test]
fn two_collections_and_shared_bytes_never_grant_access_to_the_form() {
    let r = published();
    let page = r.page(THESIS, PUBLIC).unwrap();
    assert_eq!(
        (page.id, page.instance, page.collections.len()),
        (THESIS, INSTANCE, 2)
    );
    let files: Vec<_> = page.files.iter().map(|(file, _)| file.id).collect();
    assert_eq!(files, [PDF, SHEET]);
    assert_eq!(r.objects.counts(), (0, 2));
    assert_eq!(read(&r, PDF, None, PUBLIC).unwrap(), b"shared PDF");
    for seq in [None, Some(1)] {
        assert_eq!(
            read(&r, FORM, seq, PUBLIC),
            Err(RepositoryError::NotVisible)
        );
        assert_eq!(
            read(&r, FORM, seq, ReadAudience::Staff(operator())).unwrap(),
            b"shared PDF"
        );
    }
    assert_eq!(r.metadata.counts().2, 1);
    send(&r, FORM, 4, Some(1), b"restricted v2").unwrap();
    assert_eq!(r.metadata.counts().2, 1);
    for perms in [
        vec![Permission::StaffInterface],
        vec![Permission::Cataloguing],
    ] {
        let staff = StaffIdentity {
            id: StaffId(2),
            login: String::new(),
            permissions: perms.into_iter().collect(),
        };
        assert_eq!(
            Operator::try_from(&staff),
            Err(RepositoryError::PermissionDenied)
        );
    }
}

#[test]
fn replacement_and_retirement_preserve_exact_history_without_fallback() {
    let r = published();
    let v2 = send(&r, PDF, 4, Some(1), b"revision").unwrap();
    assert_eq!(r.page(THESIS, PUBLIC).unwrap().id, THESIS);
    assert_eq!(
        read(&r, PDF, Some(1), PUBLIC),
        Err(RepositoryError::NotVisible)
    );
    assert_eq!(read(&r, PDF, None, PUBLIC).unwrap(), b"revision");
    assert_eq!(read(&r, SHEET, Some(1), PUBLIC).unwrap(), b"1,2");
    assert_eq!(
        read(&r, PDF, Some(1), ReadAudience::Staff(operator())).unwrap(),
        b"shared PDF"
    );
    assert_eq!(
        r.withdraw_version(operator(), v2.version, false),
        Err(RepositoryError::InvalidState)
    );
    let sheet = VersionRef {
        deposit: THESIS,
        file: SHEET,
        seq: 1,
    };
    r.withdraw_version(operator(), sheet, false).unwrap();
    assert_eq!(
        read(&r, SHEET, None, PUBLIC),
        Err(RepositoryError::NotVisible)
    );
    assert_eq!(
        read(&r, SHEET, Some(1), ReadAudience::Staff(operator())).unwrap(),
        b"1,2"
    );
    assert_eq!(send(&r, SHEET, 5, None, b"2,3").unwrap().version.seq, 2);
    r.withdraw_version(operator(), v2.version, true).unwrap();
    assert_eq!(
        r.publish(operator(), THESIS),
        Err(RepositoryError::InvalidState)
    );
    assert_eq!(
        read(&r, PDF, None, ReadAudience::Staff(operator())),
        Err(RepositoryError::NotVisible)
    );
    assert_eq!(
        read(&r, PDF, Some(2), ReadAudience::Staff(operator())).unwrap(),
        b"revision"
    );
}

#[test]
fn tombstone_only_for_public_page_and_hidden_instance_has_precedence() {
    let r = published();
    assert_eq!(
        r.metadata.ensure_deletable(INSTANCE),
        Err(RepositoryError::InstanceHasDeposit)
    );
    r.metadata.instance(INSTANCE, false);
    assert_eq!(
        r.page(THESIS, PUBLIC).unwrap_err(),
        RepositoryError::NotVisible
    );
    assert_eq!(
        read(&r, PDF, None, PUBLIC),
        Err(RepositoryError::NotVisible)
    );
    r.metadata.instance(INSTANCE, true);
    r.withdraw(operator(), THESIS).unwrap();
    assert_eq!(
        r.page(THESIS, PUBLIC).unwrap_err(),
        RepositoryError::Withdrawn
    );
    assert_eq!(
        read(&r, PDF, Some(1), PUBLIC),
        Err(RepositoryError::NotVisible)
    );
    assert_eq!(
        r.metadata.ensure_deletable(INSTANCE),
        Err(RepositoryError::InstanceHasDeposit)
    );
    r.metadata.instance(INSTANCE, false);
    assert_eq!(
        r.page(THESIS, PUBLIC).unwrap_err(),
        RepositoryError::NotVisible
    );
}

#[test]
fn lost_acknowledgement_replays_the_original_receipt_even_after_another_commit() {
    let r = published();
    let before = r.metadata.counts();
    r.metadata.crash(Crash::AfterCommit);
    assert_eq!(
        send(&r, PDF, 4, Some(1), b"second"),
        Err(RepositoryError::CommitUnknown)
    );
    let original = r
        .metadata
        .receipt(VersionRef {
            deposit: THESIS,
            file: PDF,
            seq: 2,
        })
        .unwrap();
    r.clock.0.set(200);
    send(&r, PDF, 5, Some(2), b"third").unwrap();
    assert_eq!(send(&r, PDF, 4, Some(1), b"second").unwrap(), original);
    assert_eq!(
        send(&r, PDF, 4, Some(1), b"different"),
        Err(RepositoryError::OperationConflict)
    );
    assert_eq!(
        send(&r, FORM, 4, Some(1), b"second"),
        Err(RepositoryError::OperationConflict)
    );
    let mut changed = request(PDF, 4, Some(1));
    changed.display_name = "another-name".into();
    assert_eq!(
        r.upload(operator(), changed, &mut b"second".as_slice(), None),
        Err(RepositoryError::OperationConflict)
    );
    assert_eq!(
        send(&r, PDF, 6, Some(1), b"stale"),
        Err(RepositoryError::VersionConflict)
    );
    assert_eq!(
        r.metadata.counts(),
        (before.0 + 2, before.1 + 2, before.2 + 2)
    );
}

struct BrokenStream(bool);
impl Read for BrokenStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.0 {
            return Err(io::Error::other("interrupted transfer"));
        }
        self.0 = true;
        b"incomplete".as_slice().read(buf)
    }
}

#[test]
fn failures_keep_the_previous_version_and_do_not_emit_effects() {
    let r = published();
    let before = r.metadata.counts();
    let put = || send(&r, PDF, 4, Some(1), b"new");
    let guard = r.objects.hold(Activity::Upload).unwrap();
    assert_eq!(put(), Err(RepositoryError::UploadBusy));
    drop(guard);
    r.objects.no_space.set(true);
    assert_eq!(put(), Err(RepositoryError::InsufficientSpace));
    r.objects.no_space.set(false);
    r.objects.max_bytes.set(2);
    assert_eq!(put(), Err(RepositoryError::TooLarge));
    r.objects.max_bytes.set(1024);
    assert_eq!(
        r.upload(
            operator(),
            request(PDF, 4, Some(1)),
            &mut b"new".as_slice(),
            Some(ObjectHash([0; 32]))
        ),
        Err(RepositoryError::ChecksumMismatch)
    );
    assert_eq!(
        r.upload(
            operator(),
            request(PDF, 4, Some(1)),
            &mut BrokenStream(false),
            None
        ),
        Err(RepositoryError::Unavailable)
    );
    assert_eq!(r.objects.counts(), (1, 2));
    r.metadata.crash(Crash::BeforeCommit);
    assert_eq!(put(), Err(RepositoryError::Unavailable));
    assert_eq!(r.objects.counts(), (1, 3));
    assert_eq!(r.metadata.counts(), before);
    assert_eq!(read(&r, PDF, None, PUBLIC).unwrap(), b"shared PDF");
    assert_eq!(put().unwrap().version.seq, 2);
}

#[test]
fn unavailable_bytes_block_download_and_staff_can_identify_all_affected_links() {
    let r = published();
    let v = VersionRef {
        deposit: THESIS,
        file: PDF,
        seq: 1,
    };
    let obj = r.metadata.receipt(v).unwrap().object;
    r.objects.corrupt(obj.hash);
    for state in [Integrity::Corrupt, Integrity::Missing] {
        let (status, mut links) = r.diagnose(operator(), v).unwrap();
        links.sort();
        assert_eq!(status, state);
        assert_eq!(links, [v, VersionRef { file: FORM, ..v }]);
        assert_eq!(
            read(&r, PDF, None, PUBLIC),
            Err(RepositoryError::ObjectUnavailable)
        );
        assert_eq!(
            read(&r, FORM, None, PUBLIC),
            Err(RepositoryError::NotVisible)
        );
        assert_eq!(r.page(THESIS, PUBLIC).unwrap().files.len(), 1);
        if state == Integrity::Corrupt {
            assert_eq!(
                send(&r, PDF, 4, Some(1), b"shared PDF"),
                Err(RepositoryError::ObjectUnavailable)
            );
            r.objects.remove(obj.hash);
        }
    }
}

#[test]
fn grace_periods_and_live_guards_protect_retained_and_in_flight_objects() {
    let r = published();
    let clock = &r.clock.0;
    let collect = || r.objects.collect(clock.get(), &r.metadata);
    r.metadata.crash(Crash::BeforeCommit);
    assert_eq!(
        send(&r, PDF, 4, Some(1), b"orphan"),
        Err(RepositoryError::Unavailable)
    );
    assert_eq!(
        r.upload(
            operator(),
            request(PDF, 5, Some(1)),
            &mut BrokenStream(false),
            None
        ),
        Err(RepositoryError::Unavailable)
    );
    let sheet = VersionRef {
        deposit: THESIS,
        file: SHEET,
        seq: 1,
    };
    r.withdraw_version(operator(), sheet, false).unwrap();
    clock.set(100 + DAY - 1);
    collect().unwrap(); // First observation of the orphan, not its creation date.
    assert_eq!(r.objects.counts(), (1, 3));
    clock.set(100 + DAY);
    collect().unwrap();
    assert_eq!(r.objects.counts(), (0, 3));
    clock.set(100 + 8 * DAY - 2);
    collect().unwrap();
    assert_eq!(r.objects.counts(), (0, 3));
    clock.set(100 + 8 * DAY - 1);
    let download = r.download(THESIS, PDF, None, PUBLIC).unwrap();
    assert_eq!(collect(), Err(RepositoryError::UploadBusy));
    drop(download);
    for activity in [Activity::Upload, Activity::Backup] {
        let guard = r.objects.hold(activity).unwrap();
        assert_eq!(collect(), Err(RepositoryError::UploadBusy));
        drop(guard);
    }
    collect().unwrap();
    assert_eq!(r.objects.counts(), (0, 2));
    assert_eq!(
        read(&r, SHEET, Some(1), ReadAudience::Staff(operator())).unwrap(),
        b"1,2"
    );
}

#[test]
fn writer_serializes_two_replacements_which_observed_the_same_head() {
    let r = published();
    let metadata = &r.metadata;
    let current = VersionRef {
        deposit: THESIS,
        file: PDF,
        seq: 1,
    };
    let object = metadata.receipt(current).unwrap().object;
    let start = std::sync::Barrier::new(2);
    let before = metadata.counts();
    let replace = |key| {
        start.wait();
        metadata.commit_version(operator(), 101, request(PDF, key, Some(1)), object)
    };
    let outcomes = std::thread::scope(|scope| {
        let first = scope.spawn(|| replace(11));
        let second = scope.spawn(|| replace(12));
        [first.join().unwrap(), second.join().unwrap()]
    });
    assert_eq!(outcomes.iter().filter(|r| r.is_ok()).count(), 1);
    assert!(outcomes.contains(&Err(RepositoryError::VersionConflict)));
    assert_eq!(
        metadata.counts(),
        (before.0 + 1, before.1 + 1, before.2 + 1)
    );
    assert_eq!(
        metadata
            .receipt(VersionRef { seq: 2, ..current })
            .unwrap()
            .object,
        object
    );
}

#[test]
fn verifies_standard_sha256_and_a_failed_staff_audit_prevents_delivery() {
    let r = setup();
    let hash: ObjectHash = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        .parse()
        .unwrap();
    let receipt = r
        .upload(
            operator(),
            request(PDF, 1, None),
            &mut b"abc".as_slice(),
            Some(hash),
        )
        .unwrap();
    assert_eq!(receipt.object, ObjectInfo { hash, bytes: 3 });
    r.metadata.crash(Crash::BeforeCommit);
    assert_eq!(
        read(&r, PDF, Some(1), ReadAudience::Staff(operator())),
        Err(RepositoryError::Unavailable)
    );
    assert_eq!(
        read(&r, PDF, Some(1), ReadAudience::Staff(operator())).unwrap(),
        b"abc"
    );
}
