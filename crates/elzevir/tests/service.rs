// SPDX-License-Identifier: GPL-3.0-or-later
//! Service plans are reviewable without touching the host's service manager.

#[test]
fn control_dry_run_is_side_effect_free_and_names_are_bounded() {
    let root = tempfile::tempdir().unwrap();
    for action in ["start", "stop", "restart", "uninstall"] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_elzevir"))
            .args(["service", action, "--name", "elzevir-test", "--dry-run"])
            .current_dir(root.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let plan: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(plan["name"], "elzevir-test");
        assert_eq!(plan["action"], action);
    }
    for name in [
        "sshd",
        "elzevir/other",
        "elzevir-",
        "elzevir;reboot",
        "elzevir-TEST",
    ] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_elzevir"))
            .args(["service", "start", "--name", name, "--dry-run"])
            .output()
            .unwrap();
        assert!(!output.status.success());
    }
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn install_plan_uses_existing_paths_and_preserves_the_initialized_institution() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("institution.toml");
    let data = root.path().join("data");
    let initialized = std::process::Command::new(env!("CARGO_BIN_EXE_elzevir"))
        .args(["init", "--name", "Fictional library", "--config"])
        .arg(&config)
        .arg("--data-dir")
        .arg(&data)
        .env("ELZEVIR_STAFF_PASSWORD", "fictional-install-password")
        .output()
        .unwrap();
    assert!(initialized.status.success());
    let original = std::fs::read(&config).unwrap();
    let before = std::fs::read(data.join("elzevir.db")).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_elzevir"))
        .args([
            "service",
            "install",
            "--name",
            "elzevir-fixture",
            "--dry-run",
            "--config",
        ])
        .arg(&config)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let plan: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(plan["action"], "install");
    assert_eq!(plan["name"], "elzevir-fixture");
    assert_eq!(
        std::path::Path::new(plan["config"].as_str().unwrap()),
        config.canonicalize().unwrap()
    );
    assert!(!data.join("elzevir-runtime.json").exists());
    assert_eq!(std::fs::read(config).unwrap(), original);
    assert_eq!(std::fs::read(data.join("elzevir.db")).unwrap(), before);
}
