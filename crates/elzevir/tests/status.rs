// SPDX-License-Identifier: GPL-3.0-or-later
//! Local status observes the live instance without touching SQLite.
#![expect(
    clippy::unwrap_used,
    reason = "integration helpers fail tests on errors"
)]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Child, Command, Output};
use std::time::{Duration, Instant};

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        drop(self.0.kill());
        drop(self.0.wait());
    }
}

fn status(root: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_elzevir"))
        .args(["status", "--data-dir", root.to_str().unwrap(), "--json"])
        .envs([
            ("HTTP_PROXY", "http://127.0.0.1:9"),
            ("http_proxy", "http://127.0.0.1:9"),
            ("ALL_PROXY", "http://127.0.0.1:9"),
            ("all_proxy", "http://127.0.0.1:9"),
            ("NO_PROXY", ""),
            ("no_proxy", ""),
        ])
        .output()
        .unwrap()
}

fn start(root: &Path) -> Server {
    let log = std::fs::File::create(root.join("stdout.log")).unwrap();
    let mut server = Server(
        Command::new(env!("CARGO_BIN_EXE_elzevir"))
            .args([
                "serve",
                "--data-dir",
                root.to_str().unwrap(),
                "--listen",
                "127.0.0.1:0",
            ])
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let text = std::fs::read_to_string(root.join("stdout.log")).unwrap();
        if text
            .lines()
            .any(|line| line.starts_with("elzevir: listening on http://"))
        {
            return server;
        }
        assert!(server.0.try_wait().unwrap().is_none(), "{text}");
        assert!(Instant::now() < deadline, "startup timed out: {text}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn status_tracks_live_and_stopped_servers_despite_stale_metadata() {
    let root = tempfile::tempdir().unwrap();
    let mut server = start(root.path());
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let output = status(root.path());
        if output.status.success() {
            let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(report["state"], "running");
            assert_eq!(report["health"]["database"], "ok");
            assert_eq!(report["health"]["indexer"], "ok");
            break;
        }
        assert!(Instant::now() < deadline, "status did not become healthy");
        std::thread::sleep(Duration::from_millis(20));
    }
    server.0.kill().unwrap();
    server.0.wait().unwrap();
    assert!(root.path().join("elzevir-runtime.json").exists());
    let output = status(root.path());
    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["state"], "stopped");
}

#[test]
fn absent_directory_is_not_created_and_bad_metadata_is_unavailable() {
    let root = tempfile::tempdir().unwrap();
    let absent = root.path().join("absent");
    let result = status(&absent);
    assert!(!result.status.success());
    assert!(!absent.exists());
    let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["state"], "unavailable");
    let lock = std::fs::File::create(root.path().join("elzevir.lock")).unwrap();
    lock.lock().unwrap();
    std::fs::write(root.path().join("elzevir-runtime.json"), "invalid").unwrap();
    let output = status(root.path());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["state"], "unavailable");
    assert_eq!(
        std::fs::read_to_string(root.path().join("elzevir-runtime.json")).unwrap(),
        "invalid"
    );
}

fn fake_response(root: &Path, code: u16, body: &str, headers: &str) -> Output {
    let lock = std::fs::File::create(root.join("elzevir.lock")).unwrap();
    lock.lock().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let info = serde_json::json!({"schema":1,"instance":"0".repeat(32),"pid":1,"address":listener.local_addr().unwrap().to_string()});
    std::fs::write(root.join("elzevir-runtime.json"), info.to_string()).unwrap();
    let response = format!(
        "HTTP/1.1 {code} Test\r\nContent-Length: {}\r\n{headers}Connection: close\r\n\r\n{body}",
        body.len()
    );
    let before = std::fs::read(root.join("elzevir-runtime.json")).unwrap();
    let responder = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok((mut stream, _)) = listener.accept() {
                // On Windows an accepted socket inherits the listener's non-blocking
                // mode, and the read below would fail with `WouldBlock`.
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = [0_u8; 2048];
                assert!(stream.read(&mut request).unwrap() > 0);
                if let Err(error) = stream.write_all(response.as_bytes()) {
                    // Bounded clients may close before the oversized body is sent.
                    assert!(matches!(
                        error.kind(),
                        std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset
                    ));
                }
                return;
            }
            assert!(
                Instant::now() < deadline,
                "status never contacted local health"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    });
    let output = status(root);
    responder.join().unwrap();
    assert_eq!(
        std::fs::read(root.join("elzevir-runtime.json")).unwrap(),
        before
    );
    assert!(!root.join("index").exists());
    assert!(!root.join("logs").exists());
    output
}

#[test]
fn identity_mismatch_and_oversized_responses_never_report_running() {
    let body = serde_json::json!({"schema":1,"instance":"1".repeat(32),"pid":1,"version":"0.0.0","uptime_seconds":1,"database":"ok","indexer":"ok"}).to_string();
    let extra = serde_json::json!({"schema":1,"instance":"0".repeat(32),"pid":1,"version":"0.0.0","uptime_seconds":1,"database":"ok","indexer":"ok","extra":"fictional-sensitive-response"}).to_string();
    for payload in [body, extra, "x".repeat(5000)] {
        let root = tempfile::tempdir().unwrap();
        let output = fake_response(root.path(), 200, &payload, "");
        assert!(!output.status.success());
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["state"], "unavailable");
        assert!(!String::from_utf8_lossy(&output.stdout).contains("fictional-sensitive-response"));
        assert!(!String::from_utf8_lossy(&output.stderr).contains("fictional-sensitive-response"));
    }
}

#[test]
fn degraded_health_is_distinct_from_an_unreachable_server() {
    let body = serde_json::json!({"schema":1,"instance":"0".repeat(32),"pid":1,"version":"0.0.0","uptime_seconds":1,"database":"failed","indexer":"ok"}).to_string();
    for (code, expected) in [(503, "degraded"), (200, "unavailable")] {
        let root = tempfile::tempdir().unwrap();
        let output = fake_response(root.path(), code, &body, "");
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(!output.status.success());
        assert_eq!(report["state"], expected);
        if code == 503 {
            assert_eq!(report["health"]["database"], "failed");
        }
    }
}

#[test]
fn remote_metadata_is_refused_before_any_network_request() {
    let root = tempfile::tempdir().unwrap();
    let lock = std::fs::File::create(root.path().join("elzevir.lock")).unwrap();
    lock.lock().unwrap();
    let info = serde_json::json!({"schema":1,"instance":"0".repeat(32),"pid":1,"address":"203.0.113.1:80"});
    std::fs::write(root.path().join("elzevir-runtime.json"), info.to_string()).unwrap();
    let output = status(root.path());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["state"], "unavailable");
    assert_eq!(report["reason"], "invalid_runtime_metadata");
}

#[test]
fn healthy_status_never_opens_sqlite_and_refuses_inconsistent_http_codes() {
    let body = serde_json::json!({"schema":1,"instance":"0".repeat(32),"pid":1,"version":"0.0.0","uptime_seconds":1,"database":"ok","indexer":"ok"}).to_string();
    for (code, expected) in [(200, "running"), (503, "unavailable"), (404, "unavailable")] {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("elzevir.db");
        std::fs::write(&database, "not a SQLite database").unwrap();
        let output = fake_response(root.path(), code, &body, "");
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["state"], expected);
        assert_eq!(output.status.success(), code == 200);
        assert_eq!(
            std::fs::read_to_string(database).unwrap(),
            "not a SQLite database"
        );
    }
}

#[test]
fn redirect_destinations_are_never_contacted() {
    let root = tempfile::tempdir().unwrap();
    let destination = TcpListener::bind("127.0.0.1:0").unwrap();
    destination.set_nonblocking(true).unwrap();
    let headers = format!(
        "Location: http://{}/health\r\n",
        destination.local_addr().unwrap()
    );
    let output = fake_response(root.path(), 302, "", &headers);
    assert!(!output.status.success());
    assert_eq!(
        destination.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[test]
fn configuration_paths_and_cli_overrides_do_not_create_data() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("settings")).unwrap();
    std::fs::create_dir(root.path().join("library")).unwrap();
    std::fs::File::create(root.path().join("library/elzevir.lock")).unwrap();
    std::fs::write(
        root.path().join("settings/server.toml"),
        "[server]\ndata_dir = '../library'\n",
    )
    .unwrap();
    let text = Command::new(env!("CARGO_BIN_EXE_elzevir"))
        .args(["status", "--config", "settings/server.toml"])
        .current_dir(root.path())
        .output()
        .unwrap();
    assert!(!text.status.success());
    assert!(String::from_utf8_lossy(&text.stdout).contains("elzevir: stopped (lock_free)"));
    let json = Command::new(env!("CARGO_BIN_EXE_elzevir"))
        .args([
            "status",
            "--config",
            "settings/server.toml",
            "--data-dir",
            "absent",
            "--json",
        ])
        .current_dir(root.path())
        .output()
        .unwrap();
    assert!(!json.status.success());
    let report: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(report["schema"], 1);
    assert_eq!(report["state"], "unavailable");
    assert!(!root.path().join("absent").exists());
    assert!(!root.path().join("library/elzevir.db").exists());
}

#[cfg(unix)]
#[test]
fn symbolic_links_are_refused_without_following_their_targets() {
    for name in ["elzevir.lock", "elzevir-runtime.json"] {
        let root = tempfile::tempdir().unwrap();
        let lock = (name != "elzevir.lock").then(|| {
            let file = std::fs::File::create(root.path().join("elzevir.lock")).unwrap();
            file.lock().unwrap();
            file
        });
        std::os::unix::fs::symlink("absent", root.path().join(name)).unwrap();
        let output = status(root.path());
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["state"], "unavailable");
        assert!(!root.path().join("absent").exists());
        assert!(
            std::fs::symlink_metadata(root.path().join(name))
                .unwrap()
                .is_symlink()
        );
        drop(lock);
    }
}
