// SPDX-License-Identifier: GPL-3.0-or-later
//! Administrator-owned binding prevents cross-institution binary replacement.

use serde::{Deserialize, Serialize};
use std::{
    io::Read as _,
    path::{Path, PathBuf},
};
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    schema: u8,
    executable: PathBuf,
    directory: PathBuf,
    config: PathBuf,
    service: String,
}
pub(crate) fn absolute(path: &Path) -> Result<PathBuf, String> {
    let parent = path
        .parent()
        .ok_or("program/configuration needs a parent")?
        .canonicalize()
        .map_err(|_| "program/configuration parent unavailable")?;
    Ok(parent.join(
        path.file_name()
            .ok_or("program/configuration filename required")?,
    ))
}
pub(crate) fn controlled_directory(path: &Path) -> Result<PathBuf, String> {
    let path = path
        .canonicalize()
        .map_err(|_| "program directory unavailable")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        if rustix::process::geteuid().is_root() {
            let mut child_owner = 0;
            for ancestor in path.ancestors() {
                let metadata = std::fs::symlink_metadata(ancestor)
                    .map_err(|_| "program directory unavailable")?;
                if !metadata.is_dir()
                    || metadata.uid() != 0
                    || metadata.mode() & 0o022 != 0
                        && !(metadata.mode() & 0o1000 != 0 && child_owner == 0)
                {
                    return Err("program directories must be administrator controlled".to_owned());
                }
                child_owner = metadata.uid();
            }
        }
    }
    Ok(path)
}
pub(crate) fn record_path(executable: &Path) -> Result<PathBuf, String> {
    use sha2::{Digest as _, Sha256};
    let executable = absolute(executable)?;
    let id = format!(
        "{:x}",
        Sha256::digest(executable.as_os_str().as_encoded_bytes())
    );
    let parent = executable.parent().ok_or("program parent required")?;
    Ok(controlled_directory(parent)?.join(format!(
        ".elzevir-instance-{}.json",
        id.get(..16).ok_or("invalid program identity")?
    )))
}
fn expected(
    executable: &Path,
    directory: &Path,
    config: &Path,
    service: &str,
) -> Result<Binding, String> {
    if !crate::service::valid_name(service) {
        return Err("invalid managed service".to_owned());
    }
    Ok(Binding {
        schema: 1,
        executable: absolute(executable)?,
        directory: directory
            .canonicalize()
            .map_err(|_| "institution unavailable")?,
        config: absolute(config)?,
        service: service.to_owned(),
    })
}
fn read(path: &Path) -> Result<Binding, String> {
    if crate::backup_archive::regular(path)? > 8192 {
        return Err("installation binding exceeds limit".to_owned());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        let metadata = std::fs::metadata(path).map_err(|_| "installation binding unavailable")?;
        if rustix::process::geteuid().is_root()
            && (metadata.uid() != 0 || metadata.mode() & 0o022 != 0)
        {
            return Err("installation binding is not administrator controlled".to_owned());
        }
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(8193).read_to_end(&mut bytes))
        .map_err(|_| "installation binding unavailable")?;
    serde_json::from_slice(&bytes).map_err(|_| "invalid installation binding".to_owned())
}
pub(crate) fn claim(
    executable: &Path,
    directory: &Path,
    config: &Path,
    service: &str,
) -> Result<(), String> {
    let path = record_path(executable)?;
    let binding = expected(executable, directory, config, service)?;
    if std::fs::symlink_metadata(&path).is_ok() {
        return verify(executable, directory, config, service);
    }
    let bytes = serde_json::to_vec(&binding).map_err(|_| "cannot encode installation binding")?;
    crate::backup_archive::write_new(&path, &bytes)?;
    crate::file_owner::operator_private(&path)
}
pub(crate) fn verify(
    executable: &Path,
    directory: &Path,
    config: &Path,
    service: &str,
) -> Result<(), String> {
    let binding = read(&record_path(executable)?)?;
    if binding != expected(executable, directory, config, service)? {
        return Err(
            "this executable belongs to another institution; use its own program directory"
                .to_owned(),
        );
    }
    Ok(())
}
pub(crate) fn release(executable: &Path, service: &str) -> Result<(), String> {
    let path = record_path(executable)?;
    if path.exists() && read(&path)?.service == service {
        std::fs::remove_file(path).map_err(|_| "cannot remove installation binding")?;
    }
    Ok(())
}
pub(crate) fn publish_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write as _;
    let parent = controlled_directory(path.parent().ok_or("private program record needs parent")?)?;
    if path.exists() {
        read_private(path, 32_768)?;
    }
    let mut file = tempfile::NamedTempFile::new_in(&parent)
        .map_err(|_| "cannot stage private program record")?;
    file.write_all(bytes)
        .and_then(|()| file.as_file().sync_all())
        .map_err(|_| "cannot sync private program record")?;
    crate::file_owner::operator_private(file.path())?;
    file.persist(path)
        .map_err(|_| "cannot publish private program record")?;
    sync_directory(&parent)
}
pub(crate) fn read_private(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    if crate::backup_archive::regular(path)? > limit {
        return Err("private program record exceeds limit".to_owned());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        let metadata = std::fs::metadata(path).map_err(|_| "private program record unavailable")?;
        if rustix::process::geteuid().is_root()
            && (metadata.uid() != 0 || metadata.mode() & 0o022 != 0)
        {
            return Err("private program record must be administrator controlled".to_owned());
        }
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(limit + 1).read_to_end(&mut bytes))
        .map_err(|_| "private program record unavailable")?;
    if u64::try_from(bytes.len()).map_err(|_| "private program record exceeds limit")? > limit {
        return Err("private program record exceeds limit".to_owned());
    }
    Ok(bytes)
}
pub(crate) fn sync_directory(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        std::fs::File::open(path)
            .and_then(|file| file.sync_all())
            .map_err(|_| "cannot sync program directory")?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

pub(crate) fn lock(executable: &Path) -> Result<std::fs::File, String> {
    use fs4::fs_std::FileExt as _;
    let path = record_path(executable)?.with_extension("lock");
    if path.exists() {
        read_private(&path, 1)?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let file = options
        .open(&path)
        .map_err(|_| "program operation lock unavailable")?;
    file.try_lock_exclusive()
        .map_err(|_| "another program operation is running")?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_second_institution_cannot_rebind_the_installed_binary() {
        let root = tempfile::tempdir().unwrap();
        let program = root.path().join("elzevir");
        std::fs::write(&program, b"fictional executable").unwrap();
        let data = root.path().join("library");
        std::fs::create_dir(&data).unwrap();
        let config = root.path().join("server.toml");
        std::fs::write(&config, b"fictional config").unwrap();
        claim(&program, &data, &config, "elzevir").unwrap();
        verify(&program, &data, &config, "elzevir").unwrap();
        let before = std::fs::read(record_path(&program).unwrap()).unwrap();
        let other = root.path().join("other");
        std::fs::create_dir(&other).unwrap();
        assert!(claim(&program, &other, &config, "elzevir-other").is_err());
        assert_eq!(
            std::fs::read(record_path(&program).unwrap()).unwrap(),
            before
        );
    }
}
