// SPDX-License-Identifier: GPL-3.0-or-later
//! Private restored files retain the institution owner's access after elevated recovery.
use std::path::Path;

pub(crate) fn private_file(file: &std::fs::File, reference: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
        let owner = std::fs::metadata(reference).map_err(|_| "institution owner unavailable")?;
        let actual = file.metadata().map_err(|_| "private file unavailable")?;
        if actual.uid() != owner.uid() || actual.gid() != owner.gid() {
            rustix::fs::fchown(
                file,
                Some(rustix::process::Uid::from_raw(owner.uid())),
                Some(rustix::process::Gid::from_raw(owner.gid())),
            )
            .map_err(|_| "cannot retain private file owner")?;
        }
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(|_| "cannot protect private file")?;
    }
    #[cfg(not(unix))]
    let _ = (file, reference);
    Ok(())
}

#[cfg(unix)]
pub(crate) fn private_tree(path: &Path, reference: &Path) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    let owner = std::fs::metadata(reference).map_err(|_| "institution owner unavailable")?;
    visit(path, owner.uid(), owner.gid(), 0, &mut 0)
}
#[cfg(unix)]
fn visit(path: &Path, uid: u32, gid: u32, depth: u8, count: &mut usize) -> Result<(), String> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    *count += 1;
    if depth > 8 || *count > 4096 {
        return Err("restored file tree exceeds limit".to_owned());
    }
    let metadata = std::fs::symlink_metadata(path).map_err(|_| "restored file unavailable")?;
    if metadata.is_symlink() || (!metadata.is_dir() && !metadata.is_file()) {
        return Err("restored path must be regular".to_owned());
    }
    if metadata.uid() != uid || metadata.gid() != gid {
        rustix::fs::chown(
            path,
            Some(rustix::process::Uid::from_raw(uid)),
            Some(rustix::process::Gid::from_raw(gid)),
        )
        .map_err(|_| "restore requires permission to retain the institution owner")?;
    }
    std::fs::set_permissions(
        path,
        std::fs::Permissions::from_mode(if metadata.is_dir() { 0o700 } else { 0o600 }),
    )
    .map_err(|_| "cannot protect restored file")?;
    if metadata.is_dir() {
        for entry in std::fs::read_dir(path).map_err(|_| "restored directory unavailable")? {
            visit(
                &entry.map_err(|_| "restored directory unavailable")?.path(),
                uid,
                gid,
                depth + 1,
                count,
            )?;
        }
    }
    Ok(())
}
#[cfg(not(unix))]
pub(crate) fn private_tree(_path: &Path, _reference: &Path) -> Result<(), String> {
    // Windows files inherit the protected installation-directory ACL from A9.
    Ok(())
}

/// Retains an existing configuration's group-read grant for a separate service account.
pub(crate) fn configuration(path: &Path, reference: &Path) -> Result<(), String> {
    private_tree(path, reference)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(reference)
            .map_err(|_| "configuration owner unavailable")?
            .permissions()
            .mode();
        std::fs::set_permissions(
            path,
            std::fs::Permissions::from_mode(0o600 | (mode & 0o040)),
        )
        .map_err(|_| "cannot retain service configuration access")?;
    }
    Ok(())
}
/// Protects a management credential from the institution's service identity.
pub(crate) fn operator_private(path: &Path) -> Result<(), String> {
    let metadata =
        std::fs::symlink_metadata(path).map_err(|_| "operator-private path unavailable")?;
    if metadata.is_symlink() || (!metadata.is_file() && !metadata.is_dir()) {
        return Err("operator-private path must be regular".to_owned());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            path,
            std::fs::Permissions::from_mode(if metadata.is_dir() { 0o700 } else { 0o600 }),
        )
        .map_err(|_| "cannot protect operator credential")?;
    }
    #[cfg(windows)]
    {
        use crate::process_runner::{Native, Request, Runner as _};
        let program = std::path::PathBuf::from(
            std::env::var_os("SystemRoot").ok_or("Windows system directory unavailable")?,
        )
        .join("System32")
        .join("icacls.exe");
        let output = Native.run(Request {
            environment: None,
            program,
            args: vec![
                path.as_os_str().to_owned(),
                "/inheritance:r".into(),
                "/grant:r".into(),
                format!(
                    "*S-1-5-18:{}",
                    if metadata.is_dir() { "(OI)(CI)F" } else { "F" }
                )
                .into(),
                format!(
                    "*S-1-5-32-544:{}",
                    if metadata.is_dir() { "(OI)(CI)F" } else { "F" }
                )
                .into(),
                "/setowner".into(),
                "*S-1-5-32-544".into(),
            ],
            input: None,
            progress: None,
            timeout: std::time::Duration::from_secs(10),
            limit: 4096,
        })?;
        if !output.success {
            return Err("cannot protect operator credential ACL".to_owned());
        }
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn restored_root_owned_configs_keep_their_service_group_read_grant() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let original = directory.path().join("original");
        let staged = directory.path().join("staged");
        std::fs::write(&original, "config").unwrap();
        std::fs::write(&staged, "replacement").unwrap();
        std::fs::set_permissions(&original, std::fs::Permissions::from_mode(0o640)).unwrap();
        configuration(&staged, &original).unwrap();
        assert_eq!(
            std::fs::metadata(staged).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }
}
