// SPDX-License-Identifier: GPL-3.0-or-later
//! Streaming authenticated backups; no live database, key or index is copied blindly.

use crate::cancellation::{Control, Reader as Controlled, check};
use age::secrecy::ExposeSecret as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    io::{Read, Write},
    path::Path,
};

const MAX_AUX: u64 = 16 * 1024 * 1024;
const MAX_DB: u64 = 512 * 1024 * 1024 * 1024;

fn private_scratch(directory: &Path) -> Result<tempfile::TempDir, String> {
    #[cfg(target_os = "linux")]
    if !rustix::process::geteuid().is_root() {
        return tempfile::tempdir_in(directory).map_err(|_| "cannot prepare backup".to_owned());
    }
    #[cfg(windows)]
    if crate::service::require_operator().is_err() {
        return tempfile::tempdir_in(directory)
            .map_err(|_| "cannot prepare private backup scratch".to_owned());
    }
    let _ = directory;
    tempfile::tempdir().map_err(|_| "cannot prepare private backup scratch".to_owned())
}

fn scoped_file(scope: &cap_std::fs::Dir, relative: &Path) -> Result<cap_std::fs::File, String> {
    use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt as _};
    let mut options = cap_std::fs::OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let file = scope
        .open_with(relative, &options)
        .map_err(|_| "backup input must be a regular file inside the institution")?;
    if !file
        .metadata()
        .map_err(|_| "backup input unavailable")?
        .is_file()
    {
        return Err("backup input must be a regular file".to_owned());
    }
    Ok(file)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Entry {
    pub name: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Manifest {
    pub schema: u8,
    pub id: String,
    pub created_at: String,
    pub version: String,
    pub database_schema: i64,
    pub entries: Vec<Entry>,
}

pub(crate) fn identifier() -> Result<String, String> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| "cannot generate operation identity")?;
    Ok(format!("{:032x}", u128::from_be_bytes(bytes)))
}

pub(crate) fn regular(path: &Path) -> Result<u64, String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| "file unavailable")?;
    if !metadata.is_file() || metadata.is_symlink() {
        return Err("regular file required".to_owned());
    }
    Ok(metadata.len())
}

pub(crate) fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|_| "destination must be new and writable")?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|_| "cannot publish file".to_owned())
}

pub(crate) fn key(path: &Path, create: bool) -> Result<age::x25519::Identity, String> {
    if !path.try_exists().map_err(|_| "cannot inspect backup key")? {
        if !create {
            return Err("backup recovery key is required".to_owned());
        }
        let identity = age::x25519::Identity::generate();
        let text = identity.to_string();
        let parent = path.parent().ok_or("backup key needs a parent")?;
        let mut file =
            tempfile::NamedTempFile::new_in(parent).map_err(|_| "cannot prepare backup key")?;
        file.write_all(text.expose_secret().as_bytes())
            .and_then(|()| file.as_file().sync_all())
            .map_err(|_| "cannot write backup key")?;
        crate::file_owner::private_file(file.as_file(), parent)?;
        // A competing creator may win; always reload the published identity.
        if file.persist_noclobber(path).is_err() && !path.exists() {
            return Err("cannot publish backup key".to_owned());
        }
    }
    if regular(path)? > 256 {
        return Err("invalid backup key".to_owned());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if std::fs::metadata(path)
            .map_err(|_| "backup key unavailable")?
            .permissions()
            .mode()
            & 0o077
            != 0
        {
            return Err("backup key must be private to its owner".to_owned());
        }
    }
    let mut text = zeroize::Zeroizing::new(String::new());
    std::fs::File::open(path)
        .and_then(|file| file.take(257).read_to_string(&mut text))
        .map_err(|_| "backup key unavailable")?;
    text.trim()
        .parse()
        .map_err(|_| "invalid backup key".to_owned())
}

fn hash(path: &Path, control: &Control) -> Result<String, String> {
    let mut source = std::fs::File::open(path).map_err(|_| "snapshot file unavailable")?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 65_536].into_boxed_slice();
    loop {
        check(control)?;
        let read = source
            .read(&mut buffer)
            .map_err(|_| "cannot hash snapshot")?;
        if read == 0 {
            break;
        }
        digest.update(buffer.get(..read).ok_or("invalid read size")?);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn allowed(name: &str) -> bool {
    matches!(
        name,
        "database.sqlite"
            | "server.toml"
            | "library.toml"
            | "operations.toml"
            | "author-table.txt"
            | "cloud.json"
            | "cloud-plans.json"
            | "cloud-artifacts.json"
    ) || name.strip_prefix("secrets/").is_some_and(|leaf| {
        !leaf.eq_ignore_ascii_case("cf-cert.pem")
            && !leaf.is_empty()
            && leaf.len() <= 64
            && leaf
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphanumeric)
            && !leaf.ends_with('.')
            && !matches!(
                leaf.split('.')
                    .next()
                    .unwrap_or_default()
                    .to_ascii_uppercase()
                    .as_str(),
                "CON"
                    | "PRN"
                    | "AUX"
                    | "NUL"
                    | "COM1"
                    | "COM2"
                    | "COM3"
                    | "COM4"
                    | "COM5"
                    | "COM6"
                    | "COM7"
                    | "COM8"
                    | "COM9"
                    | "LPT1"
                    | "LPT2"
                    | "LPT3"
                    | "LPT4"
                    | "LPT5"
                    | "LPT6"
                    | "LPT7"
                    | "LPT8"
                    | "LPT9"
            )
            && leaf
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    })
}

/// Writes an encrypted tar/gzip snapshot and publishes it without replacement.
pub(crate) fn create(
    directory: &Path,
    config: &str,
    output: &Path,
    readers: Option<&elzevir_store::ReadPool>,
) -> Result<Manifest, String> {
    let control: Control = std::sync::Arc::new(|| false);
    create_controlled(directory, config, output, readers, &control)
}

pub(crate) fn create_controlled(
    directory: &Path,
    config: &str,
    output: &Path,
    readers: Option<&elzevir_store::ReadPool>,
    control: &Control,
) -> Result<Manifest, String> {
    check(control)?;
    let directory = directory
        .canonicalize()
        .map_err(|_| "institution unavailable")?;
    let directory = directory.as_path();
    let scope = cap_std::fs::Dir::open_ambient_dir(directory, cap_std::ambient_authority())
        .map_err(|_| "institution unavailable")?;
    regular(&directory.join("elzevir.db"))?;
    if std::fs::symlink_metadata(output).is_ok() {
        return Err("backup destination already exists".to_owned());
    }
    if config.len() > 65_536 {
        return Err("configuration exceeds backup limit".to_owned());
    }
    let identity = key(&directory.join("backup.key"), true)?;
    let scratch = private_scratch(directory)?;
    let image = scratch.path().join("database.sqlite");
    let database = match readers {
        Some(pool) => pool.snapshot_controlled(&image, std::sync::Arc::clone(control)),
        None => elzevir_store::snapshot_offline(&directory.join("elzevir.db"), &image),
    }
    .map_err(|_| "database snapshot failed")?;
    if database.bytes > MAX_DB {
        return Err("database exceeds backup limit".to_owned());
    }
    write_new(&scratch.path().join("server.toml"), config.as_bytes())?;
    let files = vec![
        ("database.sqlite".to_owned(), image),
        ("server.toml".to_owned(), scratch.path().join("server.toml")),
    ];
    let mut files = additional_files(directory, &scope, files)?;
    let mut manifest = Manifest {
        schema: 1,
        id: output_id(output)?,
        created_at: jiff::Timestamp::now().to_string(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        database_schema: database.schema_version,
        entries: Vec::new(),
    };
    stage_inputs(
        directory,
        &scope,
        scratch.path(),
        &mut files,
        &mut manifest,
        control,
    )?;
    let mut file = tempfile::NamedTempFile::new_in(output.parent().ok_or("backup needs parent")?)
        .map_err(|_| "cannot prepare backup output")?;
    encrypt(
        file.as_file_mut(),
        &manifest,
        &files,
        &identity.to_public(),
        control,
    )?;
    file.as_file()
        .sync_all()
        .map_err(|_| "cannot sync backup")?;
    check(control)?;
    file.persist_noclobber(output)
        .map_err(|_| "cannot publish backup without replacement")?;
    Ok(manifest)
}

fn encrypt(
    file: &mut std::fs::File,
    manifest: &Manifest,
    files: &[(String, std::path::PathBuf)],
    recipient: &age::x25519::Recipient,
    control: &Control,
) -> Result<(), String> {
    let encryptor =
        age::Encryptor::with_recipients(std::iter::once(recipient as &dyn age::Recipient))
            .map_err(|_| "cannot encrypt backup")?;
    let writer = encryptor
        .wrap_output(file)
        .map_err(|_| "cannot encrypt backup")?;
    let gzip = flate2::write::GzEncoder::new(writer, flate2::Compression::default());
    let mut archive = tar::Builder::new(gzip);
    let bytes = serde_json::to_vec(manifest).map_err(|_| "cannot encode backup manifest")?;
    let mut header = tar::Header::new_gnu();
    header.set_size(u64::try_from(bytes.len()).map_err(|_| "manifest too large")?);
    header.set_mode(0o600);
    header.set_cksum();
    archive
        .append_data(&mut header, "manifest.json", bytes.as_slice())
        .map_err(|_| "cannot write backup manifest")?;
    for ((name, path), entry) in files.iter().zip(&manifest.entries) {
        check(control)?;
        let mut header = tar::Header::new_gnu();
        header.set_size(entry.bytes);
        header.set_mode(0o600);
        header.set_entry_type(tar::EntryType::Regular);
        header.set_cksum();
        let source = std::fs::File::open(path).map_err(|_| "backup entry unavailable")?;
        archive
            .append_data(
                &mut header,
                name,
                Controlled {
                    source,
                    control: std::sync::Arc::clone(control),
                },
            )
            .map_err(|_| "cannot write backup entry")?;
    }
    archive
        .into_inner()
        .and_then(flate2::write::GzEncoder::finish)
        .and_then(age::stream::StreamWriter::finish)
        .map_err(|_| "cannot finish encrypted backup")?;
    Ok(())
}

fn stage_inputs(
    directory: &Path,
    scope: &cap_std::fs::Dir,
    scratch: &Path,
    files: &mut [(String, std::path::PathBuf)],
    manifest: &mut Manifest,
    control: &Control,
) -> Result<(), String> {
    let mut auxiliary = 0_u64;
    for (name, path) in files {
        let bytes = regular(path)?;
        if name != "database.sqlite" && path.parent() != Some(scratch) {
            let staged = scratch.join(name.replace('/', "-"));
            let mut target = tempfile::NamedTempFile::new_in(scratch)
                .map_err(|_| "cannot stage backup input")?;
            let relative = path
                .strip_prefix(directory)
                .map_err(|_| "backup input must be inside the institution")?;
            let mut source = scoped_file(scope, relative)?.take(MAX_AUX + 1);
            let copied = std::io::copy(&mut source, target.as_file_mut())
                .map_err(|_| "cannot stage backup input")?;
            if copied != bytes || copied > MAX_AUX {
                return Err("backup input changed or exceeds limit".to_owned());
            }
            target
                .persist_noclobber(&staged)
                .map_err(|_| "cannot stage backup input")?;
            *path = staged;
        }
        if name != "database.sqlite" {
            auxiliary = auxiliary.saturating_add(bytes);
        }
        if auxiliary > MAX_AUX {
            return Err("auxiliary backup input exceeds limit".to_owned());
        }
        manifest.entries.push(Entry {
            name: name.clone(),
            bytes,
            sha256: hash(path, control)?,
        });
    }
    Ok(())
}

fn output_id(output: &Path) -> Result<String, String> {
    output
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_prefix("elzevir-"))
        .and_then(|name| name.strip_suffix(".age"))
        .filter(|id| id.len() == 32 && id.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .map_or_else(identifier, |id| Ok(id.to_owned()))
}

fn additional_files(
    directory: &Path,
    scope: &cap_std::fs::Dir,
    mut files: Vec<(String, std::path::PathBuf)>,
) -> Result<Vec<(String, std::path::PathBuf)>, String> {
    for (source, name) in [
        ("elzevir.toml", "library.toml"),
        ("operations.toml", "operations.toml"),
        ("cloud.json", "cloud.json"),
        ("cloud-plans.json", "cloud-plans.json"),
        ("cloud-artifacts.json", "cloud-artifacts.json"),
    ] {
        let path = directory.join(source);
        if path
            .try_exists()
            .map_err(|_| "cannot inspect backup input")?
        {
            files.push((name.to_owned(), path));
        }
    }
    if directory.join("elzevir.toml").exists() {
        let mut text = String::new();
        scoped_file(scope, Path::new("elzevir.toml"))?
            .take(MAX_AUX + 1)
            .read_to_string(&mut text)
            .map_err(|_| "library configuration unavailable")?;
        if u64::try_from(text.len()).map_err(|_| "configuration too large")? > MAX_AUX {
            return Err("library configuration too large".to_owned());
        }
        let config: toml::Value =
            toml::from_str(&text).map_err(|_| "invalid library configuration")?;
        if let Some(table) = config
            .get("call_number")
            .and_then(|value| value.get("author_table"))
            .and_then(toml::Value::as_str)
        {
            let path = directory.join(table);
            let relative = path
                .strip_prefix(directory)
                .map_err(|_| "author table must be inside the institution")?;
            scoped_file(scope, relative)?;
            files.push(("author-table.txt".to_owned(), path));
        }
    }
    let secrets = directory.join("secrets");
    if secrets.exists() {
        let metadata = std::fs::symlink_metadata(&secrets).map_err(|_| "secrets unavailable")?;
        if !metadata.is_dir() || metadata.is_symlink() {
            return Err("secrets must be a regular directory".to_owned());
        }
        for entry in std::fs::read_dir(secrets)
            .map_err(|_| "secrets unavailable")?
            .take(33)
        {
            let entry = entry.map_err(|_| "secrets unavailable")?;
            if entry.file_name().to_str().is_some_and(|name| {
                name.eq_ignore_ascii_case("cf-cert.pem")
                    || name.strip_prefix("rclone-").is_some_and(|tail| {
                        [".conf.old", ".pending.json"].iter().any(|suffix| {
                            tail.strip_suffix(suffix)
                                .is_some_and(crate::cloud::valid_id)
                        })
                    })
            }) {
                continue;
            }
            let name = format!(
                "secrets/{}",
                entry
                    .file_name()
                    .to_str()
                    .ok_or("invalid secret filename")?
            );
            if !allowed(&name) || files.len() >= 32 {
                return Err("secret file limit exceeded".to_owned());
            }
            files.push((name, entry.path()));
        }
    }
    Ok(files)
}

pub(crate) struct Extracted {
    pub directory: tempfile::TempDir,
    pub manifest: Manifest,
}

/// Extracts only declared regular entries, with byte limits and full authentication.
pub(crate) fn extract(input: &Path, key_file: &Path) -> Result<Extracted, String> {
    extract_controlled(input, key_file, std::sync::Arc::new(|| false), None)
}
pub(crate) fn extract_controlled(
    input: &Path,
    key_file: &Path,
    control: Control,
    scratch: Option<&Path>,
) -> Result<Extracted, String> {
    check(&control)?;
    if regular(input)? > MAX_DB {
        return Err("backup exceeds input limit".to_owned());
    }
    let identity = key(key_file, false)?;
    let source = Controlled {
        source: std::fs::File::open(input).map_err(|_| "backup unavailable")?,
        control: std::sync::Arc::clone(&control),
    };
    let header = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let source = HeaderLimit {
        source,
        remaining: 65_536,
        active: std::sync::Arc::clone(&header),
    };
    let decryptor = age::Decryptor::new(source).map_err(|_| "invalid encrypted backup")?;
    header.store(false, std::sync::atomic::Ordering::Relaxed);
    let reader = decryptor
        .decrypt(std::iter::once(&identity as &dyn age::Identity))
        .map_err(|_| "backup key or authentication refused")?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(reader));
    let mut entries = archive
        .entries()
        .map_err(|_| "invalid backup archive")?
        .raw(true);
    let first = entries
        .next()
        .ok_or("backup manifest absent")?
        .map_err(|_| "invalid backup manifest")?;
    let manifest = read_manifest(first)?;
    let (names, total) = validate_manifest(&manifest)?;
    let directory = match scratch {
        Some(directory) => private_scratch(directory)?,
        None => tempfile::tempdir().map_err(|_| "cannot prepare restore")?,
    };
    if fs4::available_space(directory.path()).map_err(|_| "cannot inspect restore space")?
        < total.saturating_add(16 * 1024 * 1024)
    {
        return Err("insufficient restore space".to_owned());
    }
    let mut found = BTreeSet::new();
    for entry in entries {
        let mut entry = entry.map_err(|_| "invalid backup entry")?;
        let name = entry
            .path()
            .map_err(|_| "invalid backup path")?
            .to_str()
            .ok_or("invalid backup path")?
            .to_owned();
        let expected = manifest
            .entries
            .iter()
            .find(|expected| expected.name == name)
            .ok_or("undeclared backup entry")?;
        if !entry.header().entry_type().is_file()
            || entry.size() != expected.bytes
            || !found.insert(name.clone())
        {
            return Err("invalid backup entry".to_owned());
        }
        let path = directory.path().join(&name);
        if name.starts_with("secrets/") && !directory.path().join("secrets").exists() {
            std::fs::create_dir(directory.path().join("secrets"))
                .map_err(|_| "cannot prepare secrets")?;
        }
        write_new(&path, &[])?;
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .map_err(|_| "cannot extract backup entry")?;
        let copied =
            std::io::copy(&mut entry, &mut output).map_err(|_| "cannot extract backup entry")?;
        if copied != expected.bytes || hash(&path, &control)? != expected.sha256 {
            return Err("backup checksum mismatch".to_owned());
        }
    }
    if found.len() != names.len() {
        return Err("incomplete backup".to_owned());
    }
    authenticate_tail(archive.into_inner())?;
    let info = elzevir_store::inspect_snapshot_controlled(
        &directory.path().join("database.sqlite"),
        control,
    )
    .map_err(|_| "backup database integrity failed")?;
    if info.schema_version != manifest.database_schema {
        return Err("backup schema mismatch".to_owned());
    }
    Ok(Extracted {
        directory,
        manifest,
    })
}

fn read_manifest<R: Read>(mut first: tar::Entry<'_, R>) -> Result<Manifest, String> {
    if first.path().map_err(|_| "invalid entry")?.as_ref() != Path::new("manifest.json")
        || first.size() > 65_536
        || !first.header().entry_type().is_file()
    {
        return Err("invalid backup manifest".to_owned());
    }
    let mut bytes = Vec::new();
    first
        .read_to_end(&mut bytes)
        .map_err(|_| "invalid backup manifest")?;
    serde_json::from_slice(&bytes).map_err(|_| "invalid backup manifest".to_owned())
}

fn authenticate_tail<R: Read>(mut gzip: flate2::read::GzDecoder<R>) -> Result<(), String> {
    // Drain past tar's terminator to authenticate the final age chunk as well.
    // Tar pads its terminator. Bound extra decompressed bytes to reject bombs.
    let drained = std::io::copy(&mut (&mut gzip).take(1025), &mut std::io::sink())
        .map_err(|_| "backup authentication failed")?;
    if drained > 1024 {
        return Err("unexpected backup tail".to_owned());
    }
    let mut encrypted = gzip.into_inner();
    let drained = std::io::copy(&mut (&mut encrypted).take(1025), &mut std::io::sink())
        .map_err(|_| "backup authentication failed")?;
    if drained > 1024 {
        return Err("unexpected backup tail".to_owned());
    }
    Ok(())
}

fn validate_manifest(manifest: &Manifest) -> Result<(BTreeSet<String>, u64), String> {
    if manifest.database_schema < 1
        || manifest.schema != 1
        || manifest.entries.len() > 32
        || manifest.database_schema > elzevir_store::supported_schema_version()
    {
        return Err("unsupported backup schema".to_owned());
    }
    let mut names = BTreeSet::new();
    let mut total = 0_u64;
    for entry in &manifest.entries {
        if !allowed(&entry.name)
            || !names.insert(entry.name.clone())
            || entry.bytes > MAX_DB
            || entry.sha256.len() != 64
            || !entry.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("invalid backup entry".to_owned());
        }
        total = total
            .checked_add(entry.bytes)
            .ok_or("backup size overflow")?;
    }
    if !names.contains("database.sqlite") || !names.contains("server.toml") {
        return Err("incomplete backup".to_owned());
    }
    let auxiliary = manifest
        .entries
        .iter()
        .filter(|entry| entry.name != "database.sqlite")
        .try_fold(0_u64, |total, entry| {
            total.checked_add(entry.bytes).ok_or("backup size overflow")
        })?;
    if auxiliary > MAX_AUX
        || manifest.id.len() != 32
        || !manifest.id.bytes().all(|byte| byte.is_ascii_hexdigit())
        || manifest.created_at.parse::<jiff::Timestamp>().is_err()
        || manifest.version.len() > 128
    {
        return Err("invalid backup manifest".to_owned());
    }
    Ok((names, total))
}

struct HeaderLimit<R> {
    source: R,
    remaining: usize,
    active: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
impl<R: Read> Read for HeaderLimit<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if !self.active.load(std::sync::atomic::Ordering::Relaxed) {
            return self.source.read(buffer);
        }
        if self.remaining == 0 {
            return Err(std::io::Error::other("encrypted header exceeds limit"));
        }
        let length = buffer.len().min(self.remaining);
        let read = self.source.read(
            buffer
                .get_mut(..length)
                .ok_or_else(|| std::io::Error::other("invalid read"))?,
        )?;
        self.remaining -= read;
        Ok(read)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encrypted_archive_roundtrips_and_preserves_existing_outputs() {
        let root = tempfile::tempdir().unwrap();
        let store = crate::open_store(root.path()).unwrap();
        let output = root.path().join("snapshot.age");
        let original = create(
            root.path(),
            "fictional-sensitive-config",
            &output,
            Some(store.store.readers()),
        )
        .unwrap();
        assert!(
            !String::from_utf8_lossy(&std::fs::read(&output).unwrap())
                .contains("fictional-sensitive-config")
        );
        assert!(create(root.path(), "another", &output, Some(store.store.readers())).is_err());
        let recovered = extract(&output, &root.path().join("backup.key")).unwrap();
        assert_eq!(recovered.manifest.id, original.id);
        assert_eq!(
            std::fs::read_to_string(recovered.directory.path().join("server.toml")).unwrap(),
            "fictional-sensitive-config"
        );
        let wrong = root.path().join("wrong.key");
        key(&wrong, true).unwrap();
        assert!(extract(&output, &wrong).is_err());
        let mut bytes = std::fs::read(&output).unwrap();
        bytes.pop();
        let corrupt = root.path().join("corrupt.age");
        std::fs::write(&corrupt, bytes).unwrap();
        assert!(extract(&corrupt, &root.path().join("backup.key")).is_err());
    }
}

#[cfg(test)]
mod account_certificate_tests {
    use super::*;
    #[test]
    fn backup_rejects_author_tables_outside_the_institution() {
        let directory = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        let table = external.path().join("authors.txt");
        std::fs::write(&table, "Ab 12\n").unwrap();
        let data = crate::open_store(directory.path()).unwrap();
        std::fs::write(
            directory.path().join("elzevir.toml"),
            format!(
                "[call_number]\nauthor_table = {}\n",
                toml::Value::String(table.to_string_lossy().into_owned())
            ),
        )
        .unwrap();
        let output = directory.path().join("backup.age");
        assert!(
            create(
                directory.path(),
                "config",
                &output,
                Some(data.store.readers())
            )
            .is_err()
        );
        assert!(!output.exists());
    }
    #[test]
    fn institution_backups_exclude_the_operators_cloudflare_account_certificate() {
        let directory = tempfile::tempdir().unwrap();
        let data = crate::open_store(directory.path()).unwrap();
        std::fs::create_dir(directory.path().join("secrets")).unwrap();
        std::fs::write(
            directory.path().join("secrets/cf-cert.pem"),
            "fictional-account-master-credential",
        )
        .unwrap();
        let output = directory.path().join("backup.age");
        let manifest = create(
            directory.path(),
            "config",
            &output,
            Some(data.store.readers()),
        )
        .unwrap();
        assert!(
            !manifest
                .entries
                .iter()
                .any(|entry| entry.name == "secrets/cf-cert.pem")
        );
        assert!(!allowed("secrets/cf-cert.pem"));
    }
}
