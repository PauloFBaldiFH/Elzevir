// SPDX-License-Identifier: GPL-3.0-or-later
//! The live server advertises a private, instance-bound IPC endpoint.
#![expect(
    clippy::unwrap_used,
    reason = "integration helpers fail tests on error"
)]

use std::path::Path;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        drop(self.0.kill());
        drop(self.0.wait());
    }
}

fn start(root: &Path) -> (Server, serde_json::Value) {
    let log = std::fs::File::create(root.join("stdout.log")).unwrap();
    let mut child = Server(
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
        if let Ok(text) = std::fs::read_to_string(root.join("elzevir-runtime.json"))
            && let Ok(info) = serde_json::from_str(&text)
        {
            return (child, info);
        }
        assert!(child.0.try_wait().unwrap().is_none());
        assert!(Instant::now() < deadline, "startup timed out");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn server_advertises_native_local_admin_and_status_uses_it() {
    let root = tempfile::tempdir().unwrap();
    let (_server, info) = start(root.path());
    assert!(
        info["admin"].is_object(),
        "no administrative endpoint: {info}"
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let output = Command::new(env!("CARGO_BIN_EXE_elzevir"))
            .args([
                "status",
                "--data-dir",
                root.path().to_str().unwrap(),
                "--json",
            ])
            .output()
            .unwrap();
        if output.status.success() {
            let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(report["health"]["instance"], info["instance"]);
            break;
        }
        assert!(Instant::now() < deadline, "IPC status never became ready");
        std::thread::sleep(Duration::from_millis(20));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = Path::new(info["admin"]["path"].as_str().unwrap());
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
    #[cfg(windows)]
    assert_eq!(info["admin"]["transport"], "windows");
}

async fn connect(info: &serde_json::Value) -> interprocess::local_socket::tokio::Stream {
    use interprocess::local_socket::tokio::prelude::*;
    #[cfg(unix)]
    let name = Path::new(
        info.get("admin")
            .unwrap()
            .get("path")
            .unwrap()
            .as_str()
            .unwrap(),
    )
    .to_fs_name::<interprocess::local_socket::GenericFilePath>()
    .unwrap();
    #[cfg(windows)]
    let name = info
        .get("admin")
        .unwrap()
        .get("name")
        .unwrap()
        .as_str()
        .unwrap()
        .to_ns_name::<interprocess::local_socket::GenericNamespaced>()
        .unwrap();
    interprocess::local_socket::tokio::Stream::connect(name)
        .await
        .unwrap()
}

async fn exchange(info: &serde_json::Value, bytes: &[u8]) -> serde_json::Value {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
    tokio::time::timeout(Duration::from_secs(5), async {
        let stream = connect(info).await;
        let mut writer = &stream;
        writer.write_all(bytes).await.unwrap();
        let mut response = Vec::new();
        tokio::io::BufReader::new((&stream).take(4097))
            .read_until(b'\n', &mut response)
            .await
            .unwrap();
        serde_json::from_slice(&response).unwrap()
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn typed_protocol_refuses_invalid_commands_versions_and_instances() {
    let root = tempfile::tempdir().unwrap();
    let (_server, info) = start(root.path());
    let base = serde_json::json!({"schema":1,"instance":info["instance"],"id":7,"command":{"type":"status"}});
    let valid = exchange(&info, format!("{base}\n").as_bytes()).await;
    assert_eq!(valid["id"], 7);
    assert_eq!(valid["instance"], info["instance"]);
    assert_eq!(valid["result"]["type"], "status");
    let mut version = base.clone();
    version["schema"] = 2.into();
    let mut instance = base.clone();
    instance["instance"] = "different".into();
    let mut command = base.clone();
    command["command"]["type"] = "drop_database".into();
    let mut extra = base;
    extra["secret"] = "never-echo-this-secret".into();
    for (request, expected) in [
        (version, "unsupported_schema"),
        (instance, "instance_mismatch"),
        (command, "invalid_request"),
        (extra, "invalid_request"),
    ] {
        let response = exchange(&info, format!("{request}\n").as_bytes()).await;
        assert_eq!(response["result"]["type"], "error");
        assert_eq!(response["result"]["code"], expected);
        assert!(!response.to_string().contains("never-echo-this-secret"));
    }
    for bytes in [vec![b'x'; 32_769], vec![0xff, b'\n']] {
        let response = exchange(&info, &bytes).await;
        assert_eq!(response["result"]["code"], "invalid_request");
    }
}

#[tokio::test]
async fn incomplete_request_expires_without_stopping_the_server() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let root = tempfile::tempdir().unwrap();
    let (_server, info) = start(root.path());
    let stream = connect(&info).await;
    let mut writer = &stream;
    writer.write_all(b"{").await.unwrap();
    let mut bytes = Vec::new();
    tokio::time::timeout(
        Duration::from_secs(5),
        (&stream).take(4097).read_to_end(&mut bytes),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(bytes.is_empty());
    let request = serde_json::json!({"schema":1,"instance":info["instance"],"id":9,"command":{"type":"status"}});
    let response = exchange(&info, format!("{request}\n").as_bytes()).await;
    assert_eq!(response["id"], 9);
    assert_eq!(response["result"]["type"], "status");
}

#[tokio::test]
async fn integrity_check_runs_through_the_existing_server() {
    let root = tempfile::tempdir().unwrap();
    let (_server, info) = start(root.path());
    let request = serde_json::json!({"schema":1,"instance":info["instance"],"id":23,"command":{"type":"diagnose"}});
    let response = exchange(&info, format!("{request}\n").as_bytes()).await;
    assert_eq!(response["id"], 23);
    assert_eq!(response["result"]["type"], "diagnose");
    assert_eq!(response["result"]["pid"], info["pid"]);
    assert_eq!(response["result"]["quick_check"], "ok");
}

#[cfg(unix)]
#[test]
fn insecure_ipc_permissions_do_not_fall_back_to_http_health() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let (mut server, info) = start(root.path());
    let path = Path::new(info["admin"]["path"].as_str().unwrap());
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o666)).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_elzevir"))
        .args([
            "status",
            "--data-dir",
            root.path().to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["state"], "unavailable");
    assert_eq!(report["reason"], "admin_unavailable");
    assert!(server.0.try_wait().unwrap().is_none());
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
}

#[cfg(unix)]
#[test]
fn sigterm_closes_the_database_and_removes_owned_endpoints() {
    let root = tempfile::tempdir().unwrap();
    let (mut server, info) = start(root.path());
    let pid = rustix::process::Pid::from_raw(i32::try_from(server.0.id()).unwrap()).unwrap();
    rustix::process::kill_process(pid, rustix::process::Signal::TERM).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = server.0.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "SIGTERM did not finish");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(status.success());
    assert!(!root.path().join("elzevir-runtime.json").exists());
    assert!(!Path::new(info["admin"]["path"].as_str().unwrap()).exists());
    assert!(std::fs::metadata(root.path().join("elzevir.db-wal")).map_or(true, |m| m.len() == 0));
    assert!(
        Command::new(env!("CARGO_BIN_EXE_elzevir"))
            .args(["export", "--data-dir", root.path().to_str().unwrap()])
            .output()
            .unwrap()
            .status
            .success()
    );
}

#[tokio::test]
async fn private_channel_creates_a_new_administrator_and_refuses_a_duplicate() {
    let root = tempfile::tempdir().unwrap();
    let (_server, info) = start(root.path());
    let command = serde_json::json!({"schema":1,"instance":info["instance"],"id":34,"command":{"type":"create_administrator","login":"test.admin","password":"fixture-password"}});
    let bytes = format!("{command}\n");
    let response = exchange(&info, bytes.as_bytes()).await;
    assert_eq!(response["result"]["type"], "administrator_created");
    let duplicate = exchange(&info, bytes.as_bytes()).await;
    assert_eq!(duplicate["result"]["type"], "error");
}
