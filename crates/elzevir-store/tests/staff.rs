// SPDX-License-Identifier: GPL-3.0-or-later
//! Staff accounts through the real command handler and directory.
#![expect(
    clippy::unwrap_used,
    reason = "integration test: helper functions fail the test on error"
)]

use elzevir_app::commands::{StaffCommand, WriteCommand, WriteOutput, WriteRefusal};
use elzevir_app::staff::{LoginRefused, Permission, StaffDirectory, hash_password, log_in};
use elzevir_store::{CommandFailed, ElzevirHandler, ElzevirStore, StoreConfig};

fn create(login: &str, password: &str, permissions: &[Permission]) -> WriteCommand {
    WriteCommand::Staff(StaffCommand::Create {
        login: login.to_owned(),
        password_hash: hash_password(password).unwrap(),
        permissions: permissions.iter().copied().collect(),
    })
}

fn open(dir: &tempfile::TempDir) -> ElzevirStore {
    ElzevirStore::open(
        &dir.path().join("elzevir.db"),
        &StoreConfig::default(),
        ElzevirHandler,
    )
    .unwrap()
}

#[test]
fn created_staff_can_log_in_and_logins_are_unique() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(&dir);

    let created = store
        .submit(create("ana", "s3cret", &[Permission::StaffInterface]))
        .unwrap()
        .wait()
        .unwrap();
    assert!(matches!(created, WriteOutput::StaffCreated(_)));

    let duplicate = store.submit(create("ana", "other", &[])).unwrap().wait();
    assert_eq!(
        duplicate,
        Err(CommandFailed::Rejected(WriteRefusal::LoginTaken(
            "ana".to_owned()
        )))
    );

    let account = store.by_login("ana").unwrap().unwrap();
    assert_eq!(account.permissions, [Permission::StaffInterface].into());
    assert!(store.by_login("nobody").unwrap().is_none());

    let who = log_in(&store, "ana", "s3cret").unwrap();
    assert_eq!(who.login, "ana");
    assert_eq!(
        log_in(&store, "ana", "other"),
        Err(LoginRefused::InvalidCredentials)
    );
}

#[test]
fn staff_creation_audits_the_actor_action_and_login_without_the_password_hash() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(&dir);
    store
        .submit_as(
            7,
            create("bia", "plain-text-password", &[Permission::StaffInterface]),
        )
        .unwrap()
        .wait()
        .unwrap();
    let (actor, kind, payload): (i64, String, String) = store
        .readers()
        .read(|c| {
            c.query_row("SELECT actor, kind, command FROM audit_log", [], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
        })
        .unwrap();
    assert_eq!(actor, 7);
    assert_eq!(kind, "staff.create");
    assert!(!payload.contains("$argon2id$"));
    assert!(!payload.contains("plain-text-password"));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&payload).unwrap(),
        serde_json::json!({"target": "bia"})
    );
}
