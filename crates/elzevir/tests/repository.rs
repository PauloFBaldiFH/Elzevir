// SPDX-License-Identifier: GPL-3.0-or-later
//! Offline repository through the actual executable, across fresh processes.
#![expect(
    clippy::unwrap_used,
    reason = "integration fixture helpers fail the test on errors"
)]
use elzevir_app::{
    catalog::{CatalogCommand, WorkChange},
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
