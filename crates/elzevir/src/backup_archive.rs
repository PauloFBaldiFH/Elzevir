// SPDX-License-Identifier: GPL-3.0-or-later
//! Streaming authenticated backups; no live database, key or index is copied blindly.

use crate::cancellation::{Control, Reader as Controlled, check};
use age::secrecy::ExposeSecret as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{BufRead, BufReader, Read, Write},
    path::Path,
};

const MAX_OBJECTS: usize = 8192;
const MAX_MANIFEST: u64 = 2 * 1024 * 1024;
const MAX_AUX: u64 = 16 * 1024 * 1024;
pub(crate) const MAX_DB: u64 = 512 * 1024 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Phase {
    Snapshot,
    Manifest,
    Compress,
    Verify,
    Publish,
    Catalog,
}
impl Phase {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Snapshot => "backup-phase-snapshot",
            Self::Manifest => "backup-phase-manifest",
            Self::Compress => "backup-phase-compress",
            Self::Verify => "backup-phase-verify",
            Self::Publish => "backup-phase-publish",
            Self::Catalog => "backup-phase-catalog",
        }
    }
}

fn private_scratch(directory: &Path) -> Result<tempfile::TempDir, String> {
    #[cfg(target_os = "linux")]
    if !rustix::process::geteuid().is_root() {
        return crate::private_temp::directory_in(directory)
            .map_err(|_| "cannot prepare backup".to_owned());
    }
    #[cfg(windows)]
    if crate::service::require_operator().is_err() {
        return crate::private_temp::directory_in(directory)
            .map_err(|_| "cannot prepare private backup scratch".to_owned());
    }
    let _ = directory;
    crate::private_temp::directory().map_err(|_| "cannot prepare private backup scratch".to_owned())
}

pub(super) fn scoped_file(
    scope: &cap_std::fs::Dir,
    relative: &Path,
) -> Result<cap_std::fs::File, String> {
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

impl Entry {
    pub(crate) fn object(&self) -> Option<elzevir_app::repository::ObjectInfo> {
        let hex = self.name.strip_prefix("object-")?;
        if hex != self.sha256 || self.bytes == 0 {
            return None;
        }
        Some(elzevir_app::repository::ObjectInfo {
            hash: hex.parse().ok()?,
            bytes: self.bytes,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Manifest {
    pub schema: u8,
    #[serde(default)]
    pub scope: elzevir_panel::BackupScope,
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

pub(crate) fn hash(path: &Path, control: &Control) -> Result<String, String> {
    let source = std::fs::File::open(path).map_err(|_| "snapshot file unavailable")?;
    hash_reader(source, control)
}

pub(crate) fn hash_reader(mut source: impl Read, control: &Control) -> Result<String, String> {
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

/// Writes an encrypted Zstandard-compressed snapshot and publishes it without replacement.
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
    create_selected(
        directory,
        config,
        output,
        readers,
        control,
        elzevir_panel::BackupOptions::default(),
        |_| Ok(()),
    )
}

pub(crate) fn create_selected(
    directory: &Path,
    config: &str,
    output: &Path,
    readers: Option<&elzevir_store::ReadPool>,
    control: &Control,
    options: elzevir_panel::BackupOptions,
    phase: impl Fn(Phase) -> Result<(), String>,
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
    phase(Phase::Snapshot)?;
    let database = match readers {
        Some(pool) => pool.snapshot_controlled(&image, std::sync::Arc::clone(control)),
        None => elzevir_store::snapshot_offline(&directory.join("elzevir.db"), &image),
    }
    .map_err(|_| "database snapshot failed")?;
    if database.bytes > MAX_DB {
        return Err("database exceeds backup limit".to_owned());
    }
    phase(Phase::Manifest)?;
    let mut files = vec![("database.sqlite".to_owned(), image)];
    if options.scope == elzevir_panel::BackupScope::Full {
        let server = scratch.path().join("server.toml");
        write_new(&server, config.as_bytes())?;
        files.push(("server.toml".to_owned(), server));
        files = additional_files(directory, &scope, files)?;
    }
    let mut manifest = Manifest {
        schema: 2,
        scope: options.scope,
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
    if manifest.scope == elzevir_panel::BackupScope::Full {
        let objects = repository_inventory(&scratch.path().join("database.sqlite"))?;
        if !objects.is_empty() {
            manifest.schema = 3;
            manifest
                .entries
                .extend(objects.into_iter().map(|object| Entry {
                    name: format!("object-{}", object.hash),
                    sha256: object.hash.to_string(),
                    bytes: object.bytes,
                }));
        }
    }
    validate_manifest(&manifest)?;
    phase(Phase::Compress)?;
    let mut file = tempfile::NamedTempFile::new_in(output.parent().ok_or("backup needs parent")?)
        .map_err(|_| "cannot prepare backup output")?;
    encrypt(
        file.as_file_mut(),
        &manifest,
        &files,
        &scope,
        &identity.to_public(),
        options.profile.level(),
        control,
    )?;
    file.as_file()
        .sync_all()
        .map_err(|_| "cannot sync backup")?;
    drop(scratch);
    phase(Phase::Verify)?;
    let verified = read_archive(
        file.path(),
        &directory.join("backup.key"),
        std::sync::Arc::clone(control),
        Some(directory),
        ObjectOutput::Verify,
    )?;
    check(control)?;
    phase(Phase::Publish)?;
    crate::repository_fs::pin_snapshot(
        directory,
        &verified.directory.path().join("database.sqlite"),
        output,
        &manifest.id,
    )?;
    file.persist_noclobber(output)
        .map_err(|_| "cannot publish backup without replacement")?;
    Ok(manifest)
}

fn encrypt(
    file: &mut impl Write,
    manifest: &Manifest,
    files: &[(String, std::path::PathBuf)],
    scope: &cap_std::fs::Dir,
    recipient: &age::x25519::Recipient,
    level: i32,
    control: &Control,
) -> Result<(), String> {
    let encryptor =
        age::Encryptor::with_recipients(std::iter::once(recipient as &dyn age::Recipient))
            .map_err(|_| "cannot encrypt backup")?;
    let writer = encryptor
        .wrap_output(file)
        .map_err(|_| "cannot encrypt backup")?;
    let compressed = zstd::stream::write::Encoder::new(writer, level)
        .map_err(|_| "cannot initialize Zstandard")?;
    let mut archive = tar::Builder::new(compressed);
    let bytes = serde_json::to_vec(manifest).map_err(|_| "cannot encode backup manifest")?;
    let mut header = tar::Header::new_gnu();
    header.set_size(u64::try_from(bytes.len()).map_err(|_| "manifest too large")?);
    header.set_mode(0o600);
    header.set_cksum();
    archive
        .append_data(&mut header, "manifest.json", bytes.as_slice())
        .map_err(|_| "cannot write backup manifest")?;
    for (index, entry) in manifest.entries.iter().enumerate() {
        check(control)?;
        let file = if let Some(object) = entry.object() {
            crate::repository_fs::object_source(scope, object)?
        } else {
            let (_, path) = files
                .get(index)
                .filter(|(name, _)| name == &entry.name)
                .ok_or("missing staged backup entry")?;
            std::fs::File::open(path).map_err(|_| "backup entry unavailable")?
        };
        let mut header = tar::Header::new_gnu();
        header.set_size(entry.bytes);
        header.set_mode(0o600);
        header.set_entry_type(tar::EntryType::Regular);
        header.set_cksum();
        archive
            .append_data(
                &mut header,
                &entry.name,
                Controlled {
                    source: file.take(entry.bytes),
                    control: std::sync::Arc::clone(control),
                },
            )
            .map_err(|_| "cannot write backup entry")?;
    }
    archive
        .into_inner()
        .and_then(zstd::stream::write::Encoder::finish)
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
pub(crate) fn extract_full(input: &Path, key_file: &Path) -> Result<Extracted, String> {
    let extracted = extract(input, key_file)?;
    if extracted.manifest.scope != elzevir_panel::BackupScope::Full {
        return Err("full restore requires a full institution backup".into());
    }
    Ok(extracted)
}
pub(crate) fn extract(input: &Path, key_file: &Path) -> Result<Extracted, String> {
    read_archive(
        input,
        key_file,
        std::sync::Arc::new(|| false),
        None,
        ObjectOutput::Extract,
    )
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum ObjectOutput {
    Extract,
    Verify,
}

pub(crate) fn verify(input: &Path, key_file: &Path) -> Result<Manifest, String> {
    verify_controlled(input, key_file, std::sync::Arc::new(|| false), None)
}
pub(crate) fn verify_controlled(
    input: &Path,
    key_file: &Path,
    control: Control,
    scratch: Option<&Path>,
) -> Result<Manifest, String> {
    Ok(read_archive(input, key_file, control, scratch, ObjectOutput::Verify)?.manifest)
}

fn read_archive(
    input: &Path,
    key_file: &Path,
    control: Control,
    scratch: Option<&Path>,
    object_output: ObjectOutput,
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
    let mut reader = BufReader::new(reader);
    let decoder = decompress(&mut reader)?;
    let mut archive = tar::Archive::new(decoder);
    let mut entries = archive
        .entries()
        .map_err(|_| "invalid backup archive")?
        .raw(true);
    let first = entries
        .next()
        .ok_or("backup manifest absent")?
        .map_err(|_| "invalid backup manifest")?;
    let manifest = read_manifest(first)?;
    let (declared, total) = validate_manifest(&manifest)?;
    let directory = match scratch {
        Some(directory) => private_scratch(directory)?,
        None => crate::private_temp::directory().map_err(|_| "cannot prepare restore")?,
    };
    let scratch_bytes = if object_output == ObjectOutput::Extract {
        total
    } else {
        manifest
            .entries
            .iter()
            .filter(|e| e.object().is_none())
            .map(|e| e.bytes)
            .sum()
    };
    if fs4::available_space(directory.path()).map_err(|_| "cannot inspect restore space")?
        < scratch_bytes.saturating_add(16 * 1024 * 1024)
    {
        return Err("insufficient restore space".to_owned());
    }
    let mut found = BTreeSet::new();
    for entry in entries {
        let entry = entry.map_err(|_| "invalid backup entry")?;
        let name = entry
            .path()
            .map_err(|_| "invalid backup path")?
            .to_str()
            .ok_or("invalid backup path")?
            .to_owned();
        let expected = declared
            .get(name.as_str())
            .ok_or("undeclared backup entry")?;
        if !entry.header().entry_type().is_file()
            || entry.size() != expected.bytes
            || !found.insert(name)
        {
            return Err("invalid backup entry".to_owned());
        }
        verify_entry(entry, expected, directory.path(), object_output, &control)?;
    }
    if found.len() != declared.len() {
        return Err("incomplete backup".to_owned());
    }
    authenticate_tail(archive.into_inner())?;
    if !reader
        .fill_buf()
        .map_err(|_| "backup authentication failed")?
        .is_empty()
    {
        return Err("unexpected compressed tail".to_owned());
    }
    verify_database(
        &directory.path().join("database.sqlite"),
        manifest.database_schema,
        control,
    )?;
    if manifest.schema == 3 {
        verify_inventory(&directory.path().join("database.sqlite"), &manifest)?;
    }
    Ok(Extracted {
        directory,
        manifest,
    })
}

fn verify_entry(
    mut entry: impl Read,
    expected: &Entry,
    directory: &Path,
    object_output: ObjectOutput,
    control: &Control,
) -> Result<(), String> {
    let name = &expected.name;
    let (size_ok, digest) = if object_output == ObjectOutput::Verify && expected.object().is_some()
    {
        let mut bytes = entry.take(expected.bytes);
        let digest = hash_reader(&mut bytes, control)?;
        (bytes.limit() == 0, digest)
    } else {
        let path = directory.join(name);
        if name.starts_with("secrets/") && !directory.join("secrets").exists() {
            std::fs::create_dir(directory.join("secrets")).map_err(|_| "cannot prepare secrets")?;
        }
        write_new(&path, &[])?;
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .map_err(|_| "cannot extract backup entry")?;
        let copied =
            std::io::copy(&mut entry, &mut output).map_err(|_| "cannot extract backup entry")?;
        (copied == expected.bytes, hash(&path, control)?)
    };
    if !size_ok || digest != expected.sha256 {
        return Err(format!("backup checksum mismatch for {name}"));
    }
    Ok(())
}

fn verify_database(path: &Path, schema: i64, control: Control) -> Result<(), String> {
    let info = elzevir_store::inspect_snapshot_controlled(path, control)
        .map_err(|_| "backup database integrity failed")?;
    if info.schema_version != schema {
        return Err("backup schema mismatch".to_owned());
    }
    Ok(())
}

fn read_manifest<R: Read>(mut first: tar::Entry<'_, R>) -> Result<Manifest, String> {
    if first.path().map_err(|_| "invalid entry")?.as_ref() != Path::new("manifest.json")
        || first.size() > MAX_MANIFEST
        || !first.header().entry_type().is_file()
    {
        return Err("invalid backup manifest".to_owned());
    }
    let mut bytes = Vec::new();
    first
        .read_to_end(&mut bytes)
        .map_err(|_| "invalid backup manifest")?;
    let manifest: Manifest =
        serde_json::from_slice(&bytes).map_err(|_| "invalid backup manifest")?;
    if manifest.schema < 3 && bytes.len() > 65536 {
        return Err("legacy backup manifest exceeds limit".into());
    }
    if manifest.schema >= 2 {
        #[derive(Deserialize)]
        struct RequiredScope {
            scope: elzevir_panel::BackupScope,
        }
        let scoped: RequiredScope =
            serde_json::from_slice(&bytes).map_err(|_| "backup scope required")?;
        if scoped.scope != manifest.scope {
            return Err("inconsistent backup scope".into());
        }
    }
    Ok(manifest)
}

fn decompress(reader: &mut impl BufRead) -> Result<Box<dyn Read + '_>, String> {
    let header = reader
        .fill_buf()
        .map_err(|_| "invalid compressed payload")?;
    Ok(if header.starts_with(&[0x1f, 0x8b]) {
        Box::new(flate2::bufread::GzDecoder::new(reader))
    } else if header.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) {
        let mut decoder = zstd::stream::read::Decoder::with_buffer(reader)
            .map_err(|_| "invalid Zstandard payload")?
            .single_frame();
        decoder
            .window_log_max(23)
            .map_err(|_| "unsupported Zstandard window")?;
        Box::new(decoder)
    } else {
        return Err("unsupported backup compression".to_owned());
    })
}

fn authenticate_tail<R: Read>(mut decoded: R) -> Result<(), String> {
    // Drain past tar's terminator to authenticate the final age chunk as well.
    // Tar pads its terminator. Bound extra decompressed bytes to reject bombs.
    let drained = std::io::copy(&mut (&mut decoded).take(1025), &mut std::io::sink())
        .map_err(|_| "backup authentication failed")?;
    if drained > 1024 {
        return Err("unexpected backup tail".to_owned());
    }
    Ok(())
}

fn repository_inventory(image: &Path) -> Result<Vec<elzevir_app::repository::ObjectInfo>, String> {
    elzevir_store::repository::backup::snapshot_objects(image, MAX_OBJECTS)
        .map_err(|e| e.to_string())
}
fn verify_inventory(image: &Path, manifest: &Manifest) -> Result<(), String> {
    let expected = repository_inventory(image)?;
    let mut actual: Vec<_> = manifest.entries.iter().filter_map(Entry::object).collect();
    actual.sort_unstable_by_key(|object| object.hash);
    if actual != expected {
        return Err("repository manifest disagrees with database snapshot".into());
    }
    Ok(())
}

fn validate_manifest(manifest: &Manifest) -> Result<(BTreeMap<&str, &Entry>, u64), String> {
    if manifest.database_schema < 1
        || !matches!(manifest.schema, 1..=3)
        || (manifest.schema != 2 && manifest.scope != elzevir_panel::BackupScope::Full)
        || manifest.entries.len() > 32 + MAX_OBJECTS
        || manifest.database_schema > elzevir_store::supported_schema_version()
    {
        return Err("unsupported backup schema".into());
    }
    let mut names = BTreeMap::new();
    let mut auxiliary = 0u64;
    let mut total = 0u64;
    let mut objects = 0;
    for entry in &manifest.entries {
        if manifest.schema == 3 && entry.object().is_some() {
            objects += 1;
        } else if allowed(&entry.name) {
            if entry.name != "database.sqlite" {
                auxiliary = auxiliary
                    .checked_add(entry.bytes)
                    .ok_or("backup size overflow")?;
            }
        } else {
            return Err("invalid backup entry".into());
        }
        if names.insert(entry.name.as_str(), entry).is_some()
            || entry.bytes > MAX_DB
            || entry.sha256.len() != 64
            || !entry.sha256.bytes().all(|c| c.is_ascii_hexdigit())
        {
            return Err("invalid backup entry".into());
        }
        total = total
            .checked_add(entry.bytes)
            .ok_or("backup size overflow")?;
    }
    if objects > MAX_OBJECTS
        || names.len() - objects > 32
        || (manifest.schema == 3 && (objects == 0 || total > MAX_DB))
        || !names.contains_key("database.sqlite")
        || match manifest.scope {
            elzevir_panel::BackupScope::Full => !names.contains_key("server.toml"),
            elzevir_panel::BackupScope::Database => names.len() != 1,
        }
    {
        return Err("incomplete or oversized backup".into());
    }
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
    fn verification_streams_objects_and_restore_extracts_the_same_bytes() {
        let root = tempfile::tempdir().unwrap();
        let control: Control = std::sync::Arc::new(|| false);
        let bytes = b"example thesis";
        let sha256 = hash_reader(bytes.as_slice(), &control).unwrap();
        let mut object = Entry {
            name: format!("object-{sha256}"),
            bytes: bytes.len() as u64,
            sha256,
        };
        let verify = |object: &Entry, mode| {
            verify_entry(bytes.as_slice(), object, root.path(), mode, &control)
        };
        verify(&object, ObjectOutput::Verify).unwrap();
        object.bytes += 1;
        assert!(verify(&object, ObjectOutput::Verify).is_err());
        assert_eq!(root.path().read_dir().unwrap().count(), 0);
        object.bytes -= 1;
        verify(&object, ObjectOutput::Extract).unwrap();
        assert_eq!(std::fs::read(root.path().join(object.name)).unwrap(), bytes);
    }
    #[test]
    fn repository_manifest_rejects_extra_objects_aliases_and_duplicates() {
        let root = tempfile::tempdir().unwrap();
        let _catalog = crate::open_store(root.path()).unwrap();
        let hash = "01".repeat(32);
        let object = Entry {
            name: format!("object-{hash}"),
            sha256: hash.clone(),
            bytes: 1,
        };
        let mut manifest = Manifest {
            schema: 3,
            scope: elzevir_panel::BackupScope::Full,
            id: "00".repeat(16),
            created_at: "2026-01-01T00:00:00Z".into(),
            version: "test".into(),
            database_schema: 62,
            entries: vec![
                Entry {
                    name: "database.sqlite".into(),
                    sha256: hash.clone(),
                    bytes: 1,
                },
                Entry {
                    name: "server.toml".into(),
                    sha256: hash,
                    bytes: 1,
                },
                object.clone(),
            ],
        };
        validate_manifest(&manifest).unwrap();
        assert!(verify_inventory(&root.path().join("elzevir.db"), &manifest).is_err());
        manifest.entries.push(object);
        assert!(validate_manifest(&manifest).is_err());
        manifest.entries.pop().unwrap();
        let entry = manifest.entries.last_mut().unwrap();
        entry.name = format!("object-../{}", entry.sha256);
        assert!(validate_manifest(&manifest).is_err());
        manifest.entries.last_mut().unwrap().name = format!("object-{}", "02".repeat(32));
        assert!(validate_manifest(&manifest).is_err());
    }
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

#[cfg(test)]
mod format_tests {
    use super::*;

    #[test]
    fn restores_gzip_fixture_created_by_7891109e() {
        let target = tempfile::tempdir().unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/gzip-v1/library.age");
        let key = target.path().join("fixture.key");
        write_new(&key, include_bytes!("../tests/data/gzip-v1/fixture.key")).unwrap();
        let legacy = extract(&source, &key).unwrap();
        assert_eq!(legacy.manifest.schema, 1);
        assert_eq!(legacy.manifest.scope, elzevir_panel::BackupScope::Full);
        let data = crate::open_store(target.path()).unwrap();
        {
            let crate::DataDir {
                store,
                _lock: _held,
            } = data;
            store.shutdown().unwrap();
        }
        crate::backup_cli::restore(&source, &key, target.path(), None).unwrap();
        assert!(target.path().join("index").is_dir());
        elzevir_store::inspect_snapshot(&target.path().join("elzevir.db")).unwrap();
    }

    #[test]
    fn database_only_uses_a_zstd_frame_and_excludes_all_settings() {
        let root = tempfile::tempdir().unwrap();
        let store = crate::open_store(root.path()).unwrap();
        let output = root.path().join("snapshot.age");
        let control: Control = std::sync::Arc::new(|| false);
        let opts = elzevir_panel::BackupOptions {
            scope: elzevir_panel::BackupScope::Database,
            profile: elzevir_panel::BackupProfile::Compact,
        };
        create_selected(
            root.path(),
            "sensitive-config",
            &output,
            Some(store.store.readers()),
            &control,
            opts,
            |_| Ok(()),
        )
        .unwrap();
        let key_path = root.path().join("backup.key");
        let recovered = extract(&output, &key_path).unwrap();
        assert_eq!(recovered.manifest.scope, opts.scope);
        assert_eq!(recovered.manifest.entries.len(), 1);
        assert!(!recovered.directory.path().join("server.toml").exists());
        let identity = key(&key_path, false).unwrap();
        let decryptor = age::Decryptor::new(std::fs::File::open(output).unwrap()).unwrap();
        let mut reader = decryptor
            .decrypt(std::iter::once(&identity as &dyn age::Identity))
            .unwrap();
        let mut magic = [0; 4];
        reader.read_exact(&mut magic).unwrap();
        assert_eq!(magic, [0x28, 0xb5, 0x2f, 0xfd]);
    }

    #[test]
    fn rejects_excess_decompressed_padding() {
        let bytes = zstd::stream::encode_all(&[0u8; 4096][..], 1).unwrap();
        let decoder = zstd::stream::read::Decoder::new(bytes.as_slice()).unwrap();
        assert!(authenticate_tail(decoder).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn full_disk_fails_the_encrypted_writer() {
        let mut full = std::fs::OpenOptions::new()
            .write(true)
            .open("/dev/full")
            .unwrap();
        let manifest = Manifest {
            schema: 2,
            scope: elzevir_panel::BackupScope::Full,
            id: identifier().unwrap(),
            created_at: jiff::Timestamp::now().to_string(),
            database_schema: 1,
            version: "test".into(),
            entries: Vec::new(),
        };
        let recipient = age::x25519::Identity::generate().to_public();
        let control: Control = std::sync::Arc::new(|| false);
        assert!(
            encrypt(
                &mut full,
                &manifest,
                &[],
                &cap_std::fs::Dir::open_ambient_dir(".", cap_std::ambient_authority()).unwrap(),
                &recipient,
                3,
                &control
            )
            .is_err()
        );
    }
}
