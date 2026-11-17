// SPDX-License-Identifier: GPL-3.0-or-later
//! Public CLI observations never register a library or initialize missing data.
use std::process::Command;

#[test]
fn local_domain_status_uses_the_default_without_creating_data_or_secrets() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("server.toml");
    std::fs::write(&config, "[server]\ndata_dir='missing'\n").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_elzevir"))
        .args(["address", "--config"])
        .arg(&config)
        .arg("status")
        .env_remove("ELZEVIR_BROKER_URL")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let view: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(view["broker_url"], "https://broker.elzevir.app");
    assert_eq!(view["domain"], "elzevir.app");
    assert_eq!(view["phase"], "not_registered");
    assert_eq!(view["connector"], "unavailable");
    assert!(!directory.path().join("missing").exists());
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn registration_accepts_the_broker_url_alias_and_requires_confirmation() {
    for flag in ["--broker", "--broker-url"] {
        let directory = tempfile::tempdir().unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_elzevir"))
            .args([
                "address",
                "--config",
                "missing.toml",
                "enroll",
                flag,
                "https://broker.example.org",
                "--institution",
                "Fictional library",
                "--email",
                "fictional@example.org",
            ])
            .current_dir(directory.path())
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains("requires --confirm"));
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    }
}
