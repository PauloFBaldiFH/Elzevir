// SPDX-License-Identifier: GPL-3.0-or-later
//! Pinned official releases, streaming size limits and exact SHA-256 verification.
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};
const REPOSITORY: &str = "https://github.com/PauloFBaldiFH/Elzevir";
const MAX_PACKAGE: u64 = 512 * 1024 * 1024;

pub(crate) trait Download {
    fn fetch(&self, url: &str, output: &mut dyn Write, limit: u64) -> Result<(), String>;
}
pub(crate) struct Http;
pub(crate) struct Metadata;

impl Download for Http {
    fn fetch(&self, url: &str, output: &mut dyn Write, limit: u64) -> Result<(), String> {
        fetch(url, output, limit, Duration::from_mins(15))
    }
}

impl Download for Metadata {
    fn fetch(&self, url: &str, output: &mut dyn Write, limit: u64) -> Result<(), String> {
        fetch(url, output, limit, Duration::from_secs(4))
    }
}

fn fetch(url: &str, output: &mut dyn Write, limit: u64, timeout: Duration) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err("downloads require HTTPS".to_owned());
    }
    let agent = crate::http_client::secure_agent(5, timeout)?;
    let mut response = agent
        .get(url)
        .header("User-Agent", "Elzevir-operations")
        .call()
        .map_err(|_| "official download unavailable")?;
    if !response.status().is_success() {
        return Err("official download refused".to_owned());
    }
    let bytes = std::io::copy(&mut response.body_mut().as_reader().take(limit + 1), output)
        .map_err(|_| "official download failed")?;
    if bytes > limit {
        return Err("official download exceeds limit".to_owned());
    }
    Ok(())
}
#[derive(Debug, Deserialize)]
pub(crate) struct Latest {
    pub tag_name: String,
    pub body: Option<String>,
}

pub(crate) fn valid_version(version: &str) -> bool {
    let Some(value) = version.strip_prefix('v').filter(|_| version.len() <= 40) else {
        return false;
    };
    let (core, prerelease) = value
        .split_once('-')
        .map_or((value, None), |(core, prerelease)| (core, Some(prerelease)));
    let parts = core.split('.').collect::<Vec<_>>();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| part.len() <= 10 && numeric_identifier(part))
        && prerelease.is_none_or(|suffix| suffix.split('.').all(prerelease_identifier))
}

fn numeric_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && (!value.starts_with('0') || value == "0")
}

fn prerelease_identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        && (!value.bytes().all(|byte| byte.is_ascii_digit()) || numeric_identifier(value))
}
pub(crate) fn latest(source: &impl Download) -> Result<Latest, String> {
    let mut bytes = Vec::new();
    source.fetch(
        "https://api.github.com/repos/PauloFBaldiFH/Elzevir/releases/latest",
        &mut bytes,
        65_536,
    )?;
    let mut release: Latest =
        serde_json::from_slice(&bytes).map_err(|_| "invalid release document")?;
    if !valid_version(&release.tag_name) {
        return Err("unsupported release version".to_owned());
    }
    release.body = release.body.map(|body| crate::panel_text(&body, 16_384));
    Ok(release)
}
pub(crate) fn target() -> Result<&'static str, String> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-musl"),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-musl"),
        ("windows", "x86_64") => Ok("x86_64-pc-windows-msvc"),
        ("windows", "aarch64") => Ok("aarch64-pc-windows-msvc"),
        _ => Err("no official package for this platform".to_owned()),
    }
}
pub(crate) fn checksum(source: &str, name: &str) -> Result<String, String> {
    let mut found = None;
    for line in source.lines() {
        let mut fields = line.split_whitespace();
        let Some(hash) = fields.next() else {
            continue;
        };
        if fields
            .next()
            .is_some_and(|entry| entry.trim_start_matches('*') == name)
        {
            if found.is_some()
                || fields.next().is_some()
                || hash.len() != 64
                || !hash.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return Err("invalid or duplicate release checksum".to_owned());
            }
            found = Some(hash.to_ascii_lowercase());
        }
    }
    found.ok_or("official package checksum absent".to_owned())
}
pub(crate) fn hash(path: &Path) -> Result<String, String> {
    let mut reader = std::fs::File::open(path).map_err(|_| "package unavailable")?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 16_384];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|_| "cannot hash package")?;
        if read == 0 {
            break;
        }
        digest.update(buffer.get(..read).ok_or("invalid package read")?);
    }
    Ok(format!("{:x}", digest.finalize()))
}
pub(crate) fn stage(
    source: &impl Download,
    version: &str,
    directory: &Path,
) -> Result<PathBuf, String> {
    if !valid_version(version) {
        return Err("invalid release version".to_owned());
    }
    let target = target()?;
    let extension = if cfg!(windows) { "zip" } else { "tar.gz" };
    let name = format!("elzevir-{version}-{target}.{extension}");
    let base = format!("{REPOSITORY}/releases/download/{version}");
    let mut sums = Vec::new();
    source.fetch(&format!("{base}/SHA256SUMS"), &mut sums, 65_536)?;
    let expected = checksum(
        std::str::from_utf8(&sums).map_err(|_| "invalid release checksums")?,
        &name,
    )?;
    let package = directory.join(&name);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&package)
        .map_err(|_| "package destination must be new")?;
    source.fetch(&format!("{base}/{name}"), &mut file, MAX_PACKAGE)?;
    file.sync_all().map_err(|_| "cannot sync package")?;
    drop(file);
    if hash(&package)? != expected {
        return Err("release checksum mismatch".to_owned());
    }
    unpack(&package, directory, cfg!(windows))
}

pub(crate) fn unpack(package: &Path, directory: &Path, windows: bool) -> Result<PathBuf, String> {
    if crate::backup_archive::regular(package)? > MAX_PACKAGE {
        return Err("package exceeds limit".to_owned());
    }
    let binary = if windows { "elzevir.exe" } else { "elzevir" };
    let output = directory.join(binary);
    if windows {
        unzip(package, &output, binary)?;
    } else {
        untar(package, &output, binary)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&output, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| "cannot prepare release executable")?;
    }
    Ok(output)
}
fn copy_binary(source: &mut impl Read, output: &Path, size: u64) -> Result<(), String> {
    if size == 0 || size > MAX_PACKAGE {
        return Err("invalid release executable size".to_owned());
    }
    crate::backup_archive::write_new(output, &[])?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .open(output)
        .map_err(|_| "cannot open staged executable")?;
    let copied = std::io::copy(&mut source.take(size + 1), &mut file)
        .map_err(|_| "cannot extract release executable")?;
    if copied != size {
        return Err("release executable size mismatch".to_owned());
    }
    file.sync_all()
        .map_err(|_| "cannot sync staged executable".to_owned())
}
fn untar(package: &Path, output: &Path, binary: &str) -> Result<(), String> {
    let input = std::fs::File::open(package).map_err(|_| "package unavailable")?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(input));
    let mut count = 0;
    let mut found = false;
    let mut auxiliary = 0_u64;
    for entry in archive
        .entries()
        .map_err(|_| "invalid release archive")?
        .raw(true)
    {
        let mut entry = entry.map_err(|_| "invalid release archive")?;
        count += 1;
        let path = entry.path().map_err(|_| "invalid release path")?;
        let name = path.to_str().ok_or("invalid release path")?;
        if count > 3
            || !entry.header().entry_type().is_file()
            || !matches!(name, "elzevir" | "README.md" | "LICENSE")
        {
            return Err("unexpected release entry".to_owned());
        }
        if name == binary {
            if found {
                return Err("duplicate release executable".to_owned());
            }
            found = true;
            let size = entry.size();
            copy_binary(&mut entry, output, size)?;
        } else {
            auxiliary = auxiliary.saturating_add(entry.size());
            if auxiliary > 4 * 1024 * 1024 {
                return Err("release documentation exceeds limit".to_owned());
            }
        }
    }
    if !found {
        return Err("release executable absent".to_owned());
    }
    Ok(())
}
pub(crate) fn bounded_zip(package: &Path, limit: u16) -> Result<std::fs::File, String> {
    use std::io::{Seek, SeekFrom};
    // Official packages use single-disk ZIP without comments or ZIP64.
    let mut input = std::fs::File::open(package).map_err(|_| "package unavailable")?;
    input
        .seek(SeekFrom::End(-22))
        .map_err(|_| "invalid release ZIP")?;
    let mut end = [0_u8; 22];
    input
        .read_exact(&mut end)
        .map_err(|_| "invalid release ZIP")?;
    if end.get(..4) != Some(b"PK\x05\x06")
        || end
            .get(10..12)
            .and_then(|bytes| <[u8; 2]>::try_from(bytes).ok())
            .map(u16::from_le_bytes)
            .is_none_or(|count| count > limit)
        || end.get(4..8) != Some(&[0, 0, 0, 0])
        || end.get(8..10) != end.get(10..12)
        || end.get(20..22) != Some(&[0, 0])
    {
        return Err("unsupported release ZIP directory".to_owned());
    }
    input.rewind().map_err(|_| "cannot rewind release ZIP")?;
    Ok(input)
}
fn unzip(package: &Path, output: &Path, binary: &str) -> Result<(), String> {
    let input = bounded_zip(package, 3)?;
    let mut archive = zip::ZipArchive::new(input).map_err(|_| "invalid release ZIP")?;
    if archive.len() > 3 {
        return Err("release ZIP exceeds entry limit".to_owned());
    }
    let mut found = false;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|_| "invalid release ZIP entry")?;
        if !entry.is_file()
            || entry.is_symlink()
            || !matches!(
                entry.name().map_err(|_| "invalid ZIP filename")?.as_ref(),
                "elzevir.exe" | "README.md" | "LICENSE"
            )
        {
            return Err("unexpected release ZIP entry".to_owned());
        }
        if entry.name().map_err(|_| "invalid ZIP filename")?.as_ref() == binary {
            if found {
                return Err("duplicate release executable".to_owned());
            }
            found = true;
            let size = entry.size();
            copy_binary(&mut entry, output, size)?;
        } else if entry.size() > 4 * 1024 * 1024 {
            return Err("release documentation exceeds limit".to_owned());
        }
    }
    if !found {
        return Err("release executable absent".to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn release_names_and_checksums_are_exact_and_unambiguous() {
        assert!(valid_version("v1.2.3"));
        assert!(valid_version("v0.1.0-alpha.1"));
        for value in [
            "v1.2",
            "v01.2.3",
            "../v1.2.3",
            "v1.2.3;command",
            "v1.2.3-",
            "v1.2.3-alpha..1",
            "v1.2.3-alpha.01",
        ] {
            assert!(!valid_version(value));
        }
        let hash = "a".repeat(64);
        let sum = format!("{hash}  expected.tar.gz\n");
        assert_eq!(checksum(&sum, "expected.tar.gz").unwrap(), hash);
        assert!(checksum(&(sum.clone() + &sum), "expected.tar.gz").is_err());
        assert!(checksum(&sum, "other.tar.gz").is_err());
    }
    #[test]
    fn release_tar_never_follows_symlinks_or_overwrites_an_existing_executable() {
        let directory = tempfile::tempdir().unwrap();
        let package = directory.path().join("package.tar.gz");
        let output = directory.path().join("elzevir");
        {
            let gzip = flate2::write::GzEncoder::new(
                std::fs::File::create(&package).unwrap(),
                flate2::Compression::default(),
            );
            let mut tar = tar::Builder::new(gzip);
            let mut header = tar::Header::new_gnu();
            header.set_size(0);
            header.set_mode(0o777);
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_link_name("/etc/passwd").unwrap();
            header.set_cksum();
            tar.append_data(&mut header, "elzevir", std::io::empty())
                .unwrap();
            tar.into_inner().unwrap().finish().unwrap();
        }
        assert!(unpack(&package, directory.path(), false).is_err());
        assert!(!output.exists());
        crate::backup_archive::write_new(&output, b"old").unwrap();
        assert!(unpack(&package, directory.path(), false).is_err());
        assert_eq!(std::fs::read(output).unwrap(), b"old");
    }
    #[test]
    fn valid_windows_release_extracts_and_never_overwrites_an_existing_binary() {
        let root = tempfile::tempdir().unwrap();
        let package = root.path().join("release.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&package).unwrap());
        zip.start_file("elzevir.exe", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"fictional windows executable").unwrap();
        zip.start_file("LICENSE", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"fictional license").unwrap();
        zip.finish().unwrap();
        let output = unpack(&package, root.path(), true).unwrap();
        assert_eq!(
            std::fs::read(&output).unwrap(),
            b"fictional windows executable"
        );
        assert!(unpack(&package, root.path(), true).is_err());
        assert_eq!(
            std::fs::read(&output).unwrap(),
            b"fictional windows executable"
        );
        assert!(bounded_zip(&package, 1).is_err());
    }
    #[test]
    fn official_windows_zip_extracts_only_the_executable_and_preserves_existing_files() {
        let root = tempfile::tempdir().unwrap();
        let package = root.path().join("release.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&package).unwrap());
        for (name, contents) in [
            ("elzevir.exe", "fictional executable"),
            ("LICENSE", "license"),
            ("README.md", "readme"),
        ] {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(contents.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        let target = root.path().join("out");
        std::fs::create_dir(&target).unwrap();
        let executable = unpack(&package, &target, true).unwrap();
        assert_eq!(std::fs::read(&executable).unwrap(), b"fictional executable");
        assert!(!target.join("README.md").exists());
        assert!(unpack(&package, &target, true).is_err());
        assert_eq!(std::fs::read(&executable).unwrap(), b"fictional executable");
    }
}
