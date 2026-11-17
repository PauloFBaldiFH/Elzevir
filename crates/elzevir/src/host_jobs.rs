// SPDX-License-Identifier: GPL-3.0-or-later
//! Privileged client-side operations survive UI exit and never run inside the daemon.
use crate::backup_archive as archive;
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Task {
    Restore {
        input: PathBuf,
        key: PathBuf,
        config: Option<PathBuf>,
    },
    Restart {
        name: String,
    },
    Reboot,
    CloudflareLogin,
    TestTunnel,
    CloudflareSetup {
        setup: crate::cloudflare::Setup,
    },
    InstallTool {
        tool: crate::managed_tools::Tool,
    },
    Update {
        config: PathBuf,
        name: String,
        version: String,
        package: Option<PathBuf>,
        checksums: Option<PathBuf>,
        executable: PathBuf,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ticket {
    schema: u8,
    id: String,
    directory: PathBuf,
    task: Task,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Receipt {
    pub id: String,
    pub action: String,
    pub state: String,
    #[serde(default)]
    pub authorization_uri: Option<String>,
}

#[derive(clap::Args)]
pub(crate) struct Args {
    #[arg(long)]
    ticket: PathBuf,
}

fn result_path(directory: &Path, id: &str) -> Result<PathBuf, String> {
    if id.len() != 32 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("invalid host job identity".to_owned());
    }
    Ok(directory.join("host-jobs").join(format!("{id}.json")))
}
fn action(task: &Task) -> &'static str {
    match task {
        Task::Restore { .. } => "restore",
        Task::Restart { .. } => "restart",
        Task::Reboot => "reboot",
        Task::CloudflareLogin => "cloudflare_login",
        Task::TestTunnel => "tunnel_test",
        Task::CloudflareSetup { .. } => "cloudflare_setup",
        Task::InstallTool { tool } => match tool {
            crate::managed_tools::Tool::Cloudflared => "install_cloudflared",
            crate::managed_tools::Tool::Rclone => "install_rclone",
        },
        Task::Update { .. } => "update",
    }
}
fn save(directory: &Path, receipt: &Receipt) -> Result<(), String> {
    let path = result_path(directory, &receipt.id)?;
    let bytes = serde_json::to_vec(receipt).map_err(|_| "cannot encode host job receipt")?;
    let mut file =
        tempfile::NamedTempFile::new_in(path.parent().ok_or("host receipt needs parent")?)
            .map_err(|_| "cannot prepare host job receipt")?;
    file.write_all(&bytes)
        .and_then(|()| file.as_file().sync_all())
        .map_err(|_| "cannot save host job receipt")?;
    file.persist(path)
        .map_err(|_| "cannot publish host job receipt")?;
    Ok(())
}

pub(crate) fn launch(directory: &Path, task: Task) -> Result<String, String> {
    archive::regular(&directory.join("elzevir.db"))?;
    crate::backup_cli::make_directory(&directory.join("host-jobs"))?;
    let id = archive::identifier()?;
    let ticket = Ticket {
        schema: 1,
        id: id.clone(),
        directory: directory
            .canonicalize()
            .map_err(|_| "institution directory unavailable")?,
        task,
    };
    let temporary = tempfile::Builder::new()
        .prefix("elzevir-host-")
        .tempdir()
        .map_err(|_| "cannot prepare host operation")?;
    let path = temporary.path().join("request.json");
    archive::write_new(
        &path,
        &serde_json::to_vec(&ticket).map_err(|_| "cannot encode host operation")?,
    )?;
    let mut receipt = Receipt {
        id: id.clone(),
        action: action(&ticket.task).to_owned(),
        state: "queued".to_owned(),
        authorization_uri: None,
    };
    save(directory, &receipt)?;
    let executable = std::env::current_exe().map_err(|_| "cannot locate host executable")?;
    let helper = if matches!(ticket.task, Task::Update { .. }) {
        let helper = temporary.path().join(if cfg!(windows) {
            "helper.exe"
        } else {
            "helper"
        });
        crate::backup_cli::copy_new(&executable, &helper)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700))
                .map_err(|_| "cannot prepare update helper")?;
        }
        helper
    } else {
        executable
    };
    let mut command = Command::new(helper);
    command
        .args(["host-job", "--ticket"])
        .arg(&path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0000_0008 | 0x0000_0200);
    }
    let Ok(mut child) = command.spawn() else {
        "failed".clone_into(&mut receipt.state);
        save(directory, &receipt)?;
        return Err("cannot launch host operation".to_owned());
    };
    // Reap Unix children and close Windows process handles independently of UI exit.
    if std::thread::Builder::new()
        .name("elzevir-host-reaper".to_owned())
        .spawn(move || {
            if child.wait().is_err() {
                tracing::warn!(event = "host.reap_failed");
            }
        })
        .is_err()
    {
        // The detached operation still runs; the OS reaps it when this client exits.
        tracing::warn!(event = "host.reaper_unavailable");
    }
    // The child owns this private directory until it has read the ticket.
    drop(temporary.keep());
    Ok(id)
}

pub(crate) fn run(args: &Args) -> Result<(), String> {
    check_private_ticket(&args.ticket)?;
    let mut bytes = Vec::new();
    std::fs::File::open(&args.ticket)
        .and_then(|file| file.take(16_385).read_to_end(&mut bytes))
        .map_err(|_| "host operation ticket unavailable")?;
    let ticket: Ticket =
        serde_json::from_slice(&bytes).map_err(|_| "invalid host operation ticket")?;
    if ticket.schema != 1 {
        return Err("unsupported host operation ticket".to_owned());
    }
    result_path(&ticket.directory, &ticket.id)?;
    std::fs::remove_file(&args.ticket).map_err(|_| "cannot consume host operation ticket")?;
    let parent = args
        .ticket
        .parent()
        .ok_or("invalid host operation ticket")?;
    let helper = parent.join(if cfg!(windows) {
        "helper.exe"
    } else {
        "helper"
    });
    // Windows cannot unlink its mapped helper; its private temporary directory is
    // retained for the OS's temporary-file cleanup. Unix can remove the mapped file.
    if !cfg!(windows) && helper.exists() {
        std::fs::remove_file(helper).map_err(|_| "cannot remove consumed update helper")?;
    }
    if let Err(error) = std::fs::remove_dir(parent)
        && (!cfg!(windows) || error.kind() != std::io::ErrorKind::DirectoryNotEmpty)
    {
        return Err("cannot close host operation ticket".to_owned());
    }
    let mut receipt = Receipt {
        id: ticket.id,
        action: action(&ticket.task).to_owned(),
        state: "running".to_owned(),
        authorization_uri: None,
    };
    let lock = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(ticket.directory.join("host-operation.lock"))
        .map_err(|_| "host operation lock unavailable")?;
    if lock.try_lock().is_err() {
        "failed".clone_into(&mut receipt.state);
        save(&ticket.directory, &receipt)?;
        return Err("another host operation is running".to_owned());
    }
    save(&ticket.directory, &receipt)?;
    let shared = std::sync::Arc::new(std::sync::Mutex::new(receipt.clone()));
    let directory = ticket.directory.clone();
    let progress: crate::process_runner::Progress = std::sync::Arc::new(move |bytes| {
        if let Some(uri) = crate::cloudflare::authorization_url(bytes) {
            let mut receipt = shared.lock().map_err(|_| "host progress unavailable")?;
            if receipt.authorization_uri.as_ref() != Some(&uri) {
                receipt.authorization_uri = Some(uri);
                save(&directory, &receipt)?;
            }
        }
        Ok(())
    });
    let result = execute(&ticket.directory, ticket.task, Some(progress));
    if result.is_ok() {
        "completed"
    } else {
        "failed"
    }
    .clone_into(&mut receipt.state);
    save(&ticket.directory, &receipt)?;
    result
}

fn check_private_ticket(path: &Path) -> Result<(), String> {
    if path.file_name().is_none_or(|name| name != "request.json")
        || archive::regular(path)? > 16_384
    {
        return Err("invalid host ticket path".to_owned());
    }
    let parent = path.parent().ok_or("host ticket parent required")?;
    let metadata =
        std::fs::symlink_metadata(parent).map_err(|_| "host ticket directory unavailable")?;
    if !metadata.is_dir()
        || metadata.is_symlink()
        || parent
            .file_name()
            .and_then(|name| name.to_str())
            .is_none_or(|name| !name.starts_with("elzevir-host-"))
    {
        return Err("invalid host ticket directory".to_owned());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let file = std::fs::symlink_metadata(path).map_err(|_| "host ticket unavailable")?;
        let uid = rustix::process::getuid().as_raw();
        if metadata.uid() != uid
            || file.uid() != uid
            || metadata.mode() & 0o077 != 0
            || file.mode() & 0o077 != 0
        {
            return Err("host ticket must be private to the operator".to_owned());
        }
    }
    Ok(())
}

fn execute(
    directory: &Path,
    task: Task,
    progress: Option<crate::process_runner::Progress>,
) -> Result<(), String> {
    match task {
        Task::Restore { input, key, config } => {
            crate::backup_cli::restore(&input, &key, directory, config.as_deref())
        }
        Task::Restart { name } => {
            crate::service::native_action("restart", &name)?;
            wait_healthy(directory)
        }
        Task::Reboot => reboot(),
        Task::CloudflareLogin => crate::cloudflare::login(directory, progress),
        Task::TestTunnel => crate::tunnel_observe::test(directory),
        Task::CloudflareSetup { setup } => crate::cloudflare::configure(directory, &setup),
        Task::InstallTool { tool } => crate::managed_tools::install(tool).map(drop),
        Task::Update {
            config,
            name,
            version,
            package,
            checksums,
            executable,
        } => crate::update::execute(
            directory,
            crate::update::Update {
                config,
                name,
                version,
                package,
                checksums,
                executable,
            },
        ),
    }
}

pub(crate) fn wait_healthy(directory: &Path) -> Result<(), String> {
    for _ in 0..60 {
        if crate::status::inspect(directory).state == "running" {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    Err("service did not become healthy".to_owned())
}

fn reboot() -> Result<(), String> {
    use crate::process_runner::{Native, Request, Runner as _};
    #[cfg(target_os = "linux")]
    let (program, args) = (PathBuf::from("/usr/bin/systemctl"), vec!["reboot".into()]);
    #[cfg(windows)]
    let (program, args) = (
        PathBuf::from(
            std::env::var_os("SystemRoot").ok_or("Windows system directory unavailable")?,
        )
        .join("System32")
        .join("shutdown.exe"),
        vec!["/r".into(), "/t".into(), "10".into()],
    );
    #[cfg(not(any(target_os = "linux", windows)))]
    return Err("host reboot unavailable on this platform".to_owned());
    #[cfg(any(target_os = "linux", windows))]
    {
        let output = Native.run(Request {
            progress: None,
            program,
            args,
            input: None,
            timeout: std::time::Duration::from_secs(10),
            limit: 4096,
        })?;
        if output.success {
            Ok(())
        } else {
            Err("operating system refused reboot".to_owned())
        }
    }
}

pub(crate) fn receipts(directory: &Path) -> Result<Vec<Receipt>, String> {
    let path = directory.join("host-jobs");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let metadata = std::fs::symlink_metadata(&path).map_err(|_| "host job history unavailable")?;
    if !metadata.is_dir() || metadata.is_symlink() {
        return Err("invalid host job directory".to_owned());
    }
    let mut entries = std::fs::read_dir(path)
        .map_err(|_| "host job history unavailable")?
        .take(257)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "host job history unavailable")?;
    if entries.len() > 256 {
        return Err("host job history limit exceeded".to_owned());
    }
    entries.sort_by_key(std::fs::DirEntry::file_name);
    let mut receipts = Vec::new();
    for entry in entries {
        let name = entry.file_name();
        let Some(id) = name.to_str().and_then(|name| name.strip_suffix(".json")) else {
            continue;
        };
        result_path(directory, id)?;
        if archive::regular(&entry.path())? > 4096 {
            return Err("host receipt exceeds limit".to_owned());
        }
        let mut source = Vec::new();
        std::fs::File::open(entry.path())
            .and_then(|file| file.take(4097).read_to_end(&mut source))
            .map_err(|_| "host receipt unavailable")?;
        if source.len() > 4096 {
            return Err("host receipt exceeds limit".to_owned());
        }
        let receipt: Receipt =
            serde_json::from_slice(&source).map_err(|_| "invalid host receipt")?;
        if receipt.id != id
            || !matches!(
                receipt.state.as_str(),
                "queued" | "running" | "completed" | "failed"
            )
            || !matches!(
                receipt.action.as_str(),
                "restore"
                    | "restart"
                    | "reboot"
                    | "update"
                    | "install_cloudflared"
                    | "install_rclone"
                    | "cloudflare_login"
                    | "cloudflare_setup"
                    | "tunnel_test"
            )
        {
            return Err("invalid host receipt".to_owned());
        }
        if receipt.authorization_uri.as_ref().is_some_and(|uri| {
            crate::cloudflare::authorization_url(uri.as_bytes()).as_deref() != Some(uri.as_str())
        }) {
            return Err("invalid host authorization URI".to_owned());
        }
        receipts.push(receipt);
    }
    Ok(receipts)
}

pub(crate) fn wait(directory: &Path, id: &str) -> Result<(), String> {
    let started = std::time::Instant::now();
    let mut seen_uri = None;
    while started.elapsed() < std::time::Duration::from_mins(45) {
        let receipts = receipts(directory)?;
        let receipt = receipts
            .iter()
            .find(|receipt| receipt.id == id)
            .ok_or("host job receipt unavailable")?;
        if receipt.authorization_uri.is_some() && receipt.authorization_uri != seen_uri {
            println!(
                "{}",
                serde_json::json!({"authorization_uri":receipt.authorization_uri})
            );
            seen_uri = receipt.authorization_uri.clone();
        }
        match receipt.state.as_str() {
            "completed" => return Ok(()),
            "failed" => return Err("host operation failed; inspect the recovery files".to_owned()),
            _ => std::thread::sleep(std::time::Duration::from_millis(200)),
        }
    }
    Err("host operation is still running; follow its receipt".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn receipts_refuse_untrusted_names_states_and_oversized_content() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join("host-jobs")).unwrap();
        let id = "a".repeat(32);
        save(
            directory.path(),
            &Receipt {
                id: id.clone(),
                action: "restore".to_owned(),
                state: "completed".to_owned(),
                authorization_uri: None,
            },
        )
        .unwrap();
        assert_eq!(receipts(directory.path()).unwrap().len(), 1);
        std::fs::write(
            result_path(directory.path(), &id).unwrap(),
            vec![b'a'; 4097],
        )
        .unwrap();
        assert!(receipts(directory.path()).is_err());
        assert!(result_path(directory.path(), "../file").is_err());
    }
    #[cfg(unix)]
    #[test]
    fn host_ticket_requires_private_operator_owned_files() {
        use std::os::unix::fs::PermissionsExt;
        let temporary = tempfile::Builder::new()
            .prefix("elzevir-host-")
            .tempdir()
            .unwrap();
        let path = temporary.path().join("request.json");
        archive::write_new(&path, b"{}").unwrap();
        check_private_ticket(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(check_private_ticket(&path).is_err());
    }
}
