// SPDX-License-Identifier: GPL-3.0-or-later
//! Private content-addressed objects of a single, exclusively opened institution.
mod backup;
pub(super) use backup::restore_archive;
mod maintenance;
pub(super) use maintenance::pin_snapshot;
#[cfg(test)]
mod tests;
use elzevir_app::repository::{
    InstalledObject, Integrity, ObjectHash, ObjectInfo, ObjectStore, RepositoryError as Error,
    Result,
};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{self, Read, Seek, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

const CHUNK: usize = 256 * 1024;
pub(super) const MAX_BYTES: u64 = 1 << 30;
pub(super) const MIN_FREE: u64 = 2 << 30;
struct Session {
    _lock: crate::InstitutionLock,
    uploading: AtomicBool,
}
struct UploadGuard(Arc<Session>);
impl Drop for UploadGuard {
    fn drop(&mut self) {
        self.0.uploading.store(false, Ordering::Release);
    }
}
pub(super) struct Installation {
    info: ObjectInfo,
    _upload: UploadGuard,
}
impl InstalledObject for Installation {
    fn info(&self) -> ObjectInfo {
        self.info
    }
}

struct Reader {
    file: io::BufReader<File>,
    _session: Arc<Session>,
}
impl Read for Reader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.file.read(buf)
    }
}

pub(super) struct FsObjectStore {
    data: PathBuf,
    session: Arc<Session>,
    objects: PathBuf,
    staging: PathBuf,
    inventory: PathBuf,
    max_bytes: u64,
    min_free: u64,
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "maps owned I/O errors from Result::map_err"
)]
fn failure(err: io::Error) -> Error {
    match err.kind() {
        io::ErrorKind::StorageFull | io::ErrorKind::QuotaExceeded => Error::InsufficientSpace,
        _ => Error::Unavailable,
    }
}
fn sync_dir(path: &Path) -> Result<()> {
    crate::installation::sync_directory(path).map_err(|_| Error::Unavailable)
}
fn directory(parent: &Path, name: &str) -> Result<PathBuf> {
    let path = parent.join(name);
    crate::backup_cli::make_directory(&path).map_err(|_| Error::Unavailable)?;
    // Also sync an existing directory entry: a previous installation might have
    // died immediately after mkdir, before it could acknowledge durability.
    sync_dir(parent)?;
    Ok(path)
}
fn shards(hash: ObjectHash) -> (String, String) {
    let [a, b, ..] = hash.0;
    (format!("{a:02x}"), format!("{b:02x}"))
}
fn read_chunk(source: &mut dyn Read, buffer: &mut [u8]) -> io::Result<usize> {
    loop {
        match source.read(buffer) {
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            result => return result,
        }
    }
}

impl FsObjectStore {
    pub(super) fn new(
        data: &Path,
        lock: crate::InstitutionLock,
        max_bytes: u64,
        min_free: u64,
    ) -> Result<Self> {
        let root = directory(data, "repository")?;
        let objects = directory(&directory(&root, "objects")?, "sha256")?;
        Ok(Self {
            data: data.to_owned(),
            session: Arc::new(Session {
                _lock: lock,
                uploading: AtomicBool::new(false),
            }),
            staging: directory(&root, "staging")?,
            inventory: directory(&root, "maintenance")?,
            objects,
            max_bytes,
            min_free,
        })
    }
    fn space(&self, next: u64) -> Result<()> {
        let free = fs4::available_space(&self.staging).map_err(failure)?;
        if free
            .checked_sub(next)
            .is_none_or(|rest| rest < self.min_free)
        {
            return Err(Error::InsufficientSpace);
        }
        Ok(())
    }
    fn verified(&self, info: ObjectInfo) -> std::result::Result<File, Integrity> {
        let (a, b) = shards(info.hash);
        let not_found = |e: io::Error| {
            if e.kind() == io::ErrorKind::NotFound {
                Integrity::Missing
            } else {
                Integrity::Corrupt
            }
        };
        let first = self.objects.join(a);
        let parent = first.join(b);
        for dir in [&first, &parent] {
            if !fs::symlink_metadata(dir).map_err(not_found)?.is_dir() {
                return Err(Integrity::Corrupt);
            }
        }
        let path = parent.join(info.hash.to_string());
        let meta = fs::symlink_metadata(&path).map_err(not_found)?;
        if !meta.is_file() || meta.len() != info.bytes {
            return Err(Integrity::Corrupt);
        }
        verify_bytes(File::open(path).map_err(not_found)?, info)
    }
    fn forget_observation(&self, hash: ObjectHash) -> Result<()> {
        match fs::remove_file(self.inventory.join(hash.to_string())) {
            Ok(()) => sync_dir(&self.inventory),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(failure(e)),
        }
    }
}

/// Open a regular, correctly sized object through its institution directory handle.
pub(super) fn object_source(
    scope: &cap_std::fs::Dir,
    info: ObjectInfo,
) -> std::result::Result<File, String> {
    use cap_fs_ext::DirExt as _;
    let mut leaf = scope.try_clone().map_err(|e| e.to_string())?;
    let (a, b) = shards(info.hash);
    for name in ["repository", "objects", "sha256", &a, &b] {
        leaf = leaf
            .open_dir_nofollow(name)
            .map_err(|_| format!("missing repository object {}", info.hash))?;
    }
    let hash = info.hash.to_string();
    if !leaf
        .symlink_metadata(&hash)
        .is_ok_and(|meta| meta.is_file())
    {
        return Err(format!("missing or non-regular repository object {hash}"));
    }
    let file = crate::backup_archive::scoped_file(&leaf, Path::new(&hash))?.into_std();
    if file.metadata().map_err(|e| e.to_string())?.len() != info.bytes {
        return Err(format!("repository object size mismatch {}", info.hash));
    }
    Ok(file)
}

/// Verify the open handle and rewind that same handle for streaming.
pub(super) fn verify_bytes(
    mut file: File,
    info: ObjectInfo,
) -> std::result::Result<File, Integrity> {
    let mut buffer = vec![0; CHUNK];
    let mut digest = Sha256::new();
    let mut total = 0u64;
    loop {
        let n = read_chunk(&mut file, &mut buffer).map_err(|_| Integrity::Corrupt)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > info.bytes {
            return Err(Integrity::Corrupt);
        }
        digest.update(buffer.get(..n).ok_or(Integrity::Corrupt)?);
    }
    if total != info.bytes || ObjectHash(digest.finalize().into()) != info.hash {
        return Err(Integrity::Corrupt);
    }
    file.rewind().map_err(|_| Integrity::Corrupt)?;
    Ok(file)
}

impl ObjectStore for FsObjectStore {
    type Installed = Installation;
    fn install(
        &self,
        source: &mut dyn Read,
        expected: Option<ObjectHash>,
        _now: i64,
    ) -> Result<Installation> {
        self.session
            .uploading
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| Error::UploadBusy)?;
        let upload = UploadGuard(Arc::clone(&self.session));
        self.space(0)?;
        let mut tmp = tempfile::Builder::new()
            .prefix("upload-")
            .tempfile_in(&self.staging)
            .map_err(failure)?;
        let mut buffer = vec![0; CHUNK];
        let mut digest = Sha256::new();
        let mut bytes = 0u64;
        loop {
            let n = read_chunk(source, &mut buffer).map_err(failure)?;
            if n == 0 {
                break;
            }
            bytes = bytes
                .checked_add(n as u64)
                .filter(|v| *v <= self.max_bytes)
                .ok_or(Error::TooLarge)?;
            self.space(n as u64)?;
            let chunk = buffer.get(..n).ok_or(Error::Unavailable)?;
            tmp.write_all(chunk).map_err(failure)?;
            digest.update(chunk);
        }
        if bytes == 0 {
            return Err(Error::InvalidState);
        }
        let hash = ObjectHash(digest.finalize().into());
        if expected.is_some_and(|h| h != hash) {
            return Err(Error::ChecksumMismatch);
        }
        tmp.as_file().sync_all().map_err(failure)?;
        let info = ObjectInfo { hash, bytes };
        let (a, b) = shards(hash);
        let parent = directory(&directory(&self.objects, &a)?, &b)?;
        self.forget_observation(hash)?;
        match tmp.persist_noclobber(parent.join(hash.to_string())) {
            Ok(_) => {}
            Err(e) if e.error.kind() == io::ErrorKind::AlreadyExists => {
                self.verified(info).map_err(|_| Error::ObjectUnavailable)?;
            }
            Err(e) => return Err(failure(e.error)),
        }
        sync_dir(&parent)?;
        Ok(Installation {
            info,
            _upload: upload,
        })
    }
    fn integrity(&self, info: ObjectInfo) -> Integrity {
        self.verified(info).map_or_else(|e| e, |_| Integrity::Ok)
    }
    fn open(&self, info: ObjectInfo) -> Result<Box<dyn Read + Send>> {
        let file = self.verified(info).map_err(|_| Error::ObjectUnavailable)?;
        Ok(Box::new(Reader {
            file: io::BufReader::with_capacity(CHUNK, file),
            _session: Arc::clone(&self.session),
        }))
    }
}
