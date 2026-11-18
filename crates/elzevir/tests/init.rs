// SPDX-License-Identifier: GPL-3.0-or-later
//! Initialization preserves existing work and creates a usable administrator.
#![expect(
    clippy::unwrap_used,
    reason = "integration helpers fail the test on error"
)]

use elzevir_app::staff::{Permission, StaffDirectory, log_in};
use elzevir_store::{ElzevirHandler, ElzevirStore, StoreConfig};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

const PASSWORD: &str = "fictional-init-password-123";

#[test]
fn installer_port_preflight_refuses_an_occupied_address_without_creating_data() {
    let root = tempfile::tempdir().unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let result = init(
        root.path(),
        &["--check-port", "--listen", &address],
        PASSWORD,
    );
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("listen address is unavailable"));
    assert!(!root.path().join("elzevir.toml").exists());
    assert!(!root.path().join("data-local").exists());
}

fn init(directory: &Path, extra: &[&str], password: &str) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_elzevir"));
    command.arg("init");
    if !extra.contains(&"--name") {
        command.args(["--name", "Biblioteca de demonstração"]);
    }
    if !extra.contains(&"--data-dir") {
        command.args(["--data-dir", "data-local"]);
    }
    command
        .args(extra)
        .current_dir(directory)
        .env("ELZEVIR_STAFF_PASSWORD", password)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn open(directory: &Path) -> ElzevirStore {
    ElzevirStore::open(
        &directory.join("elzevir.db"),
        &StoreConfig::default(),
        ElzevirHandler,
    )
    .unwrap()
}

#[test]
fn initializes_a_private_config_and_a_usable_admin_without_plaintext_password() {
    let root = tempfile::tempdir().unwrap();
    let result = init(root.path(), &[], PASSWORD);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(!String::from_utf8_lossy(&result.stdout).contains(PASSWORD));
    assert!(!String::from_utf8_lossy(&result.stderr).contains(PASSWORD));
    let source = std::fs::read_to_string(root.path().join("elzevir.toml")).unwrap();
    assert!(!source.contains(PASSWORD));
    assert!(!source.contains("argon2"));
    let config: toml::Value = toml::from_str(&source).unwrap();
    assert_eq!(
        config["institution"]["name"].as_str(),
        Some("Biblioteca de demonstração")
    );
    let data = root.path().join("data-local");
    assert_eq!(
        config["server"]["data_dir"].as_str(),
        data.canonicalize().unwrap().to_str()
    );
    let store = open(&data);
    let account = store.by_login("admin").unwrap().unwrap();
    assert!(account.password_hash.starts_with("$argon2id$"));
    let identity = log_in(&store, "admin", PASSWORD).unwrap();
    for permission in [
        Permission::StaffInterface,
        Permission::Administration,
        Permission::Cataloguing,
    ] {
        assert!(identity.can(permission));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&data).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(root.path().join("elzevir.toml"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[test]
fn rerunning_init_never_resets_credentials_or_rewrites_config() {
    let root = tempfile::tempdir().unwrap();
    assert!(init(root.path(), &[], PASSWORD).status.success());
    let config = std::fs::read(root.path().join("elzevir.toml")).unwrap();
    let store = open(&root.path().join("data-local"));
    let digest = store.digest().unwrap();
    assert!(
        !init(root.path(), &[], "a-different-password")
            .status
            .success()
    );
    assert_eq!(
        std::fs::read(root.path().join("elzevir.toml")).unwrap(),
        config
    );
    assert_eq!(store.digest().unwrap(), digest);
    assert!(log_in(&store, "admin", PASSWORD).is_ok());
}

#[test]
fn preserves_existing_paths_and_rejects_bad_input_without_side_effects() {
    let long_name = "é".repeat(201);
    for extra in [
        vec!["--login", "bad login"],
        vec!["--name", "   "],
        vec!["--name", "Library\n"],
        vec!["--name", &long_name],
        vec!["--data-dir", ""],
        vec!["--data-dir", "elzevir.toml"],
        vec!["--data-dir", "missing/data"],
        vec!["--config", "missing/elzevir.toml"],
    ] {
        let root = tempfile::tempdir().unwrap();
        assert!(!init(root.path(), &extra, PASSWORD).status.success());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }
    for password in ["short", &"x".repeat(1025)] {
        let root = tempfile::tempdir().unwrap();
        assert!(!init(root.path(), &[], password).status.success());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("data-local")).unwrap();
    std::fs::write(root.path().join("data-local/user-file"), "preserve").unwrap();
    assert!(!init(root.path(), &[], PASSWORD).status.success());
    assert_eq!(
        std::fs::read_to_string(root.path().join("data-local/user-file")).unwrap(),
        "preserve"
    );
    assert!(!root.path().join("elzevir.toml").exists());
}

#[test]
fn explicit_stdin_overrides_environment_and_custom_paths_are_escaped() {
    let root = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_elzevir"))
        .args([
            "init",
            "--name",
            "Test library",
            "--password-stdin",
            "--data-dir",
            "data with 'quotes'",
            "--login",
            "librarian",
            "--insecure-cookies",
        ])
        .current_dir(root.path())
        .env("ELZEVIR_STAFF_PASSWORD", "wrong-password")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(child.stdin.take().unwrap(), "{PASSWORD}").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let store = open(&root.path().join("data with 'quotes'"));
    assert!(log_in(&store, "librarian", PASSWORD).is_ok());
    assert!(log_in(&store, "librarian", "wrong-password").is_err());
    let config: toml::Value =
        toml::from_str(&std::fs::read_to_string(root.path().join("elzevir.toml")).unwrap())
            .unwrap();
    assert_eq!(config["server"]["insecure_cookies"].as_bool(), Some(true));
}

#[test]
fn noninteractive_init_requires_an_explicit_password_source() {
    let root = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_elzevir"))
        .args(["init", "--name", "Test library", "--data-dir", "data-local"])
        .current_dir(root.path())
        .env_remove("ELZEVIR_STAFF_PASSWORD")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--password-stdin"));
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn an_existing_config_is_preserved_without_creating_data() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("elzevir.toml"), "user configuration").unwrap();
    assert!(!init(root.path(), &[], PASSWORD).status.success());
    assert_eq!(
        std::fs::read_to_string(root.path().join("elzevir.toml")).unwrap(),
        "user configuration"
    );
    assert!(!root.path().join("data-local").exists());
}

#[cfg(unix)]
#[test]
fn dangling_links_are_never_replaced_or_followed() {
    for target in ["elzevir.toml", "data-local"] {
        let root = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink("missing-target", root.path().join(target)).unwrap();
        assert!(!init(root.path(), &[], PASSWORD).status.success());
        assert!(
            std::fs::symlink_metadata(root.path().join(target))
                .unwrap()
                .is_symlink()
        );
        assert!(!root.path().join("missing-target").exists());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    }
}

#[test]
#[cfg(unix)]
fn default_init_creates_the_institution_and_index_in_xdg_data_home() {
    let root = tempfile::tempdir().unwrap();
    let data_home = root.path().join("user-share");
    let out = Command::new(env!("CARGO_BIN_EXE_elzevir"))
        .args(["init", "--name", "Library"])
        .env("XDG_DATA_HOME", &data_home)
        .env("ELZEVIR_STAFF_PASSWORD", PASSWORD)
        .current_dir(root.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let data = data_home.join("elzevir");
    assert!(data.join("server.toml").is_file());
    assert!(data.join("index/meta.json").is_file());
    assert!(log_in(&open(&data), "admin", PASSWORD).is_ok());
}
