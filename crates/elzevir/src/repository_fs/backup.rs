// SPDX-License-Identifier: GPL-3.0-or-later
//! Transfer immutable objects across institutions under the normal backup/restore lock.
use super::{MIN_FREE, shards};
use crate::{backup_archive as archive, cancellation::Control};
use cap_fs_ext::DirExt;
use cap_std::fs::{Dir, DirBuilder, OpenOptions};
use elzevir_app::repository::ObjectInfo;
use std::{
    fs::File,
    io::{self, ErrorKind, Read, Seek},
    path::Path,
    sync::Arc,
};

fn sync(dir: &Dir) -> Result<(), String> {
    #[cfg(unix)]
    dir.open(".")
        .and_then(|file| file.sync_all())
        .map_err(|e| e.to_string())?;
    #[cfg(not(unix))]
    let _ = dir;
    Ok(())
}
fn private_child(parent: &Dir, name: &str, owner: &Path) -> Result<Dir, String> {
    #[cfg(unix)]
    let options = {
        use cap_std::fs::DirBuilderExt as _;
        let mut options = DirBuilder::new();
        options.mode(0o700);
        options
    };
    #[cfg(not(unix))]
    let options = DirBuilder::new();
    let created = match parent.create_dir_with(name, &options) {
        Ok(()) => true,
        Err(e) if e.kind() == ErrorKind::AlreadyExists => false,
        Err(e) => return Err(e.to_string()),
    };
    let dir = parent.open_dir_nofollow(name).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    if created {
        use std::os::unix::fs::MetadataExt;
        let meta = std::fs::metadata(owner).map_err(|e| e.to_string())?;
        let handle = dir.open(".").map_err(|e| e.to_string())?.into_std();
        let current = handle.metadata().map_err(|e| e.to_string())?;
        if (current.uid(), current.gid()) != (meta.uid(), meta.gid()) {
            rustix::fs::fchown(
                &handle,
                Some(rustix::process::Uid::from_raw(meta.uid())),
                Some(rustix::process::Gid::from_raw(meta.gid())),
            )
            .map_err(|e| e.to_string())?;
        }
    }
    #[cfg(not(unix))]
    let _ = owner;
    if created {
        sync(parent)?;
    }
    Ok(dir)
}

fn check_file(file: File, info: ObjectInfo, control: &Control) -> Result<(), String> {
    if file.metadata().map_err(|e| e.to_string())?.len() != info.bytes
        || archive::hash_reader(file, control)? != info.hash.to_string()
    {
        return Err(format!("repository checksum mismatch {}", info.hash));
    }
    Ok(())
}

struct Destination<'a> {
    owner: &'a Path,
    objects: Dir,
    staging: Dir,
    inventory: Dir,
    control: Control,
}
impl Destination<'_> {
    fn install(&self, source: &Path, info: ObjectInfo) -> Result<(), String> {
        let hash = info.hash.to_string();
        let (a, b) = shards(info.hash);
        let leaf = private_child(
            &private_child(&self.objects, &a, self.owner)?,
            &b,
            self.owner,
        )?;
        match leaf.symlink_metadata(&hash) {
            Ok(meta) if meta.is_file() => check_file(
                archive::scoped_file(&leaf, Path::new(&hash))?.into_std(),
                info,
                &self.control,
            )?,
            Ok(_) => return Err("repository object must be a regular file".into()),
            Err(e) if e.kind() == ErrorKind::NotFound => self.copy_missing(&leaf, source, info)?,
            Err(e) => return Err(e.to_string()),
        }
        match self.inventory.remove_file(hash) {
            Ok(()) => sync(&self.inventory),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
    fn copy_missing(&self, leaf: &Dir, source: &Path, info: ObjectInfo) -> Result<(), String> {
        let free = fs4::available_space(self.owner.join("repository/staging"))
            .map_err(|e| e.to_string())?;
        if free.saturating_sub(info.bytes) < MIN_FREE {
            return Err("insufficient space to restore repository objects".into());
        }
        let name = format!("upload-{}", archive::identifier()?);
        let mut options = OpenOptions::new();
        options.create_new(true).write(true).read(true);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let mut staged = self
            .staging
            .open_with(&name, &options)
            .map_err(|e| e.to_string())?
            .into_std();
        let result = (|| {
            let mut input = File::open(source)
                .map_err(|e| e.to_string())?
                .take(info.bytes + 1);
            if io::copy(&mut input, &mut staged).map_err(|e| e.to_string())? != info.bytes {
                return Err(format!("repository object size mismatch {}", info.hash));
            }
            crate::file_owner::private_file(&staged, self.owner)?;
            staged
                .sync_all()
                .and_then(|()| staged.rewind())
                .map_err(|e| e.to_string())?;
            check_file(staged, info, &self.control)?;
            self.staging
                .hard_link(&name, leaf, info.hash.to_string())
                .map_err(|e| e.to_string())?;
            sync(leaf)
        })();
        let cleanup = self.staging.remove_file(&name).map_err(|e| e.to_string());
        result.and(cleanup)
    }
}

/// Complete installation before the restore journal can publish the new catalog.
/// Partial installs are unreachable objects subject to ordinary maintenance grace.
pub(crate) fn restore_archive(
    data: &Path,
    restored: &archive::Extracted,
    _lease: &crate::InstitutionLock,
) -> Result<(), String> {
    if restored.manifest.schema != 3 {
        return Ok(());
    }
    let scope =
        Dir::open_ambient_dir(data, cap_std::ambient_authority()).map_err(|e| e.to_string())?;
    let repo = private_child(&scope, "repository", data)?;
    let destination = Destination {
        owner: data,
        objects: private_child(&private_child(&repo, "objects", data)?, "sha256", data)?,
        staging: private_child(&repo, "staging", data)?,
        inventory: private_child(&repo, "maintenance", data)?,
        control: Arc::new(|| false),
    };
    for entry in &restored.manifest.entries {
        if let Some(info) = entry.object() {
            destination.install(&restored.directory.path().join(&entry.name), info)?;
        }
    }
    Ok(())
}
