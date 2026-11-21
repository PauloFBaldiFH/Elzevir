// SPDX-License-Identifier: GPL-3.0-or-later
//! Service-side browsing and retrieval. Only managed encrypted filenames enter the panel.
use super::{
    Connection, Native, Progress, Runner, archive, connection, destination, environment, lock,
    read, run, valid_id,
};
use elzevir_panel::{CloudTarget, RemoteCommand, RemotePage};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};
const PAGE: usize = 20;
const CACHE: &str = "cloud-listing.json";
const LAST: &str = "cloud-download.json";

pub(super) fn page(directory: &Path) -> Option<RemotePage> {
    serde_json::from_slice(&read(&directory.join(CACHE), 16384).ok()?).ok()
}
pub(super) fn notice(directory: &Path) -> Option<(CloudTarget, String)> {
    serde_json::from_slice(&read(&directory.join("cloud-read-result.json"), 2048).ok()?).ok()
}
pub(super) fn downloaded(directory: &Path) -> Option<(CloudTarget, PathBuf)> {
    let (target, id): (CloudTarget, String) =
        serde_json::from_slice(&read(&directory.join(LAST), 2048).ok()?).ok()?;
    if !valid_id(&id) {
        return None;
    }
    let path = directory
        .join("cloud-downloads")
        .join(format!("elzevir-{id}.age"));
    archive::regular(&path).ok()?;
    Some((target, path))
}
fn publish(directory: &Path, file: &str, value: &impl serde::Serialize) -> Result<(), String> {
    crate::broker::publish(
        directory,
        &directory.join(file),
        &serde_json::to_vec(value).map_err(|_| "cannot encode cloud observation")?,
    )
}
pub(super) struct Session<'a, R> {
    directory: &'a Path,
    connection: &'a Connection,
    program: &'a Path,
    runner: &'a R,
    progress: Option<Progress>,
}
impl<'a, R: Runner> Session<'a, R> {
    pub(super) fn new(
        directory: &'a Path,
        connection: &'a Connection,
        program: &'a Path,
        runner: &'a R,
        progress: Option<Progress>,
    ) -> Self {
        Self {
            directory,
            connection,
            program,
            runner,
            progress,
        }
    }
    pub(super) fn upload(&self, folder: &str, source: &Path, digest: &str) -> Result<(), String> {
        archive::regular(source)?;
        let name = source
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("invalid archive name")?;
        let target = destination(self.connection, folder, name)?;
        self.request(
            vec![
                "copyto".into(),
                source.as_os_str().into(),
                target.clone().into(),
                "--immutable".into(),
                "--retries".into(),
                "1".into(),
                "--low-level-retries".into(),
                "2".into(),
                "--stats".into(),
                "0".into(),
            ],
            65536,
        )?;
        self.verify_remote(&target, digest)
    }
    fn request(
        &self,
        args: Vec<OsString>,
        limit: u64,
    ) -> Result<crate::process_runner::Output, String> {
        run(
            self.runner,
            self.program,
            args,
            environment(self.directory, self.connection),
            None,
            self.progress.clone(),
            limit,
        )
    }
    fn quota(&self) -> Option<u64> {
        let output = self
            .request(
                vec![
                    "about".into(),
                    format!("{}:", super::remote(self.connection)).into(),
                    "--json".into(),
                ],
                65536,
            )
            .ok()?;
        serde_json::from_slice::<serde_json::Value>(&output.stdout)
            .ok()?
            .get("free")?
            .as_u64()
    }
    fn browse(&self, target: &CloudTarget, number: u16) -> Result<(), String> {
        publish(self.directory, CACHE, &Option::<RemotePage>::None)?;
        let output = self.request(
            vec![
                "lsjson".into(),
                format!("{}:{}", super::remote(self.connection), target.folder).into(),
                "--files-only".into(),
                "--max-depth".into(),
                "1".into(),
                "--include".into(),
                "/elzevir-*.age".into(),
                "--no-mimetype".into(),
            ],
            2 * 1024 * 1024,
        )?;
        let listing = parse_page(target, &output.stdout, number, self.quota())?;
        publish(self.directory, CACHE, &listing)
    }
    fn download(&self, target: &CloudTarget, id: &str) -> Result<(), String> {
        let listing = page(self.directory)
            .filter(|p| p.target == *target)
            .ok_or("refresh this folder before downloading")?;
        let size = listing
            .files
            .iter()
            .find(|(key, _)| key == id && valid_id(id))
            .ok_or("select a listed artifact")?
            .1;
        if size == 0
            || size > archive::MAX_DB
            || fs4::available_space(self.directory).map_err(|_| "disk space unavailable")? < size
        {
            return Err("insufficient space or invalid remote size".into());
        }
        let folder = self.directory.join("cloud-downloads");
        crate::backup_cli::make_directory(&folder)?;
        let name = format!("elzevir-{id}.age");
        let remote = destination(self.connection, &target.folder, &name)?;
        let stage = crate::private_temp::directory_in(&folder)
            .map_err(|_| "cannot stage cloud download")?;
        let path = stage.path().join(&name);
        self.request(
            vec![
                "copyto".into(),
                remote.clone().into(),
                path.as_os_str().into(),
                "--max-transfer".into(),
                format!("{}B", size.saturating_add(1)).into(),
                "--cutoff-mode".into(),
                "hard".into(),
                "--retries".into(),
                "1".into(),
                "--stats".into(),
                "0".into(),
            ],
            65536,
        )?;
        if archive::regular(&path)? != size {
            return Err("partial cloud download".into());
        }
        let progress = self.progress.clone();
        let control: crate::cancellation::Control =
            std::sync::Arc::new(move || progress.as_ref().is_some_and(|p| p(&[]).is_err()));
        let digest = archive::hash(&path, &control)?;
        if crate::cloud_plans::expected_digest(self.directory, target, id)?
            .is_some_and(|d| d != digest)
        {
            return Err("remote content differs from verified upload".into());
        }
        self.verify_remote(&remote, &digest)?;
        let output = folder.join(&name);
        if output.exists() {
            archive::regular(&output)?;
            if archive::hash(&output, &control)? != digest {
                return Err("cached download name collision".into());
            }
        } else {
            crate::file_owner::private_tree(&path, self.directory)?;
            std::fs::File::open(&path)
                .and_then(|file| file.sync_all())
                .map_err(|_| "cannot sync download")?;
            std::fs::hard_link(path, output).map_err(|_| "cannot publish verified download")?;
        }
        publish(self.directory, LAST, &(target, id))
    }
    fn verify_remote(&self, remote: &str, digest: &str) -> Result<(), String> {
        let output = self.request(
            vec![
                "hashsum".into(),
                "SHA-256".into(),
                remote.into(),
                "--download".into(),
                "--retries".into(),
                "1".into(),
            ],
            65536,
        )?;
        let text = std::str::from_utf8(&output.stdout).map_err(|_| "invalid remote hash")?;
        if text.lines().count() != 1 || text.split_whitespace().next() != Some(digest) {
            return Err("remote checksum mismatch".into());
        }
        Ok(())
    }
}

pub(super) fn verify_remote(
    directory: &Path,
    connection: &Connection,
    program: &Path,
    target: &str,
    digest: &str,
    progress: Option<Progress>,
) -> Result<(), String> {
    Session {
        directory,
        connection,
        program,
        runner: &Native,
        progress,
    }
    .verify_remote(target, digest)
}
pub(super) fn delete_confirmed(
    directory: &Path,
    connection: &Connection,
    program: &Path,
    target: &str,
    digest: &str,
    progress: Option<Progress>,
) -> Result<(), String> {
    let session = Session {
        directory,
        connection,
        program,
        runner: &Native,
        progress,
    };
    session.verify_remote(target, digest)?;
    session.request(
        vec![
            "deletefile".into(),
            target.into(),
            "--retries".into(),
            "1".into(),
        ],
        65536,
    )?;
    Ok(())
}
fn parse_page(
    target: &CloudTarget,
    bytes: &[u8],
    page: u16,
    free: Option<u64>,
) -> Result<RemotePage, String> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct Entry {
        path: String,
        size: i64,
        is_dir: bool,
    }
    let entries: Vec<Entry> =
        serde_json::from_slice(bytes).map_err(|_| "invalid remote listing")?;
    if entries.len() > 8192 {
        return Err("remote folder exceeds listing limit".into());
    }
    let mut files = std::collections::BTreeMap::new();
    for e in entries {
        if !e.is_dir
            && let Some(id) = e
                .path
                .strip_prefix("elzevir-")
                .and_then(|s| s.strip_suffix(".age"))
                .filter(|s| valid_id(s))
            && let Ok(size) = u64::try_from(e.size)
            && size <= archive::MAX_DB
            && files.insert(id.to_owned(), size).is_some()
        {
            return Err("ambiguous remote object".into());
        }
    }
    let pages = u16::try_from(files.len().div_ceil(PAGE).max(1)).map_err(|_| "too many pages")?;
    if page == 0 || page > pages {
        return Err("invalid remote page".into());
    }
    Ok(RemotePage {
        target: target.clone(),
        page,
        pages,
        free,
        files: files
            .into_iter()
            .skip(usize::from(page - 1) * PAGE)
            .take(PAGE)
            .collect(),
    })
}
pub(super) fn execute(
    directory: &Path,
    target: &CloudTarget,
    command: &RemoteCommand,
    progress: Option<Progress>,
) -> Result<(), String> {
    super::require_limited_account()?;
    crate::cloud_plans::reviewed(directory, target)?;
    if let RemoteCommand::Send(id) = command {
        let control: crate::cancellation::Control =
            std::sync::Arc::new(move || progress.as_ref().is_some_and(|p| p(&[]).is_err()));
        return crate::cloud_plans::resend(directory, std::slice::from_ref(target), id, &control);
    }
    if matches!(command, RemoteCommand::Download(_)) {
        publish(directory, LAST, &Option::<String>::None)?;
    }
    let result = read_remote(directory, target, command, progress);
    let message = match (command, result.is_ok()) {
        (RemoteCommand::Download(_), true) => "cloud-downloaded",
        (RemoteCommand::Download(_), false) => "cloud-download-failed",
        (_, true) => "cloud-listed",
        (_, false) => "cloud-list-failed",
    };
    publish(directory, "cloud-read-result.json", &(target, message))?;
    result
}

fn read_remote(
    directory: &Path,
    target: &CloudTarget,
    command: &RemoteCommand,
    progress: Option<Progress>,
) -> Result<(), String> {
    let c = connection(directory, &target.connection)?;
    if !c.ready {
        return Err("connection authorization pending".into());
    }
    let _lock = lock(directory, &c)?;
    let lease = crate::managed_tools::lease(crate::managed_tools::Tool::Rclone)?;
    let session = Session {
        directory,
        connection: &c,
        program: &lease.path,
        runner: &Native,
        progress,
    };
    match command {
        RemoteCommand::Browse(page) => session.browse(target, *page),
        RemoteCommand::Download(id) => session.download(target, id),
        RemoteCommand::Create | RemoteCommand::Send(_) => {
            Err("invalid remote read operation".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process_runner::{Output, Request};
    use sha2::Digest as _;
    fn connection_fixture() -> Connection {
        Connection {
            id: "a".repeat(32),
            name: "Test".into(),
            provider: "alias".into(),
            config_id: "b".repeat(32),
            ready: true,
            parent: None,
        }
    }
    fn target() -> CloudTarget {
        CloudTarget {
            plan: "c".repeat(32),
            connection: "a".repeat(32),
            folder: "Library".into(),
        }
    }
    struct Fake {
        mode: &'static str,
    }
    impl Runner for Fake {
        fn run(&self, request: Request) -> Result<Output, String> {
            let id = "d".repeat(32);
            let name = format!("elzevir-{id}.age");
            let mut success = true;
            let stdout = match request.args.first().and_then(|v| v.to_str()).unwrap() {
                "about" => {
                    success = false;
                    Vec::new()
                }
                "lsjson" if self.mode == "expired" => {
                    success = false;
                    Vec::new()
                }
                "lsjson" => {
                    serde_json::to_vec(&serde_json::json!([{"Path":name,"IsDir":false,"Size":4}]))
                        .unwrap()
                }
                "copyto" => {
                    let path = request.args.get(2).unwrap();
                    std::fs::write(
                        path,
                        if self.mode == "partial" {
                            &b"ab"[..]
                        } else {
                            b"abcd"
                        },
                    )
                    .unwrap();
                    Vec::new()
                }
                "hashsum" => format!(
                    "{:x}  {name}\n",
                    sha2::Sha256::digest(if self.mode == "mismatch" {
                        b"xxxx"
                    } else {
                        b"abcd"
                    })
                )
                .into_bytes(),
                _ => return Err("unexpected command".into()),
            };
            Ok(Output { success, stdout })
        }
    }
    #[test]
    fn missing_quota_expired_credentials_and_partial_or_changed_downloads() {
        let dir = tempfile::tempdir().unwrap();
        let connection = connection_fixture();
        let target = target();
        let id = "d".repeat(32);
        for mode in ["partial", "mismatch", "expired", "ok"] {
            let runner = Fake { mode };
            let session = Session {
                directory: dir.path(),
                connection: &connection,
                program: Path::new("fixture"),
                runner: &runner,
                progress: None,
            };
            let result = session.browse(&target, 1);
            if mode == "expired" {
                assert!(result.is_err());
                assert!(page(dir.path()).is_none());
                continue;
            }
            result.unwrap();
            assert!(page(dir.path()).unwrap().free.is_none());
            let download = session.download(&target, &id);
            assert_eq!(download.is_ok(), mode == "ok");
            assert_eq!(downloaded(dir.path()).is_some(), mode == "ok");
            if mode != "ok" {
                assert_eq!(
                    std::fs::read_dir(dir.path().join("cloud-downloads"))
                        .unwrap()
                        .count(),
                    0
                );
            }
        }
    }
    #[test]
    fn lists_only_bounded_managed_names_and_orders_pages() {
        let target = target();
        let entries=(0..25).rev().map(|n|serde_json::json!({"Path":format!("elzevir-{n:032x}.age"),"Size":42,"IsDir":false})).collect::<Vec<_>>();
        let bytes = serde_json::to_vec(&entries).unwrap();
        let page = parse_page(&target, &bytes, 2, Some(0)).unwrap();
        assert_eq!((page.pages, page.files.len(), page.free), (2, 5, Some(0)));
        assert!(parse_page(&target, &bytes, 3, None).is_err());
        assert!(
            parse_page(&target, b"[]", 1, None)
                .unwrap()
                .files
                .is_empty()
        );
        let nested=br#"[{"Path":"../elzevir-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.age","Size":1,"IsDir":false}]"#;
        assert!(
            parse_page(&target, nested, 1, None)
                .unwrap()
                .files
                .is_empty()
        );
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "requires /usr/bin/rclone"]
    fn local_rclone_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let remote = tempfile::tempdir().unwrap();
        let connection = connection_fixture();
        let target = target();
        std::fs::create_dir(dir.path().join("secrets")).unwrap();
        std::fs::write(
            super::super::config(dir.path(), &connection),
            format!(
                "[{}]\ntype = alias\nremote = {}\n",
                super::super::remote(&connection),
                remote.path().display()
            ),
        )
        .unwrap();
        let id = "d".repeat(32);
        let source = dir.path().join(format!("elzevir-{id}.age"));
        std::fs::write(&source, b"round trip").unwrap();
        let program = Path::new("/usr/bin/rclone");
        let session = Session {
            directory: dir.path(),
            connection: &connection,
            program,
            runner: &Native,
            progress: None,
        };
        let digest = format!("{:x}", sha2::Sha256::digest(b"round trip"));
        session.upload(&target.folder, &source, &digest).unwrap();
        session.browse(&target, 1).unwrap();
        session.download(&target, &id).unwrap();
        assert_eq!(
            std::fs::read(downloaded(dir.path()).unwrap().1).unwrap(),
            b"round trip"
        );
        let object = destination(
            &connection,
            &target.folder,
            source.file_name().unwrap().to_str().unwrap(),
        )
        .unwrap();
        assert!(
            delete_confirmed(
                dir.path(),
                &connection,
                program,
                &object,
                &"0".repeat(64),
                None
            )
            .is_err()
        );
        let stored = remote
            .path()
            .join(&target.folder)
            .join(source.file_name().unwrap());
        assert!(stored.is_file());
        delete_confirmed(dir.path(), &connection, program, &object, &digest, None).unwrap();
        assert!(!stored.exists());
    }
}
