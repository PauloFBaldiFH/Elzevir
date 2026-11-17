// SPDX-License-Identifier: GPL-3.0-or-later
//! Explicit private staging permissions, independent of the process umask.
use std::{io, path::Path};

pub(crate) fn builder<'a, 'b>() -> tempfile::Builder<'a, 'b> {
    #[cfg(unix)]
    let mut builder = tempfile::Builder::new();
    #[cfg(not(unix))]
    let builder = tempfile::Builder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    builder
}
pub(crate) fn directory() -> io::Result<tempfile::TempDir> {
    builder().tempdir()
}
pub(crate) fn directory_in(path: impl AsRef<Path>) -> io::Result<tempfile::TempDir> {
    builder().tempdir_in(path)
}
