// SPDX-License-Identifier: GPL-3.0-or-later
//! Address observations never initialize a missing institution.

#[test]
fn address_cli_keeps_missing_data_missing() {
    let root = tempfile::tempdir().unwrap();
    let missing = root.path().join("missing");
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_elzevir"))
        .args(["addresses", "--test", "--data-dir"])
        .arg(&missing)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["state"], "unavailable");
    assert!(report["observed_mode"].is_null());
    assert_eq!(report["catalog_test"], "unknown");
    assert!(!missing.exists());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}
