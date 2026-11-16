// SPDX-License-Identifier: GPL-3.0-or-later
//! Configuration exercised through the real executable and HTTP listener.
#![expect(
    clippy::unwrap_used,
    reason = "integration helpers fail the test on error"
)]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        // Best effort cleanup also runs when an assertion fails.
        drop(self.0.kill());
        drop(self.0.wait());
    }
}

fn start(cwd: &Path, args: &[&str]) -> (Server, SocketAddr) {
    let log_path = cwd.join("startup.log");
    let log = std::fs::File::create(&log_path).unwrap();
    let mut process = Server(
        Command::new(env!("CARGO_BIN_EXE_elzevir"))
            .arg("serve")
            .args(args)
            .current_dir(cwd)
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let output = std::fs::read_to_string(&log_path).unwrap();
        if let Some(address) = output
            .lines()
            .find_map(|line| line.strip_prefix("elzevir: listening on http://"))
        {
            return (process, address.parse().unwrap());
        }
        assert!(
            process.0.try_wait().unwrap().is_none(),
            "server exited: {output}"
        );
        assert!(
            Instant::now() < deadline,
            "server startup timed out: {output}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn request(address: SocketAddr, path: &str) -> String {
    let response = raw(address, "localhost", path);
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    response
}

fn raw(address: SocketAddr, host: &str, path: &str) -> String {
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(5)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    stream
        .take(1024 * 1024)
        .read_to_string(&mut response)
        .unwrap();
    response
}

fn fixture(root: &Path, listen: &str) -> PathBuf {
    let dir = root.join("configuration");
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("elzevir.toml");
    std::fs::write(
        &path,
        format!("[server]\ndata_dir = 'library'\nlisten = '{listen}'\ninsecure_cookies = true\n"),
    )
    .unwrap();
    path
}

#[test]
fn file_starts_catalog_and_resolves_paths_relative_to_itself() {
    let root = tempfile::tempdir().unwrap();
    let config = fixture(root.path(), "127.0.0.1:0");
    let data = config.parent().unwrap().join("library");
    std::fs::create_dir(&data).unwrap();
    std::fs::write(
        data.join("elzevir.toml"),
        "[call_number]\nauthor_table = 'authors.txt'\n",
    )
    .unwrap();
    std::fs::write(data.join("authors.txt"), "Assi 848\n").unwrap();
    let (_server, address) = start(root.path(), &["--config", config.to_str().unwrap()]);
    assert!(
        std::fs::read_to_string(root.path().join("startup.log"))
            .unwrap()
            .contains("1 entries from 1 lines")
    );
    assert_ne!(address.port(), 0);
    assert!(request(address, "/").contains("Library catalog"));
    let login = request(address, "/equipe/login");
    assert!(login.contains("set-cookie:"));
    assert!(!login.contains("; Secure"));
    assert!(
        config
            .parent()
            .unwrap()
            .join("library/elzevir.db")
            .is_file()
    );
    assert!(!root.path().join("library").exists());
}

#[test]
fn cli_options_override_file_and_can_restore_secure_cookies() {
    let root = tempfile::tempdir().unwrap();
    // This address cannot bind locally; only the CLI override makes startup possible.
    let config = fixture(root.path(), "192.0.2.1:1");
    let (_server, address) = start(
        root.path(),
        &[
            "--config",
            config.to_str().unwrap(),
            "--data-dir",
            "override",
            "--listen",
            "127.0.0.1:0",
            "--insecure-cookies=false",
        ],
    );
    assert!(request(address, "/equipe/login").contains("; Secure"));
    assert!(root.path().join("override/elzevir.db").is_file());
    assert!(!config.parent().unwrap().join("library").exists());
}

#[test]
fn old_cli_still_starts_with_secure_cookies_by_default() {
    let root = tempfile::tempdir().unwrap();
    let (_server, address) = start(
        root.path(),
        &["--data-dir", "legacy", "--listen", "127.0.0.1:0"],
    );
    assert!(request(address, "/equipe/login").contains("; Secure"));
    assert!(root.path().join("legacy/elzevir.db").is_file());
}

#[test]
fn invalid_configuration_does_not_create_data_or_echo_contents() {
    let root = tempfile::tempdir().unwrap();
    let marker = "sensitive-value-not-for-logs";
    let mut cases = vec![
        format!("secret = '{marker}'"),
        format!("[server]\nlisten = '{marker}'"),
        format!("[server]\ninsecure_cookies = '{marker}'"),
        format!("[server]\n{marker} = true"),
        "[server".to_owned(),
        "[server]\ndata_dir = ''".to_owned(),
        "[institution]\nname = '   '".to_owned(),
        "[institution]\nname = \"Library\\n\"".to_owned(),
        format!("[institution]\nname = '{}'", "é".repeat(201)),
        "#".repeat(65 * 1024),
    ]
    .into_iter()
    .map(String::into_bytes)
    .collect::<Vec<_>>();
    cases.push(vec![0xff]);
    for bytes in cases {
        std::fs::write(root.path().join("invalid.toml"), bytes).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_elzevir"))
            .args([
                "serve",
                "--config",
                "invalid.toml",
                "--data-dir",
                "must-not-exist",
            ])
            .current_dir(root.path())
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains(marker));
        assert!(!root.path().join("must-not-exist").exists());
    }
}

#[test]
fn bare_http_flag_still_disables_secure_cookies() {
    let root = tempfile::tempdir().unwrap();
    let (_server, address) = start(
        root.path(),
        &[
            "--data-dir",
            "http",
            "--listen",
            "127.0.0.1:0",
            "--insecure-cookies",
        ],
    );
    let login = request(address, "/equipe/login");
    assert!(login.contains("set-cookie:"));
    assert!(!login.contains("; Secure"));
}

#[test]
fn absent_file_or_directory_is_reported_before_startup() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("empty.toml"), "").unwrap();
    for (file, expected) in [
        ("missing.toml", "cannot read configuration"),
        ("empty.toml", "server.data_dir"),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_elzevir"))
            .args(["serve", "--config", file])
            .current_dir(root.path())
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains(expected));
        assert!(output.stdout.is_empty());
    }
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn initialized_configuration_starts_the_real_catalog() {
    let root = tempfile::tempdir().unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_elzevir"))
        .args(["init", "--name", "Test library"])
        .env("ELZEVIR_STAFF_PASSWORD", "fictional-init-password")
        .current_dir(root.path())
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let (_server, address) = start(
        root.path(),
        &["--config", "elzevir.toml", "--listen", "127.0.0.1:0"],
    );
    assert!(request(address, "/").contains("Library catalog"));
    assert!(request(address, "/equipe/login").contains("; Secure"));
}
