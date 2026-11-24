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

pub(crate) fn lock(directory: &Path) -> Result<crate::InstitutionLock, String> {
    archive::regular(&directory.join("elzevir.db"))?;
    lock_existing(directory)
}

pub(crate) fn lock_existing(directory: &Path) -> Result<crate::InstitutionLock, String> {
    let path = directory.join("elzevir.lock");
    archive::regular(&path)?;
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|_| "institution lock unavailable")?;
    file.try_lock()
        .map_err(|_| "stop the institution before this operation")?;
    Ok(crate::InstitutionLock(file))
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
            #[cfg(unix)]
            let mut builder = std::fs::DirBuilder::new();
            #[cfg(not(unix))]
            let builder = std::fs::DirBuilder::new();
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
            let manifest = archive::verify(&input, &recovery_key)?;
            println!(
                "{}",
                serde_json::to_string(&manifest).map_err(|_| "cannot encode manifest")?
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
    let input = std::fs::File::open(source).map_err(|_| "artifact unavailable")?;
    publish_copy(input, output)
}

/// Opens a managed artifact relative to its institution, even when the exporter is elevated.
pub(crate) fn export_managed(directory: &Path, id: &str, output: &Path) -> Result<(), String> {
    let source = artifact(directory, id)?;
    let scope = cap_std::fs::Dir::open_ambient_dir(directory, cap_std::ambient_authority())
        .map_err(|_| "institution unavailable")?;
    let relative = source
        .strip_prefix(directory)
        .map_err(|_| "invalid artifact path")?;
    let input = archive::scoped_file(&scope, relative)?.into_std();
    publish_copy(input, output)
}

fn publish_copy(mut input: std::fs::File, output: &Path) -> Result<(), String> {
    let parent = output.parent().ok_or("destination needs a parent")?;
    let mut staged =
        tempfile::NamedTempFile::new_in(parent).map_err(|_| "cannot prepare export")?;
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
    restore_inner(
        prepare(archive::extract(input, key)?, directory, true)?,
        directory,
        config,
        rebuild_action,
        true,
        &mut false,
    )
    .map(drop)
}

pub(crate) fn restore_rollback(
    input: &Path,
    key: &Path,
    directory: &Path,
    config: Option<&Path>,
) -> Result<(), String> {
    restore_inner(
        prepare(archive::extract_full(input, key)?, directory, false)?,
        directory,
        config,
        rebuild,
        false,
        &mut false,
    )
    .map(drop)
}

pub(crate) struct Prepared {
    archive: archive::Extracted,
    configuration: Option<String>,
}

pub(crate) fn prepare(
    archive: archive::Extracted,
    directory: &Path,
    reset_schedules: bool,
) -> Result<Prepared, String> {
    let configuration = if archive.manifest.scope == elzevir_panel::BackupScope::Full {
        let destination = directory
            .canonicalize()
            .map_err(|_| "restore directory unavailable")?;
        let config = crate::server_config::relocate(
            &text(&archive.directory.path().join("server.toml"))?,
            &destination,
        )?;
        validate_auxiliary(archive.directory.path(), reset_schedules)?;
        Some(config)
    } else {
        None
    };
    Ok(Prepared {
        archive,
        configuration,
    })
}

pub(crate) fn restore_managed(
    restored: Prepared,
    directory: &Path,
    config: &Path,
) -> Result<crate::restore_journal::Journal, String> {
    let mut swapping = false;
    restore_inner(
        restored,
        directory,
        Some(config),
        rebuild,
        true,
        &mut swapping,
    )
    .map_err(|error| {
        if swapping {
            error
        } else {
            crate::managed_restore::BEFORE.to_owned()
        }
    })
}

fn restore_inner(
    prepared: Prepared,
    directory: &Path,
    config: Option<&Path>,
    rebuild_action: impl FnOnce(&Path) -> Result<(), String>,
    safety_snapshot: bool,
    swapping: &mut bool,
) -> Result<crate::restore_journal::Journal, String> {
    let directory = directory
        .canonicalize()
        .map_err(|_| "restore directory must already exist")?;
    let institution_lock = lock(&directory)?;
    let Prepared {
        archive: restored,
        configuration,
    } = prepared;
    let full = configuration.is_some();
    let config = config.map_or_else(|| directory.join("server.toml"), Path::to_owned);
    let config = crate::installation::absolute(&config)?;

    let stage =
        crate::private_temp::directory_in(crate::restore_journal::trusted_parent(&directory)?)
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
        if !full && name != "database.sqlite" {
            continue;
        }
        let source = restored.directory.path().join(name);
        if source.exists() {
            copy_new(&source, &stage.path().join(target))?;
        }
        replacements.push((stage.path().join(target), directory.join(target)));
    }

    let config_stage;
    if let Some(source) = configuration {
        config_stage = crate::private_temp::directory_in(
            config.parent().ok_or("configuration needs a parent")?,
        )
        .map_err(|_| "cannot stage restored configuration")?;
        archive::write_new(&config_stage.path().join("server.toml"), source.as_bytes())?;

        replacements.push((config_stage.path().join("server.toml"), config.clone()));
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
    }
    crate::repository_fs::restore_archive(&directory, &restored, &institution_lock)?;
    let safety = crate::restore_journal::create_safety(&directory)?;
    // A consistent independent image is retained even if rebuilding the replacement fails.
    if safety_snapshot {
        elzevir_store::snapshot_offline(
            &directory.join("elzevir.db"),
            &safety.join("database.sqlite"),
        )
        .map_err(|_| "cannot create restore safety image")?;
    }
    let journal =
        crate::restore_journal::Journal::begin_scoped(&directory, &safety, &config, full)?;
    *swapping = true;
    let result = (|| {
        // Move sidecars together with their database, before opening the replacement.
        for name in ["elzevir.db-wal", "elzevir.db-shm", "index"] {
            journal.preserve(&directory, &directory.join(name))?;
        }
        for (source, target) in &replacements {
            prepare_replacement(source, target, &config, &directory)?;
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
    result.map(|()| journal)
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
    fn database_restore_preserves_credentials_and_excludes_concurrent_restore() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path();
        let data = crate::open_store(directory).unwrap();
        let image = directory.join("database.age");
        let key = directory.join("backup.key");
        let config = directory.join("server.toml");
        let control: crate::cancellation::Control = std::sync::Arc::new(|| false);
        archive::create_selected(
            directory,
            "",
            &image,
            Some(data.store.readers()),
            &control,
            elzevir_panel::BackupOptions {
                scope: elzevir_panel::BackupScope::Database,
                ..Default::default()
            },
            |_| Ok(()),
        )
        .unwrap();
        data.store.shutdown().unwrap();
        let crate::DataDir {
            _lock: lock_file, ..
        } = data;
        drop(lock_file);
        let secrets = directory.join("secrets");
        std::fs::create_dir_all(&secrets).unwrap();
        std::fs::write(secrets.join("credential"), "original").unwrap();
        std::fs::write(&config, "original configuration").unwrap();
        let old_key = std::fs::read(&key).unwrap();
        restore_with(&image, &key, directory, Some(&config), |target| {
            std::thread::scope(|s| {
                let competing = s.spawn(|| restore(&image, &key, target, Some(&config)));
                assert_eq!(
                    competing.join().unwrap().unwrap_err(),
                    "stop the institution before this operation"
                );
            });
            rebuild(target)
        })
        .unwrap();
        assert_eq!(std::fs::read(key).unwrap(), old_key);
        assert_eq!(
            std::fs::read_to_string(config).unwrap(),
            "original configuration"
        );
        assert_eq!(
            std::fs::read_to_string(secrets.join("credential")).unwrap(),
            "original"
        );
        assert!(!crate::restore_journal::pending(directory));
    }
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
            directory.path().canonicalize().unwrap()
        );
        let reopened = crate::open_store(directory.path()).unwrap();
        reopened.store.shutdown().unwrap();
    }
}
