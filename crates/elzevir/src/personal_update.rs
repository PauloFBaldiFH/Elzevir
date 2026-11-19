// SPDX-License-Identifier: GPL-3.0-or-later
//! Updates user installations using the verified release and backup services.

use std::path::{Path, PathBuf};

pub(crate) fn allowed(executable: &Path, disabled: bool) -> Result<(), String> {
    let parent = executable.parent().ok_or("program directory unavailable")?;
    let package_managed = match std::fs::symlink_metadata(parent.join(".elzevir-no-self-update")) {
        Ok(_) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(_) => return Err("cannot inspect installation update policy".into()),
    };
    let winget = cfg!(windows)
        && executable.components().any(|component| {
            component.as_os_str().eq_ignore_ascii_case("WinGet")
                || component.as_os_str().eq_ignore_ascii_case("WindowsApps")
        });
    if disabled || package_managed || winget {
        return Err(
            "self-update is disabled for this installation; use its package manager or installer"
                .into(),
        );
    }
    Ok(())
}

pub(crate) fn newer(tag: &str, current: &str) -> bool {
    semver::Version::parse(tag.trim_start_matches('v'))
        .ok()
        .zip(semver::Version::parse(current.trim_start_matches('v')).ok())
        .is_some_and(|(release, running)| release.cmp_precedence(&running).is_gt())
}

pub(crate) fn run(
    executable: PathBuf,
    directory: Option<PathBuf>,
    version: String,
    dry_run: bool,
    wait: bool,
) -> Result<(), String> {
    if std::fs::symlink_metadata(crate::installation::record_path(&executable)?).is_ok() {
        return Err(
            "managed installation: use update --config with the service configuration".into(),
        );
    }
    let directory = directory.map_or_else(crate::first_use::data_dir, Ok)?;
    let config = directory.join(crate::first_use::CONFIG);
    if dry_run {
        println!(
            "{}",
            serde_json::json!({"schema":1,"action":"update","target":crate::release::target()?,"version":version,"config":config})
        );
        return Ok(());
    }
    if cfg!(windows) && wait {
        return Err(
            "omit --wait for a personal update on Windows so the executable can close".into(),
        );
    }
    let version = if version == "latest" {
        crate::release::latest(&crate::release::Metadata)?.tag_name
    } else {
        version
    };
    if !newer(&version, env!("CARGO_PKG_VERSION")) {
        println!(
            "elzevir: already up to date ({})",
            env!("CARGO_PKG_VERSION")
        );
        return Ok(());
    }
    if cfg!(windows) {
        let id = crate::host_jobs::launch(
            &directory,
            crate::host_jobs::Task::PersonalUpdate {
                config,
                version,
                executable,
            },
        )?;
        println!("{}", serde_json::json!({"id": id, "state": "queued"}));
    } else {
        execute(&directory, &config, &version, &executable)?;
        println!("elzevir: installed {version}");
    }
    Ok(())
}

pub(crate) fn execute(
    directory: &Path,
    config: &Path,
    version: &str,
    executable: &Path,
) -> Result<(), String> {
    allowed(executable, false)?;
    if !crate::release::valid_version(version) || !newer(version, env!("CARGO_PKG_VERSION")) {
        return Err("self-update requires a newer official release".into());
    }
    apply(
        directory,
        config,
        version,
        executable,
        &crate::release::Http,
    )
}

fn apply(
    directory: &Path,
    config: &Path,
    version: &str,
    executable: &Path,
    downloads: &impl crate::release::Download,
) -> Result<(), String> {
    let _program = crate::installation::lock(executable)?;
    let settings = require_personal(executable, directory, config)?;
    let _data = crate::backup_cli::lock(directory)?;
    let parent = executable.parent().ok_or("program directory unavailable")?;
    let temp = crate::private_temp::directory_in(parent)
        .map_err(|_| "cannot stage release next to the installed program")?;
    let before = crate::release::hash(executable)?;
    let staged = crate::release::stage(downloads, version, temp.path())?;
    let verified = crate::update::verify_executable(&staged, version)?;
    crate::update::backup_stopped(directory, &settings)?;
    let permissions = std::fs::metadata(executable)
        .map_err(|error| error.to_string())?
        .permissions();
    std::fs::set_permissions(&staged, permissions).map_err(|error| error.to_string())?;
    if crate::release::hash(executable)? != before || crate::release::hash(&staged)? != verified {
        return Err("an executable changed during update".into());
    }
    publish(&staged, executable)?;
    crate::installation::sync_directory(parent)
}

fn require_personal(
    executable: &Path,
    directory: &Path,
    config: &Path,
) -> Result<crate::server_config::Settings, String> {
    match std::fs::symlink_metadata(crate::installation::record_path(executable)?) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        _ => return Err("executable has a managed installation binding".into()),
    }
    crate::backup_archive::regular(executable)?;
    crate::backup_archive::regular(&directory.join("elzevir.db"))?;
    let config = match std::fs::symlink_metadata(config) {
        Ok(metadata) if metadata.is_file() => Some(config.to_owned()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        _ => return Err("invalid personal installation configuration".into()),
    };
    let fallback = config.is_none().then(|| directory.to_owned());
    let settings = crate::server_config::ServeArgs::for_status(config, fallback)
        .resolve()
        .map_err(|error| error.to_string())?;
    if settings
        .data_dir
        .canonicalize()
        .map_err(|error| error.to_string())?
        != directory
            .canonicalize()
            .map_err(|error| error.to_string())?
    {
        return Err("update configuration belongs to another institution".into());
    }
    Ok(settings)
}

fn publish(staged: &Path, executable: &Path) -> Result<(), String> {
    let start = std::time::Instant::now();
    loop {
        match std::fs::rename(staged, executable) {
            Ok(()) => return Ok(()),
            // The Windows helper may be ready before the launching image exits.
            Err(error)
                if cfg!(windows)
                    && error.kind() == std::io::ErrorKind::PermissionDenied
                    && start.elapsed() < std::time::Duration::from_secs(10) =>
            {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(error) => return Err(format!("cannot replace the executable atomically: {error}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::newer;

    #[cfg(target_os = "linux")]
    #[test]
    fn a_verified_release_is_published_only_after_an_authenticated_backup() {
        use crate::release::Download;
        use sha2::{Digest as _, Sha256};
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt as _;

        struct Fixture {
            package: Vec<u8>,
            checksum: String,
        }

        impl Download for Fixture {
            fn fetch(&self, url: &str, output: &mut dyn Write, _: u64) -> Result<(), String> {
                if url.ends_with("/SHA256SUMS") {
                    output.write_all(self.checksum.as_bytes())
                } else {
                    output.write_all(&self.package)
                }
                .map_err(|error| error.to_string())
            }
        }

        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("data");
        let digest = {
            let setup = crate::open_store(&data).unwrap();
            let digest = setup.store.digest().unwrap();
            setup.store.shutdown().unwrap();
            digest
        };
        let old = root.path().join("elzevir-installed");
        std::fs::write(&old, b"original").unwrap();
        std::fs::set_permissions(&old, std::fs::Permissions::from_mode(0o700)).unwrap();
        let config = data.join("server.toml");
        let body = b"#!/bin/sh\nprintf 'elzevir 99.0.0\\n'\n";
        let mut tar = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::fast(),
        ));
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(0o700);
        header.set_cksum();
        tar.append_data(&mut header, "elzevir", body.as_slice())
            .unwrap();
        let package = tar.into_inner().unwrap().finish().unwrap();
        let name = format!(
            "elzevir-v99.0.0-{}.tar.gz",
            crate::release::target().unwrap()
        );
        let mut downloads = Fixture {
            package,
            checksum: format!("{}  {name}\n", "0".repeat(64)),
        };
        assert!(super::apply(&data, &config, "v99.0.0", &old, &downloads).is_err());
        assert_eq!(std::fs::read(&old).unwrap(), b"original");
        assert!(!data.join("backups").exists());
        downloads.checksum = format!("{:x}  {name}\n", Sha256::digest(&downloads.package));
        super::apply(&data, &config, "v99.0.0", &old, &downloads).unwrap();
        assert_eq!(std::fs::read(&old).unwrap(), body);
        let backup = std::fs::read_dir(data.join("backups"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert!(crate::backup_archive::extract(&backup, &data.join("backup.key")).is_ok());
        let stored = crate::open_store(&data).unwrap();
        assert_eq!(stored.store.digest().unwrap(), digest);
        stored.store.shutdown().unwrap();
    }

    #[test]
    fn compares_release_precedence_including_prereleases() {
        assert!(newer("v0.1.0", "0.1.0-alpha.1"));
        assert!(newer("v0.1.0-alpha.10", "0.1.0-alpha.2"));
        assert!(!newer("v0.1.0-alpha.2", "0.1.0"));
        assert!(!newer("v0.1.0+other", "0.1.0"));
        assert!(!newer("latest", "0.1.0"));
    }
}
