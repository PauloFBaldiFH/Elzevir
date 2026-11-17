// SPDX-License-Identifier: GPL-3.0-or-later
//! Private restored files retain the institution owner's access after elevated recovery.
use std::path::Path;

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
