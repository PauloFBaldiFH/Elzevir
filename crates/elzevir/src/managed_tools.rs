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
#[derive(Serialize, Deserialize)]
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
    let path = PathBuf::from("/usr/local/lib/elzevir/tools");
    #[cfg(windows)]
    let path = PathBuf::from(
        std::env::var_os("ProgramW6432")
            .or_else(|| std::env::var_os("ProgramFiles"))
            .ok_or("program directory unavailable")?,
    )
    .join("Elzevir")
    .join("tools");
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
pub(crate) fn installed(tool: Tool) -> Result<Installed, String> {
    let directory = root(false)?;
    binary_at(tool, &directory)?;
    read_record(tool, &directory)
}
pub(crate) fn install(tool: Tool) -> Result<Installed, String> {
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
        .map_err(|_| "another tool installation is running")?;
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
    check_version(runner, &staged, tool, &release.tag_name)?;
    let record = Installed {
        schema: 1,
        tool,
        version: release.tag_name,
        sha256: crate::release::hash(&staged)?,
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
    if !checked.success || !first.starts_with(&expected_version) {
        return Err("official tool version mismatch".to_owned());
    }
    Ok(())
}

fn publish(tool: Tool, directory: &Path, staged: &Path, record: &Installed) -> Result<(), String> {
    let target = directory.join(tool.filename());
    let previous = staged.with_file_name("previous-tool");
    let existed = std::fs::symlink_metadata(&target).is_ok();
    if existed {
        archive::regular(&target)?;
        let old = read_record(tool, directory)?;
        let current = crate::release::hash(&target)?;
        if current != old.sha256 && current != record.sha256 {
            return Err("existing program is not an owned official tool".to_owned());
        }
        std::fs::rename(&target, &previous)
            .map_err(|_| "stop the optional tool service before updating")?;
    }
    let result = (|| {
        std::fs::rename(staged, &target).map_err(|_| "cannot publish official tool")?;
        let mut record_file =
            tempfile::NamedTempFile::new_in(directory).map_err(|_| "cannot stage tool record")?;
        record_file
            .write_all(&serde_json::to_vec(record).map_err(|_| "cannot encode tool record")?)
            .and_then(|()| record_file.as_file().sync_all())
            .map_err(|_| "cannot write tool record")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            record_file
                .as_file()
                .set_permissions(std::fs::Permissions::from_mode(0o644))
                .map_err(|_| "cannot set tool record permissions")?;
        }
        record_file
            .persist(record_path(tool, directory))
            .map_err(|_| "cannot publish tool record")?;
        Ok(())
    })();
    if result.is_err() {
        if target.exists() {
            std::fs::remove_file(&target).map_err(|_| "cannot clear failed tool installation")?;
        }
        if existed {
            std::fs::rename(previous, target)
                .map_err(|_| "cannot restore previous optional tool")?;
        }
    }
    result
}

fn extract_rclone(package: &Path, directory: &Path, version: &str) -> Result<PathBuf, String> {
    let mut archive = zip::ZipArchive::new(
        std::fs::File::open(package).map_err(|_| "rclone package unavailable")?,
    )
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
}
