// SPDX-License-Identifier: GPL-3.0-or-later
//! OS access control for native local IPC, with no unsafe in Elzevir.

use interprocess::local_socket::{
    ListenerOptions, Name,
    tokio::{Listener, Stream, prelude::*},
};
use serde::{Deserialize, Serialize};
use std::io;

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "transport", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Endpoint {
    Unix { path: std::path::PathBuf },
    Windows { name: String },
}

impl Endpoint {
    pub(crate) fn valid(&self, instance: &str) -> bool {
        match self {
            Self::Unix { path } => {
                cfg!(unix)
                    && path.is_absolute()
                    && path.file_name().is_some_and(|name| name == "admin.sock")
                    && path
                        .parent()
                        .and_then(std::path::Path::file_name)
                        .is_some_and(|name| name.to_string_lossy().starts_with("elzevir-admin-"))
            }
            Self::Windows { name } => cfg!(windows) && *name == format!("elzevir-admin-{instance}"),
        }
    }

    fn name(&self) -> io::Result<Name<'_>> {
        match self {
            #[cfg(unix)]
            Self::Unix { path } => path
                .as_path()
                .to_fs_name::<interprocess::local_socket::GenericFilePath>(),
            #[cfg(windows)]
            Self::Windows { name } => name
                .as_str()
                .to_ns_name::<interprocess::local_socket::GenericNamespaced>(),
            _ => Err(io::Error::other("unsupported administrative transport")),
        }
    }

    pub(crate) async fn connect(&self) -> io::Result<Stream> {
        #[cfg(unix)]
        if let Self::Unix { path } = self {
            use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
            let parent = std::fs::symlink_metadata(
                path.parent()
                    .ok_or_else(|| io::Error::other("invalid socket directory"))?,
            )?;
            let socket = std::fs::symlink_metadata(path)?;
            let uid = rustix::process::geteuid().as_raw();
            if !parent.is_dir()
                || parent.is_symlink()
                || parent.permissions().mode() & 0o777 != 0o700
                || !socket.file_type().is_socket()
                || socket.permissions().mode() & 0o777 != 0o600
                || socket.uid() != parent.uid()
                || (uid != 0 && uid != parent.uid())
            {
                return Err(io::Error::other(
                    "administrative endpoint permissions refused",
                ));
            }
        }
        Stream::connect(self.name()?).await
    }
}

pub(crate) struct Bound {
    pub(crate) listener: Listener,
    pub(crate) endpoint: Endpoint,
    #[cfg(unix)]
    _directory: tempfile::TempDir,
}

impl Bound {
    pub(crate) fn bind(instance: &str) -> io::Result<Self> {
        #[cfg(unix)]
        let directory = {
            use std::os::unix::fs::PermissionsExt;
            tempfile::Builder::new()
                .prefix("elzevir-admin-")
                .permissions(std::fs::Permissions::from_mode(0o700))
                .tempdir()?
        };
        #[cfg(unix)]
        let endpoint = Endpoint::Unix {
            path: directory.path().join("admin.sock"),
        };
        #[cfg(windows)]
        let endpoint = Endpoint::Windows {
            name: format!("elzevir-admin-{instance}"),
        };
        if !endpoint.valid(instance) {
            return Err(io::Error::other("invalid local administrative endpoint"));
        }
        let options = ListenerOptions::new().name(endpoint.name()?);
        #[cfg(target_os = "linux")]
        let options = {
            use interprocess::os::unix::local_socket::ListenerOptionsExt;
            options.mode(0o600)
        };
        #[cfg(windows)]
        let options = {
            use interprocess::os::windows::{
                local_socket::ListenerOptionsExt, security_descriptor::SecurityDescriptor,
            };
            // Protected DACL: SYSTEM, built-in Administrators and the object owner.
            // The backend's default accept_remote=false rejects SMB clients.
            let sddl = widestring::U16CString::from_str("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;OW)")
                .map_err(|_| io::Error::other("invalid administrative DACL"))?;
            options.security_descriptor(SecurityDescriptor::deserialize(&sddl)?)
        };
        let listener = options.create_tokio()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // The containing directory is already 0700 before bind, including on
            // platforms without pre-bind socket mode support (macOS).
            std::fs::set_permissions(
                directory.path().join("admin.sock"),
                std::fs::Permissions::from_mode(0o600),
            )?;
        }
        Ok(Self {
            listener,
            endpoint,
            #[cfg(unix)]
            _directory: directory,
        })
    }
}
