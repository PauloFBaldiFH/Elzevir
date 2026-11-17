// SPDX-License-Identifier: GPL-3.0-or-later
//! Optional official binaries, verified before extraction or execution.
use crate::{
    backup_archive as archive,
    process_runner::{Request, Runner},
    release::Download,
};
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};
const MAX_TOOL: u64 = 256 * 1024 * 1024;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Tool {
    Cloudflared,
    Rclone,
}
impl Tool {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Cloudflared => "cloudflared",
            Self::Rclone => "rclone",
        }
    }
    fn repository(self) -> &'static str {
        match self {
            Self::Cloudflared => "cloudflare/cloudflared",
            Self::Rclone => "rclone/rclone",
        }
    }
    pub(crate) fn filename(self) -> String {
        format!("{}{}", self.name(), if cfg!(windows) { ".exe" } else { "" })
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Installed {
    schema: u8,
    tool: Tool,
    pub version: String,
    sha256: String,
}
#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}
#[derive(Deserialize)]
struct Asset {
    name: String,
    digest: Option<String>,
    size: u64,
}
fn valid_version(value: &str) -> bool {
    value.len() <= 64
        && !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
        && !value.contains("..")
}
fn asset_name(tool: Tool, version: &str) -> Result<String, String> {
    let architecture = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        _ => return Err("tool architecture unsupported".to_owned()),
    };
    let platform = match std::env::consts::OS {
        "linux" => "linux",
        "windows" => "windows",
        _ => return Err("tool platform unsupported".to_owned()),
    };
    Ok(match tool {
        Tool::Cloudflared => format!(
            "cloudflared-{platform}-{architecture}{}",
            if cfg!(windows) { ".exe" } else { "" }
        ),
        Tool::Rclone => format!("rclone-{version}-{platform}-{architecture}.zip"),
    })
}

fn root(create: bool) -> Result<PathBuf, String> {
    #[cfg(target_os = "linux")]
    let path = PathBuf::from("/usr/local/lib/elzevir-tools");
    #[cfg(windows)]
    let path = PathBuf::from(
        std::env::var_os("ProgramW6432")
            .or_else(|| std::env::var_os("ProgramFiles"))
            .ok_or("program directory unavailable")?,
    )
    .join("ElzevirTools");
    #[cfg(not(any(target_os = "linux", windows)))]
    return Err("managed tools unavailable on this platform".to_owned());
    #[cfg(any(target_os = "linux", windows))]
    {
        // Reject existing links and writable ancestors before creating a privileged path.
        for parent in path.ancestors() {
            match std::fs::symlink_metadata(parent) {
                Ok(metadata) if metadata.is_dir() && !metadata.is_symlink() => {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::MetadataExt;
                        if rustix::process::geteuid().is_root()
                            && (metadata.uid() != 0 || metadata.mode() & 0o022 != 0)
                        {
                            return Err("tool program directory must be administrator controlled"
                                .to_owned());
                        }
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                _ => return Err("tool program path must contain regular directories".to_owned()),
            }
        }
        if create {
            std::fs::create_dir_all(&path)
                .map_err(|_| "tool installation requires program-directory write permission")?;
            crate::file_owner::program_directory(&path)?;
        }
        Ok(path)
    }
}
fn record_path(tool: Tool, directory: &Path) -> PathBuf {
    directory.join(format!(".{}.json", tool.name()))
}
fn read_record(tool: Tool, directory: &Path) -> Result<Installed, String> {
    let path = record_path(tool, directory);
    if archive::regular(&path)? > 4096 {
        return Err("tool record exceeds limit".to_owned());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(4097).read_to_end(&mut bytes))
        .map_err(|_| "tool record unavailable")?;
    let record: Installed = serde_json::from_slice(&bytes).map_err(|_| "invalid tool record")?;
    if record.schema != 1
        || record.tool != tool
        || !valid_version(&record.version)
        || record.sha256.len() != 64
        || !record.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("invalid tool record".to_owned());
    }
    Ok(record)
}
fn binary_at(tool: Tool, directory: &Path) -> Result<PathBuf, String> {
    if std::fs::symlink_metadata(tool_journal(tool, directory)).is_ok() {
        return Err("complete the interrupted tool installation before use".to_owned());
    }
    let record = read_record(tool, directory)?;
    let path = directory.join(tool.filename());
    if archive::regular(&path)? > MAX_TOOL || crate::release::hash(&path)? != record.sha256 {
        return Err("installed tool checksum mismatch".to_owned());
    }
    Ok(path)
}
pub(crate) fn binary(tool: Tool) -> Result<PathBuf, String> {
    binary_at(tool, &root(false)?)
}
pub(crate) struct Lease {
    pub(crate) path: PathBuf,
    _lock: std::fs::File,
}
pub(crate) fn lease(tool: Tool) -> Result<Lease, String> {
    lease_at(tool, &root(false)?)
}
fn lease_at(tool: Tool, directory: &Path) -> Result<Lease, String> {
    let lock_path = directory.join(format!(".{}.lock", tool.name()));
    archive::regular(&lock_path)?;
    let lock = std::fs::File::open(lock_path).map_err(|_| "tool lock unavailable")?;
    lock.try_lock_shared()
        .map_err(|_| "optional tool is being updated")?;
    Ok(Lease {
        path: binary_at(tool, directory)?,
        _lock: lock,
    })
}
pub(crate) fn installed(tool: Tool) -> Result<Installed, String> {
    let directory = root(false)?;
    binary_at(tool, &directory)?;
    read_record(tool, &directory)
}
pub(crate) fn install(tool: Tool) -> Result<Installed, String> {
    crate::service::require_operator()?;
    install_at(
        tool,
        &root(true)?,
        &crate::release::Http,
        &crate::process_runner::Native,
    )
}

pub(crate) fn ensure(tool: Tool) -> Result<PathBuf, String> {
    if let Ok(path) = binary(tool) {
        return Ok(path);
    }
    install(tool)?;
    binary(tool)
}

fn installation_lock(tool: Tool, directory: &Path) -> Result<std::fs::File, String> {
    let lock_path = directory.join(format!(".{}.lock", tool.name()));
    if std::fs::symlink_metadata(&lock_path).is_ok() {
        archive::regular(&lock_path)?;
    }
    let lock = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)
        .map_err(|_| "tool lock unavailable")?;
    lock.try_lock()
        .map_err(|_| "optional tool is in use or another installation is running")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        lock.set_permissions(std::fs::Permissions::from_mode(0o644))
            .map_err(|_| "cannot set tool lock permissions")?;
    }
    Ok(lock)
}

fn install_at(
    tool: Tool,
    directory: &Path,
    source: &impl Download,
    runner: &impl Runner,
) -> Result<Installed, String> {
    let metadata =
        std::fs::symlink_metadata(directory).map_err(|_| "tool program directory unavailable")?;
    if !metadata.is_dir() || metadata.is_symlink() {
        return Err("invalid tool program directory".to_owned());
    }
    let _lock = installation_lock(tool, directory)?;
    recover_publication(tool, directory)?;
    let mut bytes = Vec::new();
    source.fetch(
        &format!(
            "https://api.github.com/repos/{}/releases/latest",
            tool.repository()
        ),
        &mut bytes,
        2 * 1024 * 1024,
    )?;
    let release: Release =
        serde_json::from_slice(&bytes).map_err(|_| "invalid official tool release")?;
    if !valid_version(&release.tag_name) || release.assets.len() > 512 {
        return Err("invalid official tool version or asset count".to_owned());
    }
    let name = asset_name(tool, &release.tag_name)?;
    let matches = release
        .assets
        .iter()
        .filter(|asset| asset.name == name)
        .collect::<Vec<_>>();
    let [asset] = matches.as_slice() else {
        return Err("official tool package absent or duplicated for this platform".to_owned());
    };
    let expected = asset
        .digest
        .as_deref()
        .and_then(|digest| digest.strip_prefix("sha256:"))
        .ok_or("official tool checksum unavailable")?;
    if expected.len() != 64
        || !expected.bytes().all(|byte| byte.is_ascii_hexdigit())
        || asset.size == 0
        || asset.size > MAX_TOOL
    {
        return Err("invalid official tool checksum or size".to_owned());
    }
    let scratch = tempfile::tempdir_in(directory).map_err(|_| "cannot stage official tool")?;
    let package = scratch.path().join(&name);
    let mut file = std::fs::File::create(&package).map_err(|_| "cannot stage tool package")?;
    source.fetch(
        &format!(
            "https://github.com/{}/releases/download/{}/{}",
            tool.repository(),
            release.tag_name,
            name
        ),
        &mut file,
        MAX_TOOL,
    )?;
    file.sync_all().map_err(|_| "cannot sync tool download")?;
    drop(file);
    if archive::regular(&package)? != asset.size
        || crate::release::hash(&package)? != expected.to_ascii_lowercase()
    {
        return Err("official tool checksum mismatch".to_owned());
    }
    let staged = match tool {
        Tool::Cloudflared => package,
        Tool::Rclone => extract_rclone(&package, scratch.path(), &release.tag_name)?,
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))
            .map_err(|_| "cannot prepare official tool executable")?;
    }
    let staged_hash = crate::release::hash(&staged)?;
    check_version(runner, &staged, tool, &release.tag_name)?;
    if crate::release::hash(&staged)? != staged_hash {
        return Err("official tool changed during version verification".to_owned());
    }
    let record = Installed {
        schema: 1,
        tool,
        version: release.tag_name,
        sha256: staged_hash,
    };
    publish(tool, directory, &staged, &record)?;
    Ok(record)
}

fn check_version(
    runner: &impl Runner,
    staged: &Path,
    tool: Tool,
    version: &str,
) -> Result<(), String> {
    let checked = runner.run(Request {
        environment: None,
        progress: None,
        program: staged.to_owned(),
        args: vec!["--version".into()],
        input: None,
        timeout: Duration::from_secs(10),
        limit: 4096,
    })?;
    let output = std::str::from_utf8(&checked.stdout).map_err(|_| "invalid tool version output")?;
    let first = output.lines().next().ok_or("tool version output absent")?;
    let expected_version = match tool {
        Tool::Cloudflared => format!("cloudflared version {version}"),
        Tool::Rclone => format!("rclone {version}"),
    };
    if !checked.success
        || first
            .strip_prefix(&expected_version)
            .is_none_or(|suffix| !suffix.is_empty() && !suffix.starts_with(char::is_whitespace))
    {
        return Err("official tool version mismatch".to_owned());
    }
    Ok(())
}

fn publish(tool: Tool, directory: &Path, staged: &Path, record: &Installed) -> Result<(), String> {
    let target = directory.join(tool.filename());
    let old = if std::fs::symlink_metadata(&target).is_ok() {
        binary_at(tool, directory)?;
        Some(read_record(tool, directory)?)
    } else {
        if record_path(tool, directory).exists() {
            return Err("tool record without executable requires recovery".to_owned());
        }
        None
    };
    let previous = tool_previous(tool, directory);
    if std::fs::symlink_metadata(&previous).is_ok() {
        return Err("previous tool recovery file must be new".to_owned());
    }
    let journal = ToolJournal {
        schema: 1,
        old,
        new: record.clone(),
    };
    journal.save(tool, directory)?;
    if journal.old.is_some() {
        std::fs::rename(&target, &previous)
            .map_err(|_| "stop the optional tool service before updating")?;
    }
    if std::fs::rename(staged, &target).is_err() {
        recover_publication(tool, directory)?;
        return Err("cannot publish official tool; previous state recovered".to_owned());
    }
    crate::installation::sync_directory(directory)?;
    write_record(tool, directory, record)?;
    journal.finish(tool, directory)
}
fn write_record(tool: Tool, directory: &Path, record: &Installed) -> Result<(), String> {
    let mut file =
        tempfile::NamedTempFile::new_in(directory).map_err(|_| "cannot stage tool record")?;
    file.write_all(&serde_json::to_vec(record).map_err(|_| "cannot encode tool record")?)
        .and_then(|()| file.as_file().sync_all())
        .map_err(|_| "cannot write tool record")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o644))
            .map_err(|_| "cannot set tool record permissions")?;
    }
    file.persist(record_path(tool, directory))
        .map_err(|_| "cannot publish tool record")?;
    crate::installation::sync_directory(directory)
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolJournal {
    schema: u8,
    old: Option<Installed>,
    new: Installed,
}
fn tool_journal(tool: Tool, directory: &Path) -> PathBuf {
    directory.join(format!(".{}-update.json", tool.name()))
}
fn tool_previous(tool: Tool, directory: &Path) -> PathBuf {
    directory.join(format!(".{}-previous", tool.name()))
}
impl ToolJournal {
    fn save(&self, tool: Tool, directory: &Path) -> Result<(), String> {
        crate::installation::publish_private(
            &tool_journal(tool, directory),
            &serde_json::to_vec(self).map_err(|_| "cannot encode tool journal")?,
        )
    }
    fn finish(&self, tool: Tool, directory: &Path) -> Result<(), String> {
        let previous = tool_previous(tool, directory);
        if std::fs::symlink_metadata(&previous).is_ok() {
            let old = self.old.as_ref().ok_or("unexpected previous tool file")?;
            if crate::release::hash(&previous)? != old.sha256 {
                return Err("previous tool changed; recovery files preserved".to_owned());
            }
            std::fs::remove_file(previous).map_err(|_| "cannot remove owned previous tool")?;
        }
        std::fs::remove_file(tool_journal(tool, directory))
            .map_err(|_| "cannot finalize tool journal")?;
        crate::installation::sync_directory(directory)
    }
}
fn valid_record(record: &Installed, tool: Tool) -> bool {
    record.schema == 1
        && record.tool == tool
        && valid_version(&record.version)
        && record.sha256.len() == 64
        && record.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
}
fn recover_publication(tool: Tool, directory: &Path) -> Result<(), String> {
    let path = tool_journal(tool, directory);
    if std::fs::symlink_metadata(&path).is_err() {
        return Ok(());
    }
    let journal: ToolJournal =
        serde_json::from_slice(&crate::installation::read_private(&path, 8192)?)
            .map_err(|_| "invalid tool recovery journal")?;
    if journal.schema != 1
        || !valid_record(&journal.new, tool)
        || journal
            .old
            .as_ref()
            .is_some_and(|old| !valid_record(old, tool))
    {
        return Err("invalid tool recovery record".to_owned());
    }
    let target = directory.join(tool.filename());
    let selected = if std::fs::symlink_metadata(&target).is_ok() {
        let hash = crate::release::hash(&target)?;
        if hash == journal.new.sha256 {
            &journal.new
        } else if let Some(old) = journal.old.as_ref().filter(|old| old.sha256 == hash) {
            old
        } else {
            return Err("tool changed outside installation; recovery files preserved".to_owned());
        }
    } else if let Some(old) = &journal.old {
        let previous = tool_previous(tool, directory);
        if crate::release::hash(&previous)? != old.sha256 {
            return Err("previous tool checksum mismatch".to_owned());
        }
        std::fs::rename(previous, &target).map_err(|_| "cannot recover previous tool")?;
        crate::installation::sync_directory(directory)?;
        old
    } else {
        if record_path(tool, directory).exists() || tool_previous(tool, directory).exists() {
            return Err("unexpected tool recovery state".to_owned());
        }
        return journal.finish(tool, directory);
    };
    write_record(tool, directory, selected)?;
    journal.finish(tool, directory)
}

fn extract_rclone(package: &Path, directory: &Path, version: &str) -> Result<PathBuf, String> {
    let mut archive = zip::ZipArchive::new(crate::release::bounded_zip(package, 32)?)
        .map_err(|_| "invalid rclone ZIP")?;
    if archive.len() > 32 {
        return Err("rclone package entry limit exceeded".to_owned());
    }
    let asset = asset_name(Tool::Rclone, version)?;
    let expected = format!(
        "{}/{}",
        asset.trim_end_matches(".zip"),
        Tool::Rclone.filename()
    );
    let output = directory.join(Tool::Rclone.filename());
    let mut found = false;
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|_| "invalid rclone package entry")?;
        if entry
            .name()
            .map_err(|_| "invalid rclone package path")?
            .as_ref()
            != expected
        {
            continue;
        }
        if found
            || !entry.is_file()
            || entry.is_symlink()
            || entry.size() == 0
            || entry.size() > MAX_TOOL
        {
            return Err("invalid rclone executable entry".to_owned());
        }
        found = true;
        archive::write_new(&output, &[])?;
        let mut target = std::fs::OpenOptions::new()
            .write(true)
            .open(&output)
            .map_err(|_| "cannot stage rclone")?;
        let expected = entry.size();
        let copied = std::io::copy(&mut entry.take(MAX_TOOL + 1), &mut target)
            .map_err(|_| "cannot extract rclone")?;
        if copied != expected {
            return Err("rclone executable size mismatch".to_owned());
        }
        target.sync_all().map_err(|_| "cannot sync rclone")?;
    }
    if !found {
        return Err("rclone executable absent".to_owned());
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        process_runner::{Output, Request, Runner},
        release::Download,
    };
    use sha2::Digest as _;
    use std::cell::Cell;
    struct Fake {
        bad_hash: bool,
        runs: Cell<usize>,
    }
    impl Download for Fake {
        fn fetch(&self, url: &str, output: &mut dyn Write, _limit: u64) -> Result<(), String> {
            let binary = b"fictional executable";
            if url.ends_with("/latest") {
                let digest = if self.bad_hash {
                    "0".repeat(64)
                } else {
                    format!("{:x}", sha2::Sha256::digest(binary))
                };
                let document = serde_json::json!({"tag_name":"2026.11.0","assets":[{"name":asset_name(Tool::Cloudflared,"2026.11.0")?,"digest":format!("sha256:{digest}"),"size":binary.len()}]});
                output
                    .write_all(&serde_json::to_vec(&document).unwrap())
                    .unwrap();
            } else {
                output.write_all(binary).unwrap();
            }
            Ok(())
        }
    }
    impl Runner for Fake {
        fn run(&self, _request: Request) -> Result<Output, String> {
            self.runs.set(self.runs.get() + 1);
            Ok(Output {
                success: true,
                stdout: b"cloudflared version 2026.11.0".to_vec(),
            })
        }
    }
    #[test]
    fn verified_installation_is_recorded_and_tampering_is_refused() {
        let directory = tempfile::tempdir().unwrap();
        let source = Fake {
            bad_hash: false,
            runs: Cell::new(0),
        };
        install_at(Tool::Cloudflared, directory.path(), &source, &source).unwrap();
        let binary = binary_at(Tool::Cloudflared, directory.path()).unwrap();
        std::fs::write(&binary, b"tampered").unwrap();
        assert!(binary_at(Tool::Cloudflared, directory.path()).is_err());
        assert_eq!(source.runs.get(), 1);
    }
    #[test]
    fn checksum_failure_never_executes_or_replaces_a_tool() {
        let directory = tempfile::tempdir().unwrap();
        let source = Fake {
            bad_hash: true,
            runs: Cell::new(0),
        };
        assert!(install_at(Tool::Cloudflared, directory.path(), &source, &source).is_err());
        assert_eq!(source.runs.get(), 0);
        assert!(!directory.path().join(Tool::Cloudflared.filename()).exists());
    }
    #[test]
    fn concurrent_readers_prevent_replacement_until_both_release_the_tool() {
        let directory = tempfile::tempdir().unwrap();
        let source = Fake {
            bad_hash: false,
            runs: Cell::new(0),
        };
        install_at(Tool::Cloudflared, directory.path(), &source, &source).unwrap();
        let first = lease_at(Tool::Cloudflared, directory.path()).unwrap();
        let second = lease_at(Tool::Cloudflared, directory.path()).unwrap();
        assert!(install_at(Tool::Cloudflared, directory.path(), &source, &source).is_err());
        drop(first);
        assert!(install_at(Tool::Cloudflared, directory.path(), &source, &source).is_err());
        drop(second);
        install_at(Tool::Cloudflared, directory.path(), &source, &source).unwrap();
    }
    struct MutatingRunner;
    impl Runner for MutatingRunner {
        fn run(&self, request: Request) -> Result<Output, String> {
            std::fs::write(request.program, b"changed during version check").unwrap();
            Ok(Output {
                success: true,
                stdout: b"cloudflared version 2026.11.0".to_vec(),
            })
        }
    }
    #[test]
    fn version_probe_cannot_publish_a_changed_executable() {
        let directory = tempfile::tempdir().unwrap();
        let source = Fake {
            bad_hash: false,
            runs: Cell::new(0),
        };
        assert!(
            install_at(
                Tool::Cloudflared,
                directory.path(),
                &source,
                &MutatingRunner
            )
            .is_err()
        );
        assert!(!directory.path().join(Tool::Cloudflared.filename()).exists());
    }
    #[test]
    fn interrupted_tool_publication_recovers_old_or_finishes_new_without_network() {
        for new_published in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let tool = Tool::Cloudflared;
            let source = Fake {
                bad_hash: false,
                runs: Cell::new(0),
            };
            install_at(tool, directory.path(), &source, &source).unwrap();
            let target = directory.path().join(tool.filename());
            let old = read_record(tool, directory.path()).unwrap();
            let new = Installed {
                schema: 1,
                tool,
                version: "2026.12.0".to_owned(),
                sha256: format!("{:x}", sha2::Sha256::digest(b"new executable")),
            };
            let journal = ToolJournal {
                schema: 1,
                old: Some(old),
                new,
            };
            journal.save(tool, directory.path()).unwrap();
            std::fs::rename(&target, tool_previous(tool, directory.path())).unwrap();
            if new_published {
                std::fs::write(&target, b"new executable").unwrap();
            }
            recover_publication(tool, directory.path()).unwrap();
            assert!(binary_at(tool, directory.path()).is_ok());
            assert_eq!(
                read_record(tool, directory.path()).unwrap().version,
                if new_published {
                    "2026.12.0"
                } else {
                    "2026.11.0"
                }
            );
            assert!(!tool_journal(tool, directory.path()).exists());
        }
    }
    #[test]
    fn version_validation_rejects_a_different_version_with_the_same_prefix() {
        struct Probe(&'static str);
        impl Runner for Probe {
            fn run(&self, _: Request) -> Result<Output, String> {
                Ok(Output {
                    success: true,
                    stdout: self.0.as_bytes().to_vec(),
                })
            }
        }
        for (output, accepted) in [
            ("cloudflared version 2026.11.0", true),
            ("cloudflared version 2026.11.0 (built for fixture)", true),
            ("cloudflared version 2026.11.01", false),
        ] {
            assert_eq!(
                check_version(
                    &Probe(output),
                    Path::new("fictional"),
                    Tool::Cloudflared,
                    "2026.11.0"
                )
                .is_ok(),
                accepted
            );
        }
    }
}
