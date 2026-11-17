// SPDX-License-Identifier: GPL-3.0-or-later
//! Local readiness is reachable without exposing it through the catalog.
#![expect(
    clippy::unwrap_used,
    reason = "integration helpers fail tests on errors"
)]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        // Best effort cleanup also runs after a failed assertion.
        drop(self.0.kill());
        drop(self.0.wait());
    }
}

fn start(root: &Path) -> (Server, SocketAddr, SocketAddr) {
    let output = root.join("stdout.log");
    let log = std::fs::File::create(&output).unwrap();
    let mut server = Server(
        Command::new(env!("CARGO_BIN_EXE_elzevir"))
            .env("RUST_LOG", "trace")
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
        let text = std::fs::read_to_string(&output).unwrap();
        let catalog = text
            .lines()
            .find_map(|line| line.strip_prefix("elzevir: listening on http://"));
        let health = text.lines().find_map(|line| {
            line.strip_prefix("elzevir: health on http://")
                .and_then(|address| address.strip_suffix("/health"))
        });
        if let (Some(catalog), Some(health)) = (catalog, health) {
            return (server, catalog.parse().unwrap(), health.parse().unwrap());
        }
        assert!(server.0.try_wait().unwrap().is_none(), "{text}");
        assert!(Instant::now() < deadline, "startup timed out: {text}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn request(address: SocketAddr, method: &str, path: &str) -> String {
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    stream.take(4096).read_to_string(&mut response).unwrap();
    response
}

#[test]
fn readiness_has_a_local_listener_and_never_appears_on_catalog() {
    let root = tempfile::tempdir().unwrap();
    let (server, catalog, health) = start(root.path());
    assert!(health.ip().is_loopback());
    assert_ne!(health.port(), 0);
    assert_ne!(health, catalog);
    assert!(request(catalog, "GET", "/health").starts_with("HTTP/1.1 404"));
    assert!(request(health, "POST", "/health").starts_with("HTTP/1.1 405"));
    assert!(request(health, "GET", "/").starts_with("HTTP/1.1 404"));
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let response = request(health, "GET", "/health");
        assert!(response.contains("cache-control: no-store"));
        let (_, body) = response.split_once("\r\n\r\n").unwrap();
        let report: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(report.as_object().unwrap().len(), 7);
        assert_eq!(report["schema"], 1);
        assert_eq!(report["pid"], server.0.id());
        assert_eq!(report["version"], env!("CARGO_PKG_VERSION"));
        assert!(report["uptime_seconds"].as_u64().is_some());
        let instance = report["instance"].as_str().unwrap();
        assert_eq!(instance.len(), 32);
        assert!(instance.bytes().all(|byte| byte.is_ascii_hexdigit()));
        if response.starts_with("HTTP/1.1 200") {
            assert_eq!(report["database"], "ok");
            assert_eq!(report["indexer"], "ok");
            break;
        }
        assert!(response.starts_with("HTTP/1.1 503"), "{response}");
        assert!(Instant::now() < deadline, "server did not become ready");
        std::thread::sleep(Duration::from_millis(20));
    }
    #[cfg(unix)]
    {
        assert_graceful_stop(server, catalog, health);
        assert!(!root.path().join("elzevir-runtime.json").exists());
    }
}

#[cfg(unix)]
fn assert_graceful_stop(mut server: Server, catalog: SocketAddr, health: SocketAddr) {
    assert!(
        Command::new("kill")
            .args(["-INT", &server.0.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = server.0.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(Instant::now() < deadline, "server did not stop gracefully");
        std::thread::sleep(Duration::from_millis(20));
    }
    for address in [catalog, health] {
        assert!(TcpStream::connect_timeout(&address, Duration::from_secs(1)).is_err());
    }
}

#[test]
fn logs_are_structured_retained_and_do_not_capture_request_secrets() {
    let root = tempfile::tempdir().unwrap();
    let logs = root.path().join("logs");
    std::fs::create_dir(&logs).unwrap();
    for day in 1..=8 {
        std::fs::write(logs.join(format!("elzevir.2000-01-{day:02}.jsonl")), "").unwrap();
    }
    std::fs::write(logs.join("unrelated.txt"), "preserve").unwrap();
    let (server, address, health) = start(root.path());
    let metadata: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.path().join("elzevir-runtime.json")).unwrap())
            .unwrap();
    assert_eq!(metadata["pid"], server.0.id());
    assert!(health.ip().is_loopback());
    let marker = "secret-that-must-never-be-logged";
    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    write!(stream, "GET /search?q={marker} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {marker}\r\nCookie: token={marker}\r\nConnection: close\r\n\r\n").unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200"));
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let files = std::fs::read_dir(&logs)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
            .collect::<Vec<_>>();
        assert_eq!(files.len(), 7);
        let rows: Vec<_> = files
            .iter()
            .flat_map(|file| {
                std::fs::read_to_string(file)
                    .unwrap()
                    .lines()
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .collect();
        assert!(rows.iter().all(|row| !row.contains(marker)));
        if !rows.is_empty() {
            for row in rows {
                let event: serde_json::Value = serde_json::from_str(&row).unwrap();
                assert_eq!(event["fields"]["institution"], "local");
                assert_eq!(event["fields"]["instance"], metadata["instance"]);
            }
            break;
        }
        assert!(Instant::now() < deadline, "operational event not written");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        std::fs::read_to_string(logs.join("unrelated.txt")).unwrap(),
        "preserve"
    );
    #[cfg(unix)]
    {
        assert_graceful_stop(server, address, health);
        let rows = log_text(&logs);
        assert!(!rows.contains(marker));
        assert!(rows.lines().any(
            |line| serde_json::from_str::<serde_json::Value>(line).unwrap()["fields"]["event"]
                == "server.stopped"
        ));
    }
}

fn log_text(directory: &Path) -> String {
    std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "jsonl")
        })
        .map(|path| std::fs::read_to_string(path).unwrap())
        .collect()
}

#[test]
fn startup_failure_is_flushed_without_logging_its_path() {
    let root = tempfile::tempdir().unwrap();
    let index = root.path().join("index");
    std::fs::write(&index, "preserve user file").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_elzevir"))
        .args(["serve", "--data-dir", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(
        std::fs::read_to_string(&index).unwrap(),
        "preserve user file"
    );
    let events = log_text(&root.path().join("logs"));
    assert!(!events.contains(root.path().to_str().unwrap()));
    assert!(events.lines().any(
        |line| serde_json::from_str::<serde_json::Value>(line).unwrap()["fields"]["event"]
            == "server.failed"
    ));
}

#[test]
fn an_existing_file_is_not_replaced_by_the_log_directory() {
    let root = tempfile::tempdir().unwrap();
    let logs = root.path().join("logs");
    std::fs::write(&logs, "preserve user file").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_elzevir"))
        .args(["serve", "--data-dir", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(std::fs::read_to_string(logs).unwrap(), "preserve user file");
}

#[cfg(unix)]
#[test]
fn symbolic_links_cannot_redirect_operational_logs() {
    for directory_link in [true, false] {
        let root = tempfile::tempdir().unwrap();
        let logs = root.path().join("logs");
        let target = root.path().join("target");
        if directory_link {
            std::fs::create_dir(&target).unwrap();
            std::os::unix::fs::symlink(&target, &logs).unwrap();
        } else {
            std::fs::create_dir(&logs).unwrap();
            std::fs::write(&target, "preserve user file").unwrap();
            std::os::unix::fs::symlink(&target, logs.join("elzevir.fixture.jsonl")).unwrap();
        }
        let output = Command::new(env!("CARGO_BIN_EXE_elzevir"))
            .args(["serve", "--data-dir", root.path().to_str().unwrap()])
            .output()
            .unwrap();
        assert!(!output.status.success());
        if directory_link {
            assert_eq!(std::fs::read_dir(&target).unwrap().count(), 0);
        } else {
            assert_eq!(
                std::fs::read_to_string(&target).unwrap(),
                "preserve user file"
            );
        }
    }
}
