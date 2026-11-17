// SPDX-License-Identifier: GPL-3.0-or-later
//! Diagnostics are available without creating or opening an institution.

use std::process::Command;

#[test]
fn missing_data_stays_missing_and_export_never_overwrites() {
    let root = tempfile::tempdir().unwrap();
    let missing = root.path().join("missing");
    let output = root.path().join("support.zip");
    let result = Command::new(env!("CARGO_BIN_EXE_elzevir"))
        .arg("diagnose")
        .arg("--data-dir")
        .arg(&missing)
        .arg("--output")
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["schema"], 1);
    assert_eq!(report["state"], "unavailable");
    assert_eq!(report["quick_check"], "unknown");
    assert!(!missing.exists());
    let bytes = std::fs::read(&output).unwrap();
    assert!(bytes.starts_with(b"PK\x03\x04"));
    assert!(bytes.windows(11).any(|s| s == b"report.json"));
    assert!(bytes.windows(12).any(|s| s == b"events.jsonl"));
    assert!(
        !bytes
            .windows(missing.as_os_str().len())
            .any(|s| s == missing.to_string_lossy().as_bytes())
    );
    let second = Command::new(env!("CARGO_BIN_EXE_elzevir"))
        .arg("diagnose")
        .arg("--data-dir")
        .arg(&missing)
        .arg("--output")
        .arg(&output)
        .output()
        .unwrap();
    assert!(!second.status.success());
    assert_eq!(std::fs::read(output).unwrap(), bytes);
    assert!(!missing.exists());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}
