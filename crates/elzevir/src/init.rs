// SPDX-License-Identifier: GPL-3.0-or-later
//! First installation only: preserve existing paths and publish config last.

use elzevir_app::commands::{StaffCommand, WriteCommand, WriteOutput};
use elzevir_app::staff::{Permission, hash_password};
use std::io::{self, BufRead, IsTerminal, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

#[derive(clap::Args)]
pub(crate) struct InitArgs {
    /// Institution name, saved in the configuration.
    #[arg(long)]
    name: String,
    /// New configuration file; its parent directory must already exist.
    #[arg(long, default_value = "elzevir.toml")]
    config: PathBuf,
    /// New data directory; its parent must already exist.
    #[arg(long, default_value = "data-local")]
    data_dir: PathBuf,
    /// First administrator's login.
    #[arg(long, default_value = "admin")]
    login: String,
    /// Server IP and port.
    #[arg(long, default_value = "127.0.0.1:8080")]
    listen: SocketAddr,
    /// Allow plain-HTTP cookies for local development.
    #[arg(long)]
    insecure_cookies: bool,
    /// Read a password line from redirected stdin instead of prompting.
    #[arg(long)]
    password_stdin: bool,
}

pub(crate) fn run(args: &InitArgs) -> Result<(), String> {
    let config = new_path(&args.config)?;
    let data_dir = new_path(&args.data_dir)?;
    if config == data_dir {
        return Err("configuration and data directory must have different paths".to_owned());
    }
    if args.login.is_empty()
        || args.login.len() > 64
        || !args
            .login
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
    {
        return Err(
            "login must contain 1 to 64 ASCII letters, digits, dots, underscores or hyphens"
                .to_owned(),
        );
    }
    let source = crate::server_config::render_init(
        &args.name,
        &data_dir,
        args.listen,
        args.insecure_cookies,
    )
    .map_err(|error| error.to_string())?;
    let parent = data_dir.parent().ok_or("data directory needs a parent")?;
    crate::local_disk::check(parent)?;
    let password = password(args.password_stdin)?;
    if password.chars().count() < 8 || password.len() > 1024 {
        return Err("password must have at least 8 characters and at most 1024 bytes".to_owned());
    }
    let hash = hash_password(&password).map_err(|error| error.to_string())?;
    let mut builder = tempfile::Builder::new();
    // No random suffix: create exactly the requested directory, exclusively.
    builder
        .prefix(data_dir.file_name().ok_or("invalid data directory")?)
        .rand_bytes(0);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    let directory = builder
        .tempdir_in(parent)
        .map_err(|error| format!("cannot create new data directory: {error}"))?;
    finish(directory, &config, &source, &args.login, hash)?;
    println!("elzevir: initialized {}", config.display());
    Ok(())
}

fn finish(
    directory: tempfile::TempDir,
    config: &Path,
    source: &str,
    login: &str,
    hash: String,
) -> Result<(), String> {
    match populate(directory.path(), config, source, login, hash) {
        Ok(()) => {
            drop(directory.keep());
        }
        Err(error) => {
            directory.close().map_err(|cleanup| {
                format!("{error}; cannot remove newly created data directory: {cleanup}")
            })?;
            return Err(error);
        }
    }
    Ok(())
}

fn new_path(path: &Path) -> Result<PathBuf, String> {
    let name = path
        .file_name()
        .ok_or("destination must name a new file or directory")?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let absolute = parent
        .canonicalize()
        .map_err(|error| format!("destination parent must exist: {error}"))?
        .join(name);
    match std::fs::symlink_metadata(&absolute) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(absolute),
        Ok(_) => Err(format!(
            "destination already exists: {}",
            absolute.display()
        )),
        Err(error) => Err(format!("cannot inspect destination: {error}")),
    }
}

fn password(from_stdin: bool) -> Result<String, String> {
    if from_stdin {
        if io::stdin().is_terminal() {
            return Err("omit --password-stdin for hidden interactive input".to_owned());
        }
        return read_password_line(io::stdin().lock());
    }
    match std::env::var("ELZEVIR_STAFF_PASSWORD") {
        Ok(value) => return Ok(value),
        Err(std::env::VarError::NotUnicode(_)) => {
            return Err("password environment variable must be UTF-8".to_owned());
        }
        Err(std::env::VarError::NotPresent) => {}
    }
    if !io::stdin().is_terminal() {
        return Err(
            "use --password-stdin or ELZEVIR_STAFF_PASSWORD for noninteractive initialization"
                .to_owned(),
        );
    }
    let first = rpassword::prompt_password("Administrator password: ")
        .map_err(|_| "cannot read password from terminal".to_owned())?;
    let confirmation = rpassword::prompt_password("Confirm password: ")
        .map_err(|_| "cannot read password confirmation".to_owned())?;
    if first != confirmation {
        return Err("password confirmation does not match".to_owned());
    }
    Ok(first)
}

fn read_password_line(reader: impl BufRead) -> Result<String, String> {
    let mut line = String::new();
    let count = reader
        .take(1027)
        .read_line(&mut line)
        .map_err(|_| "cannot read password from stdin".to_owned())?;
    if count == 1027 {
        return Err("password exceeds 1024 bytes".to_owned());
    }
    let without_lf = line.strip_suffix('\n').unwrap_or(&line);
    Ok(without_lf
        .strip_suffix('\r')
        .unwrap_or(without_lf)
        .to_owned())
}

fn populate(
    data_dir: &Path,
    config: &Path,
    source: &str,
    login: &str,
    hash: String,
) -> Result<(), String> {
    // On errors, DataDir drops the store before releasing the single-writer lock.
    let data = crate::open_store(data_dir)?;
    let outcome = data
        .store
        .submit(WriteCommand::Staff(StaffCommand::Create {
            login: login.to_owned(),
            password_hash: hash,
            permissions: [
                Permission::StaffInterface,
                Permission::Administration,
                Permission::Cataloguing,
            ]
            .into_iter()
            .collect(),
        }))
        .map_err(|error| error.to_string())?
        .wait()
        .map_err(|error| error.to_string())?;
    if !matches!(outcome, WriteOutput::StaffCreated(_)) {
        return Err("unexpected answer while creating administrator".to_owned());
    }
    // Finish the writer and checkpoint the database before publishing its config.
    data.store.shutdown().map_err(|error| error.to_string())?;
    let mut file =
        tempfile::NamedTempFile::new_in(config.parent().ok_or("configuration needs a parent")?)
            .map_err(|error| error.to_string())?;
    file.write_all(source.as_bytes())
        .map_err(|error| error.to_string())?;
    file.as_file()
        .sync_all()
        .map_err(|error| error.to_string())?;
    file.persist_noclobber(config).map_err(|error| {
        format!(
            "cannot publish configuration without overwriting: {}",
            error.error
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirected_password_is_bounded_without_truncation() {
        let password = "x".repeat(1024);
        assert_eq!(
            read_password_line(format!("{password}\r\n").as_bytes()).unwrap(),
            password
        );
        assert!(read_password_line("x".repeat(1027).as_bytes()).is_err());
        assert!(read_password_line(format!("valid-pass{}", "\r".repeat(2000)).as_bytes()).is_err());
    }

    #[test]
    fn config_created_after_preflight_is_not_overwritten() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("elzevir.toml");
        // The caller already owns the new directory; another init won config publication.
        let data = tempfile::tempdir_in(root.path()).unwrap();
        std::fs::write(&config, "preserve").unwrap();
        let hash = hash_password("fictional-password").unwrap();
        let path = data.path().to_owned();
        assert!(finish(data, &config, "replacement", "admin", hash).is_err());
        assert_eq!(std::fs::read_to_string(&config).unwrap(), "preserve");
        assert!(!path.exists());
    }
}
