// SPDX-License-Identifier: GPL-3.0-or-later
//! Explicit backup recovery at the command-line boundary.

use crate::backup_archive as archive;
use age::secrecy::ExposeSecret as _;
use std::{
    io::Read,
    path::{Path, PathBuf},
};

#[derive(clap::Args)]
pub(crate) struct BackupArgs {
    #[command(subcommand)]
    action: Action,
}

#[derive(clap::Subcommand)]
enum Action {
    /// Start an encrypted backup; uses the running server when available.
    Now(Target),
    /// Authenticate every archive entry and check SQLite integrity.
    Verify {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        recovery_key: PathBuf,
    },
    /// Export the institution recovery key to a new private file.
    Key {
        #[arg(long)]
        data_dir: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Restore a stopped institution, preserving a safety copy and rebuilding search.
    Restore {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        recovery_key: PathBuf,
        #[arg(long)]
        data_dir: PathBuf,
        /// Existing server configuration to replace; otherwise `DATA_DIR/server.toml`.
        #[arg(long)]
        config: Option<PathBuf>,
        /// Required acknowledgement of replacement of the stopped institution.
        #[arg(long)]
        confirm_restore: bool,
    },
    /// Undo a restore interrupted by a process or machine crash.
    RecoverRestore {
        #[arg(long)]
        data_dir: PathBuf,
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long)]
        confirm_restore: bool,
    },
    /// Copy a completed encrypted artifact to a new destination.
    Export {
        #[arg(long)]
        data_dir: PathBuf,
        #[arg(long)]
        id: String,
        #[arg(long)]
        output: PathBuf,
    },
}

#[derive(clap::Args)]
struct Target {
    #[arg(long, required_unless_present = "data_dir")]
    config: Option<PathBuf>,
    #[arg(long)]
    data_dir: Option<PathBuf>,
}

pub(crate) fn lock(directory: &Path) -> Result<std::fs::File, String> {
    archive::regular(&directory.join("elzevir.db"))?;
    lock_existing(directory)
}

pub(crate) fn lock_existing(directory: &Path) -> Result<std::fs::File, String> {
    let path = directory.join("elzevir.lock");
    archive::regular(&path)?;
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|_| "institution lock unavailable")?;
    file.try_lock()
        .map_err(|_| "stop the institution before this operation")?;
    Ok(file)
}

pub(crate) fn artifact(directory: &Path, id: &str) -> Result<PathBuf, String> {
    if id.len() != 32 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("invalid backup identity".to_owned());
    }
    Ok(directory.join("backups").join(format!("elzevir-{id}.age")))
}

pub(crate) fn make_directory(directory: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(directory) {
        Ok(metadata) if metadata.is_dir() && !metadata.is_symlink() => Ok(()),
        Ok(_) => Err("operation directory must not be a file or link".to_owned()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder
                .create(directory)
                .map_err(|_| "cannot prepare operation directory".to_owned())
        }
        Err(_) => Err("operation directory unavailable".to_owned()),
    }
}

pub(crate) fn run(args: BackupArgs) -> Result<(), String> {
    match args.action {
        Action::Now(target) => {
            let settings =
                crate::server_config::ServeArgs::for_status(target.config, target.data_dir)
                    .resolve()
                    .map_err(|error| error.to_string())?;
            let directory = &settings.data_dir;
            if crate::status::inspect(directory).state == "stopped" {
                let _lock = lock(directory)?;
                make_directory(&directory.join("backups"))?;
                let output = artifact(directory, &archive::identifier()?)?;
                let config = crate::server_config::snapshot(&settings)
                    .map_err(|_| "cannot snapshot configuration")?;
                let manifest = archive::create(directory, &config, &output, None)?;
                println!(
                    "{}",
                    serde_json::json!({"id":manifest.id,"path":output,"state":"completed"})
                );
            } else {
                let info = crate::runtime_info::read(&directory.join(crate::runtime_info::NAME))?;
                let job = crate::admin::start_backup(&info)?;
                println!(
                    "{}",
                    serde_json::to_string(&job).map_err(|_| "cannot encode backup job")?
                );
            }
        }
        Action::RecoverRestore {
            data_dir,
            config,
            confirm_restore,
        } => {
            if !confirm_restore {
                return Err("recovery requires --confirm-restore".to_owned());
            }
            crate::restore_journal::recover(&data_dir, config.as_deref())?;
        }
        Action::Verify {
            input,
            recovery_key,
        } => {
            let verified = archive::extract(&input, &recovery_key)?;
            println!(
                "{}",
                serde_json::to_string(&verified.manifest).map_err(|_| "cannot encode manifest")?
            );
        }
        Action::Key { data_dir, output } => {
            let identity = archive::key(&data_dir.join("backup.key"), false)?;
            archive::write_new(&output, identity.to_string().expose_secret().as_bytes())?;
        }
        Action::Export {
            data_dir,
            id,
            output,
        } => {
            let source = artifact(&data_dir, &id)?;
            copy_new(&source, &output)?;
        }
        Action::Restore {
            input,
            recovery_key,
            data_dir,
            config,
            confirm_restore,
        } => {
            if !confirm_restore {
                return Err(
                    "restore requires --confirm-restore and a stopped institution".to_owned(),
                );
            }
            restore(&input, &recovery_key, &data_dir, config.as_deref())?;
        }
    }
    Ok(())
}

pub(crate) fn copy_new(source: &Path, output: &Path) -> Result<(), String> {
    archive::regular(source)?;
    let parent = output.parent().ok_or("destination needs a parent")?;
    let mut staged =
        tempfile::NamedTempFile::new_in(parent).map_err(|_| "cannot prepare export")?;
    let mut input = std::fs::File::open(source).map_err(|_| "artifact unavailable")?;
    std::io::copy(&mut input, staged.as_file_mut()).map_err(|_| "cannot copy artifact")?;
    staged
        .as_file()
        .sync_all()
        .map_err(|_| "cannot sync export")?;
    staged
        .persist_noclobber(output)
        .map_err(|_| "destination must be new")?;
    Ok(())
}

fn text(path: &Path) -> Result<String, String> {
    if archive::regular(path)? > 1_048_576 {
        return Err("configuration exceeds restore limit".to_owned());
    }
    let mut source = String::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(1_048_577).read_to_string(&mut source))
        .map_err(|_| "configuration unavailable")?;
    Ok(source)
}

/// Holds the original institution lock through all swaps and rollback paths.
pub(crate) fn restore(
    input: &Path,
    key: &Path,
    directory: &Path,
    config: Option<&Path>,
) -> Result<(), String> {
    restore_with(input, key, directory, config, rebuild)
}

fn restore_with(
    input: &Path,
    key: &Path,
    directory: &Path,
    config: Option<&Path>,
    rebuild_action: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<(), String> {
    restore_inner(input, key, directory, config, rebuild_action, true)
}

pub(crate) fn restore_rollback(
    input: &Path,
    key: &Path,
    directory: &Path,
    config: Option<&Path>,
) -> Result<(), String> {
    restore_inner(input, key, directory, config, rebuild, false)
}

fn restore_inner(
    input: &Path,
    key: &Path,
    directory: &Path,
    config: Option<&Path>,
    rebuild_action: impl FnOnce(&Path) -> Result<(), String>,
    safety_snapshot: bool,
) -> Result<(), String> {
    let directory = directory
        .canonicalize()
        .map_err(|_| "restore directory must already exist")?;
    let _lock = lock(&directory)?;
    let restored = archive::extract(input, key)?;
    let config = config.map_or_else(|| directory.join("server.toml"), Path::to_owned);
    let source = crate::server_config::relocate(
        &text(&restored.directory.path().join("server.toml"))?,
        &directory,
    )?;
    // Validate auxiliary configuration before any live file is moved.
    validate_auxiliary(restored.directory.path(), safety_snapshot)?;
    let safety = crate::restore_journal::create_safety(&directory)?;
    // A consistent independent image is retained even if rebuilding the replacement fails.
    if safety_snapshot {
        elzevir_store::snapshot_offline(
            &directory.join("elzevir.db"),
            &safety.join("database.sqlite"),
        )
        .map_err(|_| "cannot create restore safety image")?;
    }
    let stage = tempfile::tempdir_in(crate::restore_journal::trusted_parent(&directory)?)
        .map_err(|_| "cannot stage restore")?;
    crate::file_owner::operator_private(stage.path())?;
    let mut replacements = Vec::new();
    for (name, target) in [
        ("database.sqlite", "elzevir.db"),
        ("elzevir.toml", "elzevir.toml"),
        ("operations.toml", "operations.toml"),
        ("author-table.txt", "author-table.txt"),
        ("cloud.json", "cloud.json"),
        ("cloud-plans.json", "cloud-plans.json"),
        ("cloud-artifacts.json", "cloud-artifacts.json"),
    ] {
        let source = restored.directory.path().join(name);
        if source.exists() {
            copy_new(&source, &stage.path().join(target))?;
        }
        replacements.push((stage.path().join(target), directory.join(target)));
    }
    let config_stage = tempfile::tempdir_in(config.parent().ok_or("configuration needs a parent")?)
        .map_err(|_| "cannot stage restored configuration")?;
    archive::write_new(&config_stage.path().join("server.toml"), source.as_bytes())?;
    let configuration_target = config.clone();
    replacements.push((config_stage.path().join("server.toml"), config));
    // Secrets are treated as one directory, never merged with stale credentials.
    let secrets = restored.directory.path().join("secrets");
    if secrets.exists() {
        make_directory(&stage.path().join("secrets"))?;
        for entry in std::fs::read_dir(secrets).map_err(|_| "restored secrets unavailable")? {
            let entry = entry.map_err(|_| "restored secrets unavailable")?;
            copy_new(
                &entry.path(),
                &stage.path().join("secrets").join(entry.file_name()),
            )?;
        }
    }
    replacements.push((stage.path().join("secrets"), directory.join("secrets")));
    let journal =
        crate::restore_journal::Journal::begin(&directory, &safety, &configuration_target)?;
    let result = (|| {
        // Move sidecars together with their database, before opening the replacement.
        for name in ["elzevir.db-wal", "elzevir.db-shm", "index"] {
            journal.preserve(&directory, &directory.join(name))?;
        }
        for (source, target) in &replacements {
            prepare_replacement(source, target, &configuration_target, &directory)?;
            journal.preserve(&directory, target)?;
            if source.exists() {
                std::fs::rename(source, target).map_err(|_| "cannot publish restored file")?;
            }
        }
        rebuild_action(&directory)?;
        crate::file_owner::private_tree(&directory.join("index"), &directory)?;
        Ok(())
    })();
    if result.is_err() {
        journal.rollback(&directory)?;
    } else {
        crate::restore_journal::Journal::commit(&directory)?;
    }
    result
}

fn validate_auxiliary(directory: &Path, reset_cloud_schedules: bool) -> Result<(), String> {
    let library = directory.join("library.toml");
    if library.exists() {
        let mut value: toml::Value = toml::from_str(&text(&library)?)
            .map_err(|_| "invalid restored library configuration")?;
        if value
            .get("call_number")
            .and_then(|call| call.get("author_table"))
            .is_some()
            && !directory.join("author-table.txt").exists()
        {
            return Err("backup is missing its configured author table".to_owned());
        }
        if directory.join("author-table.txt").exists() {
            let call = value
                .get_mut("call_number")
                .and_then(toml::Value::as_table_mut)
                .ok_or("missing author table configuration")?;
            call.insert(
                "author_table".to_owned(),
                toml::Value::String("author-table.txt".to_owned()),
            );
        }
        let library_text =
            toml::to_string_pretty(&value).map_err(|_| "invalid library configuration")?;
        std::fs::write(&library, library_text)
            .map_err(|_| "cannot prepare library configuration")?;
        std::fs::rename(&library, directory.join("elzevir.toml"))
            .map_err(|_| "cannot prepare library configuration")?;
        crate::config::load(directory)?;
    }
    if directory.join("cloud-plans.json").exists() {
        if reset_cloud_schedules {
            crate::cloud_plans::restored(directory)?;
        } else {
            crate::cloud_plans::load(directory)?;
        }
    }
    Ok(())
}

fn prepare_replacement(
    source: &Path,
    target: &Path,
    configuration_target: &Path,
    directory: &Path,
) -> Result<(), String> {
    if source.exists() {
        let reference = if target.exists() { target } else { directory };
        if target == configuration_target {
            crate::file_owner::configuration(source, reference)?;
        } else {
            crate::file_owner::private_tree(source, reference)?;
        }
    }
    Ok(())
}

fn rebuild(directory: &Path) -> Result<(), String> {
    let store = elzevir_store::ElzevirStore::open(
        &directory.join("elzevir.db"),
        &elzevir_store::StoreConfig::default(),
        elzevir_store::ElzevirHandler,
    )
    .map_err(|_| "cannot open restored database")?;
    let result = (|| {
        let (index, parallelism) = crate::open_index(directory)?;
        let options = elzevir_app::search::IndexOptions {
            parallelism,
            batch: crate::INDEX_BATCH,
        };
        elzevir_app::search::index_pending(&store, &store, &index, &options)
            .map_err(|_| "restored index rebuild failed")?;
        Ok(())
    })();
    result.and(
        store
            .shutdown()
            .map_err(|_| "restored database checkpoint failed".to_owned()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn restore_preserves_lock_inode_relocates_config_and_rolls_back_a_rebuild_failure() {
        let directory = tempfile::tempdir().unwrap();
        let data = crate::open_store(directory.path()).unwrap();
        let settings =
            crate::server_config::ServeArgs::for_status(None, Some(directory.path().to_owned()))
                .resolve()
                .unwrap();
        let config = crate::server_config::snapshot(&settings).unwrap();
        let image = directory.path().join("backup.age");
        archive::create(
            directory.path(),
            &config,
            &image,
            Some(data.store.readers()),
        )
        .unwrap();
        data.store.shutdown().unwrap();
        let crate::DataDir {
            _lock: lock_file, ..
        } = data;
        drop(lock_file);
        let original = std::fs::read(directory.path().join("elzevir.db")).unwrap();
        let marker = directory.path().join("elzevir.toml");
        std::fs::write(&marker, "[library]\ntime_zone='UTC'\n").unwrap();
        assert!(
            restore_with(
                &image,
                &directory.path().join("backup.key"),
                directory.path(),
                None,
                |_| Err("forced rebuild failure".to_owned())
            )
            .is_err()
        );
        assert_eq!(
            std::fs::read(directory.path().join("elzevir.db")).unwrap(),
            original
        );
        assert_eq!(
            std::fs::read_to_string(&marker).unwrap(),
            "[library]\ntime_zone='UTC'\n"
        );
        // A separately opened handle must conflict with the same lock throughout restore.
        let mut checked = false;
        restore_with(
            &image,
            &directory.path().join("backup.key"),
            directory.path(),
            None,
            |target| {
                checked = true;
                assert!(lock(target).is_err());
                rebuild(target)
            },
        )
        .unwrap();
        assert!(checked);
        assert!(
            !marker.exists(),
            "an absent library configuration must remove stale configuration"
        );
        assert_eq!(
            crate::server_config::ServeArgs::for_status(
                Some(directory.path().join("server.toml")),
                None
            )
            .resolve()
            .unwrap()
            .data_dir,
            directory.path()
        );
        let reopened = crate::open_store(directory.path()).unwrap();
        reopened.store.shutdown().unwrap();
    }
}
