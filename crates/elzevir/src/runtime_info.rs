// SPDX-License-Identifier: GPL-3.0-or-later
//! Local discovery metadata, owned by the process holding the data directory lock.

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

pub(crate) const NAME: &str = "elzevir-runtime.json";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeInfo {
    pub(crate) schema: u8,
    pub(crate) instance: String,
    pub(crate) pid: u32,
    pub(crate) address: SocketAddr,
}

pub(crate) fn read(path: &Path) -> Result<RuntimeInfo, String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| "runtime metadata unavailable")?;
    if !metadata.is_file() || metadata.is_symlink() {
        return Err("runtime metadata must be a regular file".to_owned());
    }
    let mut source = String::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(4097).read_to_string(&mut source))
        .map_err(|_| "cannot read runtime metadata")?;
    if source.len() > 4096 {
        return Err("runtime metadata too large".to_owned());
    }
    let info: RuntimeInfo =
        serde_json::from_str(&source).map_err(|_| "invalid runtime metadata")?;
    info.validate()?;
    Ok(info)
}

impl RuntimeInfo {
    fn validate(&self) -> Result<(), String> {
        if self.schema != 1
            || self.pid == 0
            || !self.address.ip().is_loopback()
            || self.address.port() == 0
            || self.instance.len() != 32
            || !self.instance.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("incompatible runtime metadata".to_owned());
        }
        Ok(())
    }
}

pub(crate) struct RuntimeFile {
    path: PathBuf,
    instance: String,
}

impl RuntimeFile {
    pub(crate) fn publish(
        data_dir: &Path,
        instance: &str,
        address: SocketAddr,
    ) -> Result<Self, String> {
        let path = data_dir.join(NAME);
        match std::fs::symlink_metadata(&path) {
            Ok(_) => {
                read(&path)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("cannot inspect runtime metadata".to_owned()),
        }
        let info = RuntimeInfo {
            schema: 1,
            instance: instance.to_owned(),
            pid: std::process::id(),
            address,
        };
        info.validate()?;
        let bytes = serde_json::to_vec(&info).map_err(|_| "cannot encode runtime metadata")?;
        if bytes.len() > 4096 {
            return Err("runtime metadata too large".to_owned());
        }
        let mut file = tempfile::NamedTempFile::new_in(data_dir)
            .map_err(|_| "cannot prepare runtime metadata")?;
        file.write_all(&bytes)
            .and_then(|()| file.as_file().sync_all())
            .map_err(|_| "cannot write runtime metadata")?;
        file.persist(&path)
            .map_err(|_| "cannot publish runtime metadata")?;
        Ok(Self {
            path,
            instance: instance.to_owned(),
        })
    }
}

impl Drop for RuntimeFile {
    fn drop(&mut self) {
        if read(&self.path).is_ok_and(|info| info.instance == self.instance) {
            // Metadata is advisory. After an unclean exit the lock still prevents false readiness.
            if std::fs::remove_file(&self.path).is_err() {
                eprintln!("elzevir: cannot remove owned runtime metadata");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_content_is_not_overwritten_and_owned_metadata_is_cleaned() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(NAME);
        std::fs::write(&path, "user content").unwrap();
        let instance = "0".repeat(32);
        let address = "127.0.0.1:12345".parse().unwrap();
        assert!(RuntimeFile::publish(directory.path(), &instance, address).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "user content");
        std::fs::remove_file(&path).unwrap();
        let owned = RuntimeFile::publish(directory.path(), &instance, address).unwrap();
        assert_eq!(read(&path).unwrap().instance, instance);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        drop(owned);
        assert!(!path.exists());
    }

    #[test]
    fn cleanup_does_not_remove_replacement_metadata() {
        let directory = tempfile::tempdir().unwrap();
        let address = "127.0.0.1:12345".parse().unwrap();
        let previous = RuntimeFile::publish(directory.path(), &"0".repeat(32), address).unwrap();
        let current = RuntimeFile::publish(directory.path(), &"1".repeat(32), address).unwrap();
        drop(previous);
        assert_eq!(
            read(&directory.path().join(NAME)).unwrap().instance,
            "1".repeat(32)
        );
        drop(current);
    }

    #[test]
    fn metadata_is_bounded_and_rejects_invalid_protocol_fields_without_modification() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(NAME);
        let valid = serde_json::json!({"schema":1,"instance":"0".repeat(32),"pid":1,"address":"127.0.0.1:12345"});
        let mut invalid = vec![vec![0xff], vec![b'x'; 4097]];
        for (field, value) in [
            ("schema", serde_json::json!(2)),
            ("pid", serde_json::json!(0)),
            ("instance", serde_json::json!("invalid")),
            ("instance", serde_json::json!("z".repeat(32))),
            ("address", serde_json::json!("203.0.113.1:80")),
            ("address", serde_json::json!("127.0.0.1:0")),
            ("extra", serde_json::json!("unknown")),
        ] {
            let mut document = valid.clone();
            document[field] = value;
            invalid.push(serde_json::to_vec(&document).unwrap());
        }
        for document in invalid {
            std::fs::write(&path, &document).unwrap();
            assert!(read(&path).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), document);
        }
    }
}
