// SPDX-License-Identifier: GPL-3.0-or-later
//! Demo-only command validation without terminal escape sequences or data access.

use std::process::Command;

#[test]
fn panel_requires_explicit_demo_and_an_interactive_terminal() {
    let dir = tempfile::tempdir().unwrap();
    for (args, expected) in [
        (vec!["panel"], "--demo"),
        (vec!["panel", "--demo"], "interactive terminal"),
        (
            vec!["panel", "--demo", "--lang", "pt-BR"],
            "terminal interativo",
        ),
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_elzevir"))
            .args(args)
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains(expected));
        assert!(
            result.stdout.is_empty(),
            "non-TTY output must not contain escape sequences"
        );
        assert!(!result.stderr.contains(&27));
    }
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn connected_panel_checks_tty_without_initializing_data() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("elzevir.toml"),
        "[server]\ndata_dir='missing'\n",
    )
    .unwrap();
    for args in [
        vec!["panel", "--config", "elzevir.toml"],
        vec!["panel", "--data-dir", "missing"],
        vec!["panel", "--demo", "--config", "elzevir.toml"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_elzevir"))
            .args(args)
            .current_dir(root.path())
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.contains(&27));
        assert!(!root.path().join("missing").exists());
    }
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}
