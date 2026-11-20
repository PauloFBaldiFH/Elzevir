// SPDX-License-Identifier: GPL-3.0-or-later
//! First installation only: preserve existing paths and publish config last.

use elzevir_app::commands::{StaffCommand, WriteCommand, WriteOutput};
use elzevir_app::staff::{Permission, hash_password};
use elzevir_panel::Messages;
use std::io::{self, BufRead, IsTerminal, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

#[derive(clap::Args)]
pub(crate) struct InitArgs {
    /// Institution name, saved in the configuration.
    #[arg(long)]
    name: Option<String>,
    /// New configuration file; its parent directory must already exist.
    #[arg(long)]
    config: Option<PathBuf>,
    /// New data directory; its parent must already exist.
    #[arg(long)]
    data_dir: Option<PathBuf>,
    /// First administrator's login.
    #[arg(long, default_value = "admin")]
    pub(crate) login: String,
    /// Server IP and port.
    #[arg(long, default_value_t = crate::server_config::default_listen())]
    pub(crate) listen: SocketAddr,
    /// Allow plain-HTTP cookies for local development.
    #[arg(long, num_args = 0..=1, require_equals = true, default_missing_value = "true")]
    insecure_cookies: Option<bool>,
    #[arg(long, value_parser = ["en", "pt-BR"])]
    lang: Option<String>,
    #[arg(long, value_enum)]
    profile: Option<Profile>,
    /// Read a password line from redirected stdin instead of prompting.
    #[arg(long, requires = "name")]
    password_stdin: bool,
    /// Reserve the listen address during initialization, for managed installation.
    #[arg(long)]
    check_port: bool,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum Profile {
    Public,
    School,
    University,
}

impl InitArgs {
    pub(crate) fn first_use(
        lang: Option<String>,
        listen: Option<SocketAddr>,
        insecure: Option<bool>,
    ) -> Self {
        let listen = listen.unwrap_or_else(crate::server_config::default_listen);
        Self {
            name: None,
            config: None,
            data_dir: None,
            login: "admin".into(),
            listen,
            insecure_cookies: insecure,
            lang,
            profile: None,
            password_stdin: false,
            check_port: true,
        }
    }
}

fn prompt(messages: &Messages, key: &str) -> Result<String, String> {
    let mut output = io::stdout().lock();
    write!(output, "{}  ", messages.get(key))
        .and_then(|()| output.flush())
        .map_err(|error| error.to_string())?;
    let mut answer = String::new();
    let count = io::Read::take(io::stdin().lock(), 1025)
        .read_line(&mut answer)
        .map_err(|error| error.to_string())?;
    if count == 0 || count > 1024 {
        return Err(messages.get("setup-cancelled"));
    }
    Ok(answer.trim().to_owned())
}

pub(crate) fn confirm_setup(messages: &Messages) -> Result<bool, String> {
    Ok(matches!(
        prompt(messages, "setup-offer")?
            .to_ascii_lowercase()
            .as_str(),
        "" | "y" | "yes" | "s" | "sim"
    ))
}

pub(crate) fn run(args: &InitArgs) -> Result<(), String> {
    let _listener = if args.check_port {
        Some(
            std::net::TcpListener::bind(args.listen)
                .map_err(|_| "listen address is unavailable; no institution was created")?,
        )
    } else {
        None
    };
    let language = crate::first_use::language(args.lang.as_deref());
    let messages = Messages::new(language).map_err(|error| error.to_string())?;
    let Paths {
        config,
        data_dir,
        using_default,
        restore_empty,
    } = paths(args)?;
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
    let (name, password, profile) = initial_choices(args, &messages, language)?;
    let insecure = args
        .insecure_cookies
        .unwrap_or_else(|| using_default && args.listen.ip().is_loopback());
    let source = crate::server_config::render_init(&name, &data_dir, args.listen, insecure)
        .map_err(|error| error.to_string())?;
    let parent = data_dir.parent().ok_or("data directory needs a parent")?;
    let existing = parent
        .ancestors()
        .find(|path| path.is_dir())
        .ok_or("data directory needs an existing ancestor")?;
    crate::local_disk::check(existing)?;
    if password.chars().count() < 8 || password.len() > 1024 {
        return Err("password must have at least 8 characters and at most 1024 bytes".to_owned());
    }
    let hash = hash_password(&password).map_err(|error| error.to_string())?;
    if using_default {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        crate::local_disk::check(parent)?;
    }
    let mut builder = crate::private_temp::builder();
    // No random suffix: create exactly the requested directory, exclusively.
    builder
        .prefix(data_dir.file_name().ok_or("invalid data directory")?)
        .rand_bytes(0);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    // Older installers created an empty directory. Only remove it if still empty;
    // a concurrent writer that has added any file wins and keeps its data.
    if restore_empty {
        std::fs::remove_dir(&data_dir).map_err(|error| error.to_string())?;
    }
    let result = builder
        .tempdir_in(parent)
        .map_err(|error| format!("cannot create new data directory: {error}"))
        .and_then(|directory| {
            finish(
                directory,
                &config,
                &source,
                &args.login,
                hash,
                profile,
                &messages,
            )
        });
    if restore_empty && result.is_err() {
        match std::fs::create_dir(&data_dir) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(format!("cannot restore previous empty directory: {error}")),
        }
    }
    result?;
    println!("elzevir: initialized {}", config.display());
    Ok(())
}

struct Paths {
    config: PathBuf,
    data_dir: PathBuf,
    using_default: bool,
    restore_empty: bool,
}

fn paths(args: &InitArgs) -> Result<Paths, String> {
    let using_default = args.data_dir.is_none();
    let data_dir = args
        .data_dir
        .clone()
        .map_or_else(crate::first_use::data_dir, Ok)?;
    let config_in_data = using_default && args.config.is_none();
    let config = if config_in_data {
        data_dir.join(crate::first_use::CONFIG)
    } else {
        new_path(
            args.config
                .as_deref()
                .unwrap_or_else(|| Path::new("elzevir.toml")),
        )?
    };
    let data_dir = if using_default {
        data_dir
    } else {
        new_path(&data_dir)?
    };
    let restore_empty = if using_default {
        match std::fs::symlink_metadata(&data_dir) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => false,
            Ok(metadata)
                if metadata.is_dir()
                    && std::fs::read_dir(&data_dir)
                        .map_err(|error| error.to_string())?
                        .next()
                        .is_none() =>
            {
                true
            }
            _ => {
                return Err(format!(
                    "destination already exists: {}",
                    data_dir.display()
                ));
            }
        }
    } else {
        false
    };
    Ok(Paths {
        config,
        data_dir,
        using_default,
        restore_empty,
    })
}

fn initial_choices(
    args: &InitArgs,
    messages: &Messages,
    language: &str,
) -> Result<(String, zeroize::Zeroizing<String>, Option<Profile>), String> {
    if let Some(name) = &args.name {
        // Reject invalid explicit names before reading a secret.
        crate::server_config::validate_name(name).map_err(|error| error.to_string())?;
        let secret = zeroize::Zeroizing::new(password(args.password_stdin, messages)?);
        return Ok((name.clone(), secret, args.profile));
    }
    if !crate::first_use::interactive() {
        return Err("use --name for noninteractive initialization".into());
    }
    let selected = elzevir_panel::setup(language)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| messages.get("setup-cancelled"))?;
    let profile = match selected.profile {
        elzevir_panel::SetupProfile::Empty => None,
        elzevir_panel::SetupProfile::Public => Some(Profile::Public),
        elzevir_panel::SetupProfile::School => Some(Profile::School),
        elzevir_panel::SetupProfile::University => Some(Profile::University),
    };
    Ok((selected.name, selected.password, args.profile.or(profile)))
}

fn finish(
    directory: tempfile::TempDir,
    config: &Path,
    source: &str,
    login: &str,
    hash: String,
    profile: Option<Profile>,
    messages: &Messages,
) -> Result<(), String> {
    match populate(
        directory.path(),
        config,
        source,
        login,
        hash,
        profile,
        messages,
    ) {
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

fn password(from_stdin: bool, messages: &Messages) -> Result<String, String> {
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
    let first = rpassword::prompt_password(messages.get("setup-password"))
        .map_err(|_| "cannot read password from terminal".to_owned())?;
    let first = zeroize::Zeroizing::new(first);
    let confirmation = rpassword::prompt_password(messages.get("setup-repeat"))
        .map_err(|_| "cannot read password confirmation".to_owned())?;
    let confirmation = zeroize::Zeroizing::new(confirmation);
    if first.as_str() != confirmation.as_str() {
        return Err("password confirmation does not match".to_owned());
    }
    Ok(first.to_string())
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
    profile: Option<Profile>,
    messages: &Messages,
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
                Permission::Circulation,
                Permission::Patrons,
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
    if let Some(profile) = profile {
        use elzevir_app::catalog::CatalogCommand;
        let key = match profile {
            Profile::Public => "setup-public",
            Profile::School => "setup-school",
            Profile::University => "setup-university",
        };
        for command in [
            CatalogCommand::CreateBranch {
                name: messages.get(key),
            },
            CatalogCommand::CreatePolicy {
                name: messages.get("setup-policy"),
            },
        ] {
            data.store
                .submit(WriteCommand::Catalog(command))
                .map_err(|error| error.to_string())?
                .wait()
                .map_err(|error| error.to_string())?;
        }
    }
    drop(crate::open_index(data_dir)?);
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
        assert!(
            finish(
                data,
                &config,
                "replacement",
                "admin",
                hash,
                None,
                &Messages::new("en").unwrap()
            )
            .is_err()
        );
        assert_eq!(std::fs::read_to_string(&config).unwrap(), "preserve");
        assert!(!path.exists());
    }
}
