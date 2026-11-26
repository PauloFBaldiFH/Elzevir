// SPDX-License-Identifier: GPL-3.0-or-later
//! Inspect the selected personal installation before enabling operational actions.
use crate::{
    first_use,
    server_config::{ServeArgs, Settings},
};
use elzevir_panel::{Inspection, InspectionAction, InstallationIssue};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

fn inspect(
    args: &ServeArgs,
    default: Option<&Path>,
) -> Result<(Settings, PathBuf, &'static str), Inspection> {
    let directory = args.data_dir.as_deref().or(default);
    let config = args
        .config
        .clone()
        .or_else(|| directory.map(|d| d.join(first_use::CONFIG)));
    let issue = |reason, path: &Path| Inspection {
        issue: reason,
        location: path.display().to_string(),
        running: false,
        administrator: None,
    };
    let config = config.ok_or_else(|| issue(InstallationIssue::Unavailable, Path::new("")))?;
    match fs::symlink_metadata(&config) {
        Ok(meta) if meta.is_file() => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound && args.config.is_none() => {
            let folder = config
                .parent()
                .ok_or_else(|| issue(InstallationIssue::Unavailable, &config))?;
            let is_new = match fs::symlink_metadata(folder) {
                Err(e) => e.kind() == io::ErrorKind::NotFound,
                Ok(meta) => {
                    meta.is_dir() && fs::read_dir(folder).is_ok_and(|mut d| d.next().is_none())
                }
            };
            let reason = if is_new && args.data_dir.is_none() {
                InstallationIssue::New
            } else if fs::symlink_metadata(folder.join("elzevir.db"))
                .is_ok_and(|meta| meta.is_file())
            {
                InstallationIssue::Legacy
            } else {
                InstallationIssue::Unavailable
            };
            let mut report = issue(reason, folder);
            let state = crate::status::inspect(folder).state;
            report.running = matches!(state, "running" | "degraded");
            if state == "stopped" && reason == InstallationIssue::Legacy {
                report.administrator = offline_admin(folder);
            }
            return Err(report);
        }
        _ => return Err(issue(InstallationIssue::InvalidConfig, &config)),
    }
    let mut input = args.clone();
    input.config = Some(config.clone());
    let settings = input
        .resolve()
        .map_err(|_| issue(InstallationIssue::InvalidConfig, &config))?;
    let db = settings.data_dir.join("elzevir.db");
    match fs::symlink_metadata(&db) {
        Ok(meta) if meta.is_file() && meta.len() > 0 => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Err(issue(InstallationIssue::MissingDatabase, &db));
        }
        _ => return Err(issue(InstallationIssue::Unavailable, &db)),
    }
    if crate::restore_journal::pending(&settings.data_dir) {
        return Err(issue(InstallationIssue::Unavailable, &settings.data_dir));
    }
    let state = crate::status::inspect(&settings.data_dir).state;
    if state == "stopped" {
        match offline_admin(&settings.data_dir) {
            Some(true) => {}
            Some(false) => return Err(issue(InstallationIssue::MissingAdministrator, &db)),
            None => return Err(issue(InstallationIssue::Unavailable, &db)),
        }
    }
    Ok((settings, config, state))
}

fn offline_admin(directory: &Path) -> Option<bool> {
    if crate::restore_journal::pending(directory) {
        return None;
    }
    let path = directory.join("elzevir.lock");
    if !fs::symlink_metadata(&path).ok()?.is_file() {
        return None;
    }
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .ok()?;
    lock.try_lock().ok()?;
    elzevir_store::inspect_offline_administrator(&directory.join("elzevir.db")).ok()
}

pub(crate) fn open(
    args: &ServeArgs,
    ascii: bool,
    auto_start: bool,
    no_self_update: bool,
) -> Result<(), String> {
    let lang = first_use::language(args.lang.as_deref());
    let default = if args.config.is_none() && args.data_dir.is_none() {
        first_use::data_dir().ok()
    } else {
        None
    };
    loop {
        let inspection = match inspect(args, default.as_deref()) {
            Ok((mut settings, config, state)) => {
                if state != "unavailable" {
                    if auto_start && state == "stopped" && !crate::panel_cli::managed() {
                        let saved = args.listen.is_none().then_some(config.as_path());
                        settings.listen =
                            crate::listen_port::ensure(settings.listen, saved, &settings.lang)?;
                        crate::background::launch(&settings, Some(&config))?;
                    }
                    return crate::panel_cli::connected(
                        &settings,
                        Some(config),
                        ascii,
                        no_self_update,
                        || false,
                    );
                }
                Inspection {
                    location: settings.data_dir.display().to_string(),
                    issue: InstallationIssue::Unavailable,
                    running: false,
                    administrator: None,
                }
            }
            Err(inspection) => inspection,
        };
        match elzevir_panel::inspect_installation(lang, ascii, inspection)
            .map_err(|e| e.to_string())?
        {
            None => return Ok(()),
            Some(InspectionAction::Connect) => {
                let Some(directory) = args.data_dir.as_deref().or(default.as_deref()) else {
                    continue;
                };
                if !matches!(
                    crate::status::inspect(directory).state,
                    "running" | "degraded"
                ) {
                    continue;
                }
                let mut selection = args.clone();
                selection.data_dir = Some(directory.to_owned());
                let settings = selection.resolve().map_err(|e| e.to_string())?;
                return crate::panel_cli::connected(&settings, None, ascii, no_self_update, || {
                    false
                });
            }
            Some(InspectionAction::Create) => {}
        }
        if let Some(setup) =
            elzevir_panel::setup_with_login(lang, "admin", ascii).map_err(|e| e.to_string())?
        {
            let mut init = crate::init::InitArgs::first_use(
                args.lang.clone(),
                args.listen,
                args.insecure_cookies,
            );
            init.listen = crate::listen_port::ensure(init.listen, None, lang)?;
            crate::init::initialize(&init, Some(setup))?;
            // Return to the new institution; starting service is a separate explicit action.
            return open(args, ascii, false, no_self_update);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inspection_never_creates_files_or_treats_an_unreadable_installation_as_new() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("new library #1");
        let args = ServeArgs::for_status(None, None);
        let issue = || inspect(&args, Some(&folder)).err().unwrap().issue;
        assert_eq!(issue(), InstallationIssue::New);
        assert!(!folder.exists());
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join("elzevir.db"), b"existing").unwrap();
        assert_eq!(issue(), InstallationIssue::Legacy);
        fs::write(folder.join("server.toml"), b"invalid TOML").unwrap();
        assert_eq!(issue(), InstallationIssue::InvalidConfig);
        let config = crate::server_config::render_init(
            "Library",
            &folder,
            "127.0.0.1:7390".parse().unwrap(),
            true,
        )
        .unwrap();
        fs::write(folder.join("server.toml"), config).unwrap();
        fs::File::create(folder.join("elzevir.lock")).unwrap();
        assert_eq!(issue(), InstallationIssue::Unavailable);
        assert_eq!(fs::read_dir(&folder).unwrap().count(), 3);
        fs::remove_file(folder.join("elzevir.db")).unwrap();
        assert_eq!(issue(), InstallationIssue::MissingDatabase);
        {
            let database = crate::open_store(&folder).unwrap();
            database.store.shutdown().unwrap();
        }
        assert_eq!(issue(), InstallationIssue::MissingAdministrator);
        assert!(!folder.join("elzevir.db-shm").exists());
        assert!(!folder.join("elzevir.db-wal").exists());
        fs::write(folder.join("elzevir.db-wal"), b"uncheckpointed").unwrap();
        assert_eq!(issue(), InstallationIssue::Unavailable);
        assert_eq!(
            fs::read(folder.join("elzevir.db-wal")).unwrap(),
            b"uncheckpointed"
        );
    }
}
