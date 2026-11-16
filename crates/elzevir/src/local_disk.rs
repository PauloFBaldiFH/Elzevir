// SPDX-License-Identifier: GPL-3.0-or-later
//! Conservative local-volume checks before creating SQLite data.

use std::path::Path;

#[cfg(target_os = "linux")]
pub(crate) fn check(parent: &Path) -> Result<(), String> {
    let volume = rustix::fs::statfs(parent)
        .map_err(|error| format!("cannot inspect data volume: {error}"))?;
    if local_linux(u64::try_from(volume.f_type).unwrap_or_default()) {
        Ok(())
    } else {
        Err("data directory must be on a recognized local filesystem; network and unknown volumes are refused".to_owned())
    }
}

#[cfg(target_os = "linux")]
fn local_linux(kind: u64) -> bool {
    // Local filesystem magic values from Linux uapi/linux/magic.h. Unknown/FUSE
    // filesystems are refused because they can conceal network storage.
    matches!(
        kind,
        0xef53 | 0x5846_5342 | 0x9123_683e | 0xf2f5_2010 | // ext, XFS, btrfs, F2FS
        0x2fc1_2fc1 | 0xca45_1a4e | // ZFS, bcachefs
        0x0102_1994 | 0x8584_58f6 | 0x794c_7630 | // tmpfs, ramfs, overlay
        0x4d44 | 0x2011_bab0 | 0x5346_544e // FAT, exFAT, NTFS
    )
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    #[test]
    fn remote_and_unknown_filesystems_are_refused() {
        // NFS, CIFS, 9P, FUSE, unknown. No mounts or network services needed.
        for kind in [0x6969, 0xff53_4d42, 0x0102_1997, 0x6573_5546, 0] {
            assert!(!super::local_linux(kind));
        }
        assert!(super::local_linux(0xef53));
        assert!(super::local_linux(0x794c_7630));
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn check(parent: &Path) -> Result<(), String> {
    let volume = rustix::fs::statfs(parent)
        .map_err(|error| format!("cannot inspect data volume: {error}"))?;
    // Darwin sys/mount.h: inspect the actual mount, including mounts nested
    // below a local volume; a list containing only local disks would miss them.
    const MNT_LOCAL: u32 = 0x1000;
    if volume.f_flags & MNT_LOCAL != 0 {
        Ok(())
    } else {
        Err("data directory must be on a local volume".to_owned())
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn check(parent: &Path) -> Result<(), String> {
    let disks = sysinfo::Disks::new_with_refreshed_list()
        .map_err(|error| format!("cannot inspect data volume: {error}"))?;
    // On Windows sysinfo only enumerates fixed/removable local volumes. Resolve
    // mount points before comparing, including verbatim Windows path prefixes.
    // Unresolvable mounts are excluded; absence of a match refuses the target.
    let disk = disks
        .iter()
        .filter_map(|disk| {
            let mount = disk.mount_point().canonicalize().ok()?;
            parent.starts_with(&mount).then_some((mount, disk))
        })
        .max_by_key(|(mount, _)| mount.components().count());
    let local = disk.is_some_and(|(_, disk)| {
        matches!(
            disk.file_system()
                .to_string_lossy()
                .to_ascii_lowercase()
                .as_str(),
            "ntfs" | "refs" | "fat" | "fat32" | "exfat"
        )
    });
    if local {
        Ok(())
    } else {
        Err("data directory must be on a recognized local volume; network and unknown volumes are refused".to_owned())
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
pub(crate) fn check(_parent: &Path) -> Result<(), String> {
    Err("local-volume inspection is unavailable on this platform".to_owned())
}
