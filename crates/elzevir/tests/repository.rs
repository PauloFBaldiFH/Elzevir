// SPDX-License-Identifier: GPL-3.0-or-later
//! Offline repository through the actual executable, across fresh processes.
#![expect(
    clippy::unwrap_used,
    reason = "integration fixture helpers fail the test on errors"
)]
use elzevir_app::{
    catalog::{CatalogCommand, CatalogReader, InstanceChange, InstanceUpdate, WorkChange},
    commands::WriteCommand,
    creating::new_record,
};
use elzevir_store::{ElzevirHandler, ElzevirStore, StoreConfig};
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

fn institution() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::File::create(dir.path().join("elzevir.lock")).unwrap();
    let store = ElzevirStore::open(
        &dir.path().join("elzevir.db"),
        &StoreConfig::default(),
        ElzevirHandler,
    )
    .unwrap();
    let record = new_record(vec![WorkChange::Title("Digital thesis".into())], vec![]).unwrap();
    store
        .submit(WriteCommand::Catalog(CatalogCommand::AddRecord(Box::new(
            record,
        ))))
        .unwrap()
        .wait()
        .unwrap();
    store.shutdown().unwrap();
    dir
}
fn cli(dir: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_elzevir"));
    cmd.arg("repository")
        .arg("--data-dir")
        .arg(dir)
        .args(["--min-free-bytes", "0"]);
    cmd
}
fn run(dir: &Path, args: &[&str]) -> String {
    let output = cli(dir).args(args).output().unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .trim_end()
        .to_owned()
}
fn refuses(dir: &Path, args: &[&str], code: &str) {
    let output = cli(dir).args(args).output().unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(code),
        "{:?}",
        output.stderr
    );
}
fn upload(
    dir: &Path,
    deposit: &str,
    file: &str,
    op: u32,
    expected: Option<&str>,
    data: &[u8],
) -> String {
    let input = dir.join("fixture");
    fs::write(&input, data).unwrap();
    let key = format!("{op:032x}");
    let mut args = vec![
        "upload",
        deposit,
        file,
        input.to_str().unwrap(),
        "--name",
        "thesis",
        "--media-type",
        "application/octet-stream",
        "--operation",
        &key,
    ];
    if let Some(seq) = expected {
        args.extend(["--expected-current-seq", seq]);
    }
    run(dir, &args)
}

#[test]
fn thesis_publication_retries_restrictions_deduplication_and_withdrawal() {
    let fixture = institution();
    let d = fixture.path();
    let deposit = run(d, &["draft", "1"]);
    let primary = run(
        d,
        &["add-file", &deposit, "Thesis PDF", "primary", "public"],
    );
    let sheet = run(
        d,
        &["add-file", &deposit, "Public data", "supplement", "public"],
    );
    let term = run(
        d,
        &["add-file", &deposit, "Consent", "administrative", "staff"],
    );
    for name in ["Theses", "Department"] {
        let collection = run(d, &["collection", name]);
        run(d, &["join", &deposit, &collection]);
    }
    refuses(d, &["publish", &deposit], "invalid_state");
    let pdf = upload(d, &deposit, &primary, 1, None, b"%PDF-example");
    upload(d, &deposit, &sheet, 2, None, b"a,b");
    upload(d, &deposit, &term, 3, None, b"a,b");
    assert!(run(d, &["verify", &deposit, &term, "1"]).contains("references=2"));
    refuses(d, &["show", &deposit, "--public"], "not_visible");
    run(d, &["publish", &deposit]);
    let page = run(d, &["show", &deposit, "--public"]);
    assert_eq!(
        page.lines()
            .filter(|s| s.starts_with("collection="))
            .count(),
        2
    );
    assert!(page.contains(&primary) && page.contains(&sheet) && !page.contains(&term));
    refuses(d, &["download", &deposit, &term, "--public"], "not_visible");
    assert_eq!(run(d, &["download", &deposit, &sheet, "--public"]), "a,b");
    upload(d, &deposit, &primary, 4, Some("1"), b"%PDF-revised");
    // Repeat the original operation after a later commit using a fresh process.
    assert_eq!(pdf, upload(d, &deposit, &primary, 1, None, b"%PDF-example"));
    refuses(
        d,
        &["download", &deposit, &primary, "--seq", "1", "--public"],
        "not_visible",
    );
    assert_eq!(
        run(d, &["download", &deposit, &primary, "--seq", "1"]),
        "%PDF-example"
    );
    refuses(d, &["retire", &deposit, &primary, "2"], "invalid_state");
    run(
        d,
        &["retire", &deposit, &primary, "2", "--withdraw-deposit"],
    );
    refuses(d, &["show", &deposit, "--public"], "withdrawn");
    refuses(
        d,
        &["download", &deposit, &primary, "--public"],
        "not_visible",
    );
    assert!(run(d, &["maintain", "--apply"]).contains("first_observations=0"));
    assert_eq!(
        run(d, &["download", &deposit, &primary, "--seq", "2"]),
        "%PDF-revised"
    );
    let hash = pdf
        .split_whitespace()
        .find_map(|s| s.strip_prefix("sha256="))
        .unwrap();
    let object = d
        .join("repository/objects/sha256")
        .join(hash.get(..2).unwrap())
        .join(hash.get(2..4).unwrap())
        .join(hash);
    fs::write(&object, b"%PDF-damaged").unwrap();
    refuses(
        d,
        &["verify", &deposit, &primary, "1"],
        "object_unavailable",
    );
    refuses(
        d,
        &["download", &deposit, &primary, "--seq", "1"],
        "object_unavailable",
    );
    fs::remove_file(object).unwrap();
    let output = cli(d)
        .args(["verify", &deposit, &primary, "1"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("integrity=Missing"));
}

#[test]
fn interrupted_process_releases_offline_lock_and_leaves_only_staging() {
    let fixture = institution();
    let d = fixture.path();
    let deposit = run(d, &["draft", "1"]);
    let file = run(d, &["add-file", &deposit, "PDF", "primary", "public"]);
    let args = [
        "upload",
        &deposit,
        &file,
        "-",
        "--operation",
        "00000000000000000000000000000001",
        "--name",
        "pdf",
        "--media-type",
        "application/pdf",
    ];
    let mut child = cli(d).args(args).stdin(Stdio::piped()).spawn().unwrap();
    let mut input = child.stdin.take().unwrap();
    let partial = b"partial upload";
    input.write_all(partial).unwrap();
    let staging = d.join("repository/staging");
    let deadline = Instant::now() + Duration::from_secs(30);
    // On Windows the directory entry may report a stale length until the writer
    // closes. Read the file itself to synchronize with the completed chunk write.
    while !staging
        .read_dir()
        .unwrap()
        .any(|entry| fs::read(entry.unwrap().path()).unwrap() == partial)
    {
        assert!(child.try_wait().unwrap().is_none());
        assert!(Instant::now() < deadline, "upload never staged its input");
        thread::sleep(Duration::from_millis(10));
    }
    refuses(d, &["maintain", "--apply"], "stop the institution");
    child.kill().unwrap();
    child.wait().unwrap();
    drop(input);
    assert_eq!(staging.read_dir().unwrap().count(), 1);
    assert!(run(d, &["maintain", "--apply"]).contains("temporary=0 orphan=0"));
    assert!(run(d, &["show", &deposit]).contains("current=None"));
    let temp = staging.read_dir().unwrap().next().unwrap().unwrap().path();
    fs::File::options()
        .write(true)
        .open(temp)
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH)
        .unwrap();
    assert!(run(d, &["maintain", "--apply"]).contains("temporary=1 orphan=0 recoverable_bytes=14"));
    assert!(upload(d, &deposit, &file, 1, None, b"complete").contains("seq=1"));
}

fn backup(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_elzevir"))
        .arg("backup")
        .args(args)
        .output()
        .unwrap()
}
fn checked_backup(args: &[&str]) -> serde_json::Value {
    let result = backup(args);
    assert!(
        result.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    if result.stdout.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&result.stdout).unwrap()
    }
}
#[test]
fn full_backup_moves_history_shared_objects_and_permissions_to_another_institution() {
    let src = institution();
    let s = src.path();
    let source = s.to_str().unwrap();
    let deposit = run(s, &["draft", "1"]);
    let primary = run(
        s,
        &["add-file", &deposit, "Thesis PDF", "primary", "public"],
    );
    let sheet = run(
        s,
        &["add-file", &deposit, "Public data", "supplement", "public"],
    );
    let term = run(
        s,
        &["add-file", &deposit, "Consent", "administrative", "staff"],
    );
    upload(s, &deposit, &primary, 1, None, b"first edition");
    upload(s, &deposit, &primary, 2, Some("1"), b"revised edition");
    upload(s, &deposit, &sheet, 3, None, b"research data");
    upload(s, &deposit, &term, 4, None, b"research data");
    run(s, &["publish", &deposit]);
    run(s, &["retire", &deposit, &primary, "1"]);
    let artifact = checked_backup(&["now", "--data-dir", source]);
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("thesis.age");
    let key = temp.path().join("recovery.key");
    let archive = archive.to_str().unwrap();
    let key = key.to_str().unwrap();
    checked_backup(&[
        "export",
        "--data-dir",
        source,
        "--id",
        artifact["id"].as_str().unwrap(),
        "--output",
        archive,
    ]);
    checked_backup(&["key", "--data-dir", source, "--output", key]);
    drop(src);
    let manifest = checked_backup(&["verify", "--input", archive, "--recovery-key", key]);
    assert_eq!(manifest["schema"], 3);
    let objects: Vec<_> = manifest["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["name"].as_str().unwrap().starts_with("object-"))
        .collect();
    assert_eq!(objects.len(), 3);
    let target = institution();
    let t = target.path();
    let restore = [
        "restore",
        "--confirm-restore",
        "--input",
        archive,
        "--recovery-key",
        key,
        "--data-dir",
        t.to_str().unwrap(),
    ];
    // A conflicting destination must keep its live database and damaged evidence.
    let local = run(t, &["draft", "1"]);
    // The first object is installed before the second one fails verification.
    let hash = objects[1]["sha256"].as_str().unwrap();
    let shard = t
        .join("repository/objects/sha256")
        .join(hash.get(..2).unwrap())
        .join(hash.get(2..4).unwrap());
    fs::create_dir_all(&shard).unwrap();
    let damaged = shard.join(hash);
    let wrong = vec![0; usize::try_from(objects[1]["bytes"].as_u64().unwrap()).unwrap()];
    fs::write(&damaged, &wrong).unwrap();
    let conflict = backup(&restore);
    assert!(!conflict.status.success());
    assert!(String::from_utf8_lossy(&conflict.stderr).contains(hash));
    assert!(run(t, &["show", &local]).contains("Draft"));
    assert_eq!(fs::read(&damaged).unwrap(), wrong);
    fs::remove_file(damaged).unwrap();
    assert!(run(t, &["maintain", "--apply"]).contains("first_observations=1 temporary=0 orphan=0"));
    checked_backup(&restore);
    checked_backup(&restore); // safely reuse every installed object
    assert_eq!(
        run(t, &["download", &deposit, &primary, "--public"]),
        "revised edition"
    );
    assert_eq!(
        run(t, &["download", &deposit, &primary, "--seq", "1"]),
        "first edition"
    );
    refuses(
        t,
        &["download", &deposit, &primary, "--seq", "1", "--public"],
        "not_visible",
    );
    assert_eq!(
        run(t, &["download", &deposit, &sheet, "--public"]),
        "research data"
    );
    refuses(t, &["download", &deposit, &term, "--public"], "not_visible");
    assert_eq!(run(t, &["download", &deposit, &term]), "research data");
}

#[test]
fn missing_or_corrupt_historical_bytes_prevent_backup_publication() {
    let src = institution();
    let d = src.path();
    let deposit = run(d, &["draft", "1"]);
    let file = run(d, &["add-file", &deposit, "PDF", "primary", "public"]);
    let first = upload(d, &deposit, &file, 1, None, b"original");
    upload(d, &deposit, &file, 2, Some("1"), b"updated");
    let hash = first
        .split_whitespace()
        .find_map(|s| s.strip_prefix("sha256="))
        .unwrap();
    let object = d
        .join("repository/objects/sha256")
        .join(hash.get(..2).unwrap())
        .join(hash.get(2..4).unwrap())
        .join(hash);
    let args = ["now", "--data-dir", d.to_str().unwrap()];
    fs::write(&object, b"corrupt!").unwrap();
    let corrupt = backup(&args);
    assert!(!corrupt.status.success());
    assert!(String::from_utf8_lossy(&corrupt.stderr).contains(hash));
    fs::remove_file(object).unwrap();
    let missing = backup(&args);
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains(hash));
    assert_eq!(fs::read_dir(d.join("backups")).unwrap().count(), 0);
}

#[test]
fn public_http_exposes_only_the_current_authorized_version() {
    let fixture = institution();
    let d = fixture.path();
    let deposit = run(d, &["draft", "1"]);
    let pdf = run(
        d,
        &["add-file", &deposit, "Public thesis", "primary", "public"],
    );
    let consent = run(
        d,
        &["add-file", &deposit, "Consent", "administrative", "staff"],
    );
    upload(d, &deposit, &pdf, 1, None, b"earlier");
    upload(d, &deposit, &pdf, 2, Some("1"), b"final thesis");
    upload(d, &deposit, &consent, 3, None, b"final thesis");
    run(d, &["publish", &deposit]);
    let (_server, host, agent) = start_http(d);
    let url = format!("{host}/repository/{deposit}");
    let mut bib = agent.get(format!("{host}/record/1")).call().unwrap();
    assert!(
        bib.body_mut()
            .read_to_string()
            .unwrap()
            .contains(&format!("/repository/{deposit}"))
    );
    let mut page = agent.get(&url).call().unwrap();
    assert_eq!(page.status(), 200);
    assert_eq!(page.headers()["cache-control"], "no-store");
    let html = page.body_mut().read_to_string().unwrap();
    assert!(html.contains("Public thesis") && html.contains(&pdf));
    assert!(!html.contains("Consent") && !html.contains(&consent));
    let current = format!("{url}/files/{pdf}/versions/2");
    let mut download = agent.get(&current).call().unwrap();
    assert_eq!(download.status(), 200);
    assert!(
        download.headers()["content-disposition"]
            .to_str()
            .unwrap()
            .starts_with("attachment")
    );
    assert_eq!(
        download.body_mut().read_to_string().unwrap(),
        "final thesis"
    );
    assert_eq!(
        agent.head(&current).call().unwrap().headers()["content-length"],
        "12"
    );
    for (id, seq) in [(&pdf, 1), (&consent, 1)] {
        let path = format!("{url}/files/{id}/versions/{seq}");
        assert_eq!(agent.get(&path).call().unwrap().status(), 404);
        assert_eq!(agent.head(&path).call().unwrap().status(), 404);
    }
}
fn start_http(d: &Path) -> (HttpServer, String, ureq::Agent) {
    let log = d.join("server-output");
    let output = fs::File::create(&log).unwrap();
    let mut server = HttpServer(
        Command::new(env!("CARGO_BIN_EXE_elzevir"))
            .args(["serve", "--data-dir"])
            .arg(d)
            .args(["--listen", "127.0.0.1:0"])
            .stdout(output.try_clone().unwrap())
            .stderr(output)
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    let url = loop {
        let text = fs::read_to_string(&log).unwrap();
        if let Some(address) = text
            .lines()
            .find_map(|s| s.strip_prefix("elzevir: listening on http://"))
        {
            break format!("http://{address}");
        }
        assert!(
            server.0.try_wait().unwrap().is_none() && Instant::now() < deadline,
            "{text}"
        );
        thread::sleep(Duration::from_millis(20));
    };
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .proxy(None)
            .http_status_as_error(false)
            .build(),
    );
    (server, url, agent)
}
struct HttpServer(std::process::Child);
impl Drop for HttpServer {
    fn drop(&mut self) {
        drop(self.0.kill());
        drop(self.0.wait());
    }
}

#[test]
fn opac_conceals_drafts_then_distinguishes_unavailable_bytes_and_withdrawal() {
    let fixture = institution();
    let d = fixture.path();
    let deposit = run(d, &["draft", "1"]);
    let file = run(d, &["add-file", &deposit, "Article", "primary", "public"]);
    let receipt = upload(d, &deposit, &file, 1, None, b"article");
    let path = format!("/repository/{deposit}");
    let version = format!("{path}/files/{file}/versions/1");
    let assert_status = |page, download, linked| {
        let (_server, host, agent) = start_http(d);
        let response = agent.get(format!("{host}{path}")).call().unwrap();
        assert_eq!(response.status().as_u16(), page);
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert_eq!(
            agent
                .head(format!("{host}{version}"))
                .call()
                .unwrap()
                .status()
                .as_u16(),
            download
        );
        let mut bib = agent.get(format!("{host}/record/1")).call().unwrap();
        assert_eq!(
            bib.body_mut().read_to_string().unwrap().contains(&path),
            linked
        );
    };
    assert_status(404, 404, false);
    run(d, &["publish", &deposit]);
    let hash = receipt
        .split_whitespace()
        .find_map(|entry| entry.strip_prefix("sha256="))
        .unwrap();
    let object = d
        .join("repository/objects/sha256")
        .join(hash.get(..2).unwrap())
        .join(hash.get(2..4).unwrap())
        .join(hash);
    fs::write(&object, b"changed").unwrap();
    assert_status(200, 503, true);
    fs::remove_file(object).unwrap();
    assert_status(200, 503, true);
    run(d, &["withdraw", &deposit]);
    assert_status(410, 404, false);
    let store = ElzevirStore::open(
        &d.join("elzevir.db"),
        &StoreConfig::default(),
        ElzevirHandler,
    )
    .unwrap();
    let instance = elzevir_domain::catalog::InstanceId(1);
    let row_version = store.record_version(instance).unwrap().unwrap();
    store
        .submit(WriteCommand::Catalog(CatalogCommand::UpdateInstance(
            Box::new(InstanceUpdate {
                instance,
                row_version,
                changes: vec![InstanceChange::OpacHidden(true)],
            }),
        )))
        .unwrap()
        .wait()
        .unwrap();
    store.shutdown().unwrap();
    assert_status(404, 404, false);
}
