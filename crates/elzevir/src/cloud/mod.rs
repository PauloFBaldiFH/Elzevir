// SPDX-License-Identifier: GPL-3.0-or-later
//! Institution-owned rclone connections and private provider configuration sessions.
use crate::{
    backup_archive as archive,
    process_runner::{Native, Output, Progress, Request, Runner},
};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};
use zeroize::Zeroize as _;
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[expect(
    clippy::struct_excessive_bools,
    reason = "mirrors independent flags in the published rclone option schema"
)]
struct Field {
    name: String,
    #[serde(default)]
    help: String,
    #[serde(default)]
    default: serde_json::Value,
    #[serde(default)]
    required: bool,
    #[serde(default)]
    is_password: bool,
    #[serde(default)]
    sensitive: bool,
    #[serde(default)]
    exclusive: bool,
    #[serde(default)]
    examples: Vec<Example>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Example {
    value: String,
    #[serde(default)]
    help: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Provider {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    options: Vec<Field>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Pending {
    #[serde(default)]
    revision: String,
    #[serde(default)]
    state: String,
    option: Option<Field>,
    #[serde(default)]
    error: String,
}
impl Drop for Pending {
    fn drop(&mut self) {
        self.state.zeroize();
        self.error.zeroize();
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Connection {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub config_id: String,
    pub ready: bool,
    pub parent: Option<Parent>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Parent {
    pub id: String,
    pub folder: String,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    connections: Vec<Connection>,
}
fn word(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}
pub(crate) fn valid_id(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
fn read(path: &Path, limit: u64) -> Result<zeroize::Zeroizing<Vec<u8>>, String> {
    if archive::regular(path)? > limit {
        return Err("cloud configuration exceeds limit".to_owned());
    }
    let mut bytes = zeroize::Zeroizing::new(Vec::new());
    std::fs::File::open(path)
        .and_then(|file| file.take(limit + 1).read_to_end(&mut bytes))
        .map_err(|_| "cloud configuration unavailable")?;
    if u64::try_from(bytes.len()).map_err(|_| "cloud configuration exceeds limit")? > limit {
        return Err("cloud configuration exceeds limit".to_owned());
    }
    Ok(bytes)
}
fn catalog(directory: &Path) -> Result<Catalog, String> {
    let path = directory.join("cloud.json");
    if !path.exists() {
        return Ok(Catalog::default());
    }
    let catalog: Catalog = serde_json::from_slice(&read(&path, 65_536)?)
        .map_err(|_| "invalid cloud connection catalog")?;
    if catalog.connections.len() > 8 {
        return Err("cloud connection limit exceeded".to_owned());
    }
    let mut ids = std::collections::BTreeSet::new();
    let mut names = std::collections::BTreeSet::new();
    for connection in &catalog.connections {
        if !valid_id(&connection.id)
            || !valid_id(&connection.config_id)
            || !ids.insert(&connection.id)
            || !names.insert(&connection.name)
            || !word(&connection.provider)
            || connection.name.is_empty()
            || connection.name.len() > 64
            || connection.name.chars().any(char::is_control)
            || connection
                .parent
                .as_ref()
                .is_some_and(|parent| !valid_id(&parent.id) || folder(&parent.folder).is_err())
        {
            return Err("invalid cloud connection catalog".to_owned());
        }
    }
    for connection in &catalog.connections {
        if let Some(parent) = &connection.parent {
            if connection.provider != "crypt"
                || !catalog.connections.iter().any(|candidate| {
                    candidate.id == parent.id
                        && candidate.ready
                        && candidate.config_id == connection.config_id
                        && candidate.parent.is_none()
                })
            {
                return Err("invalid crypt parent connection".to_owned());
            }
        } else if connection.config_id != connection.id {
            return Err("invalid cloud configuration identity".to_owned());
        }
    }
    Ok(catalog)
}
fn save(directory: &Path, catalog: &Catalog) -> Result<(), String> {
    crate::broker::publish(
        directory,
        &directory.join("cloud.json"),
        &serde_json::to_vec(catalog).map_err(|_| "cannot encode cloud connection catalog")?,
    )
}

pub(crate) fn connection(directory: &Path, id: &str) -> Result<Connection, String> {
    if !valid_id(id) {
        return Err("invalid cloud connection identity".to_owned());
    }
    catalog(directory)?
        .connections
        .into_iter()
        .find(|connection| connection.id == id)
        .ok_or("cloud connection not found".to_owned())
}
fn config(directory: &Path, connection: &Connection) -> PathBuf {
    directory
        .join("secrets")
        .join(format!("rclone-{}.conf", connection.config_id))
}
fn pending_path(directory: &Path, connection: &Connection) -> PathBuf {
    directory
        .join("secrets")
        .join(format!("rclone-{}.pending.json", connection.id))
}
fn remote(connection: &Connection) -> String {
    format!("elzevir_{}", connection.id)
}
fn lock(directory: &Path, connection: &Connection) -> Result<std::fs::File, String> {
    use fs4::fs_std::FileExt as _;
    let path = directory.join(format!("cloud-{}.lock", connection.config_id));
    if path.exists() {
        archive::regular(&path)?;
    }
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|_| "cloud connection lock unavailable")?;
    crate::file_owner::private_tree(&path, directory)?;
    file.try_lock_exclusive()
        .map_err(|_| "cloud connection is in use")?;
    Ok(file)
}
pub(crate) fn folder(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 256
        || value.starts_with('/')
        || value.ends_with('/')
        || value
            .chars()
            .any(|character| character.is_control() || matches!(character, ':' | '\\'))
        || value.split('/').any(|part| matches!(part, "" | "." | ".."))
    {
        return Err("cloud folder must be a bounded relative path without traversal".to_owned());
    }
    Ok(())
}
fn environment(
    directory: &Path,
    connection: &Connection,
) -> Vec<(String, zeroize::Zeroizing<String>)> {
    vec![
        (
            "RCLONE_CONFIG".to_owned(),
            zeroize::Zeroizing::new(config(directory, connection).to_string_lossy().into_owned()),
        ),
        (
            "RCLONE_LOG_LEVEL".to_owned(),
            zeroize::Zeroizing::new("NOTICE".to_owned()),
        ),
        (
            "RCLONE_CONFIG_AUTH_NO_BROWSER".to_owned(),
            zeroize::Zeroizing::new("true".to_owned()),
        ),
    ]
}
fn run(
    runner: &impl Runner,
    program: &Path,
    args: Vec<OsString>,
    environment: Vec<(String, zeroize::Zeroizing<String>)>,
    input: Option<zeroize::Zeroizing<Vec<u8>>>,
    progress: Option<Progress>,
    limit: u64,
) -> Result<Output, String> {
    let output = runner.run(Request {
        environment: Some(environment),
        program: program.to_owned(),
        args,
        input,
        progress,
        timeout: Duration::from_mins(15),
        limit,
    })?;
    if !output.success {
        return Err("rclone operation failed; private configuration was preserved".to_owned());
    }
    Ok(output)
}
fn providers_with(
    directory: &Path,
    program: &Path,
    runner: &impl Runner,
    progress: Option<Progress>,
) -> Result<Vec<Provider>, String> {
    let output = run(
        runner,
        program,
        vec!["config".into(), "providers".into()],
        vec![(
            "RCLONE_LOG_LEVEL".to_owned(),
            zeroize::Zeroizing::new("NOTICE".to_owned()),
        )],
        None,
        progress,
        4 * 1024 * 1024,
    )?;
    let providers: Vec<Provider> =
        serde_json::from_slice(&output.stdout).map_err(|_| "invalid rclone provider catalog")?;
    if providers.len() > 256
        || providers.iter().any(|provider| {
            !word(&provider.name)
                || provider.description.len() > 4096
                || provider.options.len() > 256
                || provider.options.iter().any(|field| {
                    !word(&field.name) || field.help.len() > 65_536 || field.examples.len() > 256
                })
        })
    {
        return Err("rclone provider catalog exceeds limits".to_owned());
    }
    crate::broker::publish(
        directory,
        &directory.join("cloud-providers.json"),
        &serde_json::to_vec(&providers).map_err(|_| "cannot encode rclone providers")?,
    )?;
    Ok(providers)
}
fn cached_providers(directory: &Path) -> Result<Vec<Provider>, String> {
    serde_json::from_slice(&read(
        &directory.join("cloud-providers.json"),
        4 * 1024 * 1024,
    )?)
    .map_err(|_| "rclone providers unavailable; refresh providers first".to_owned())
}
fn default_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(value) => value.clone(),
        serde_json::Value::Bool(_) | serde_json::Value::Number(_) => value.to_string(),
        _ => String::new(),
    }
}
fn sensitive(field: &Field) -> bool {
    field.is_password
        || field.sensitive
        || ["password", "secret", "token", "key"]
            .iter()
            .any(|word| field.name.contains(word))
}
fn pending(directory: &Path, connection: &Connection) -> Result<Pending, String> {
    serde_json::from_slice(&read(&pending_path(directory, connection), 65_536)?)
        .map_err(|_| "invalid private cloud configuration session".to_owned())
}
fn accept(directory: &Path, connection: &Connection, output: &[u8]) -> Result<(), String> {
    let mut pending: Pending =
        serde_json::from_slice(output).map_err(|_| "invalid rclone configuration response")?;
    pending.revision = archive::identifier()?;
    if pending.state.len() > 8192
        || pending.error.len() > 16_384
        || pending.option.as_ref().is_some_and(|field| {
            !word(&field.name) || field.examples.len() > 256 || field.help.len() > 32_768
        })
    {
        return Err("rclone configuration response exceeds limits".to_owned());
    }
    crate::file_owner::private_tree(&config(directory, connection), directory)?;
    if pending.state.is_empty() {
        if !pending.error.is_empty() {
            return Err("provider did not accept the configuration".to_owned());
        }
        let mut catalog = catalog(directory)?;
        catalog
            .connections
            .iter_mut()
            .find(|entry| entry.id == connection.id)
            .ok_or("cloud connection not found")?
            .ready = true;
        save(directory, &catalog)?;
        if pending_path(directory, connection).exists() {
            std::fs::remove_file(pending_path(directory, connection))
                .map_err(|_| "cannot finish private cloud configuration session")?;
        }
    } else {
        if pending.option.is_none() {
            return Err("rclone did not supply the next configuration question".to_owned());
        }
        crate::broker::publish(
            directory,
            &pending_path(directory, connection),
            &zeroize::Zeroizing::new(
                serde_json::to_vec(&pending)
                    .map_err(|_| "cannot encode private cloud configuration session")?,
            ),
        )?;
    }
    Ok(())
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Action {
    SendGroup {
        targets: Vec<elzevir_panel::CloudTarget>,
        id: String,
    },
    Remote {
        target: elzevir_panel::CloudTarget,
        command: elzevir_panel::RemoteCommand,
    },
    Providers,
    Plan {
        definition: crate::cloud_plans::Definition,
    },
    RunPlan {
        id: String,
    },
    DisablePlan {
        id: String,
    },
    RemovePlan {
        id: String,
    },
    RemoveConnection {
        id: String,
    },
    Begin {
        name: String,
        provider: String,
        parent: Option<Parent>,
    },
    Answer {
        id: String,
        revision: String,
        value: String,
    },
    Test {
        id: String,
        folder: String,
    },
}

impl std::fmt::Debug for Action {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("CloudAction { .. }")
    }
}
impl Drop for Action {
    fn drop(&mut self) {
        if let Self::Answer { value, .. } = self {
            value.zeroize();
        }
    }
}
pub(crate) fn execute(
    directory: &Path,
    action: &Action,
    progress: Option<Progress>,
) -> Result<(), String> {
    match action {
        Action::SendGroup { targets, id } => {
            require_limited_account()?;
            let control: crate::cancellation::Control =
                std::sync::Arc::new(move || progress.as_ref().is_some_and(|p| p(&[]).is_err()));
            return crate::cloud_plans::resend(directory, targets, id, &control);
        }
        Action::Remote { target, command } => {
            return remote_files::execute(directory, target, command, progress);
        }
        Action::Plan { definition } => {
            return crate::cloud_plans::upsert(directory, definition.clone()).map(drop);
        }
        Action::DisablePlan { id } => return crate::cloud_plans::disable(directory, id),
        Action::RemovePlan { id } => return crate::cloud_plans::remove(directory, id),
        Action::RunPlan { .. } => return Err("invalid cloud backup dispatch".to_owned()),
        _ => {}
    }
    require_limited_account()?;
    let lease = crate::managed_tools::lease(crate::managed_tools::Tool::Rclone)?;
    let program = &lease.path;
    execute_with(directory, action, program, &Native, progress)
}
fn execute_with(
    directory: &Path,
    action: &Action,
    program: &Path,
    runner: &impl Runner,
    progress: Option<Progress>,
) -> Result<(), String> {
    match action {
        Action::SendGroup { .. }
        | Action::Remote { .. }
        | Action::Plan { .. }
        | Action::DisablePlan { .. }
        | Action::RemovePlan { .. }
        | Action::RunPlan { .. } => Err("invalid cloud metadata dispatch".to_owned()),
        Action::RemoveConnection { id } => {
            remove_connection(directory, id, program, runner, progress)
        }
        Action::Providers => providers_with(directory, program, runner, progress).map(drop),
        Action::Begin { .. } => begin(directory, action, program, runner, progress),
        Action::Answer {
            id,
            revision,
            value,
        } => answer(directory, id, revision, value, program, runner, progress),
        Action::Test { id, folder } => test(directory, id, folder, program, runner, progress),
    }
}

fn begin(
    directory: &Path,
    action: &Action,
    program: &Path,
    runner: &impl Runner,
    progress: Option<Progress>,
) -> Result<(), String> {
    let Action::Begin {
        name,
        provider,
        parent,
    } = action
    else {
        return Err("invalid cloud connection action".to_owned());
    };
    let parent = parent.as_ref();
    if name.trim().is_empty()
        || name.len() > 64
        || name.chars().any(char::is_control)
        || !word(provider)
    {
        return Err("invalid cloud connection name or provider".to_owned());
    }
    let providers = cached_providers(directory)
        .or_else(|_| providers_with(directory, program, runner, progress.clone()))?;
    if !providers.iter().any(|entry| entry.name == *provider) {
        return Err("unknown rclone provider".to_owned());
    }
    let mut catalog = catalog(directory)?;
    if let Some(existing) = catalog.connections.iter().find(|entry| entry.name == *name) {
        if existing.provider != *provider || existing.ready {
            return Err("existing cloud connection was preserved".to_owned());
        }
        if pending_path(directory, existing).exists() {
            return Ok(());
        }
        return advance(directory, existing, program, runner, progress);
    }
    if catalog.connections.len() >= 8 {
        return Err("cloud connection limit exceeded".to_owned());
    }
    let id = archive::identifier()?;
    let config_id = if let Some(parent) = parent {
        folder(&parent.folder)?;
        let underlying = connection(directory, &parent.id)?;
        if provider != "crypt" || !underlying.ready || underlying.parent.is_some() {
            return Err("crypt requires a ready base connection".to_owned());
        }
        underlying.config_id
    } else {
        if provider == "crypt" {
            return Err("crypt requires a managed parent connection".to_owned());
        }
        id.clone()
    };
    let connection = Connection {
        id: id.clone(),
        name: name.to_owned(),
        provider: provider.to_owned(),
        config_id,
        ready: false,
        parent: parent.cloned(),
    };
    let _lock = lock(directory, &connection)?;
    crate::backup_cli::make_directory(&directory.join("secrets"))?;
    if connection.parent.is_none() {
        archive::write_new(&config(directory, &connection), b"")?;
        crate::file_owner::private_tree(&config(directory, &connection), directory)?;
    }
    catalog.connections.push(connection.clone());
    save(directory, &catalog)?;
    advance_unlocked(directory, &connection, program, runner, progress)
}
fn parent_remote(directory: &Path, connection: &Connection) -> Result<Option<String>, String> {
    connection
        .parent
        .as_ref()
        .map(|parent| {
            let underlying = self::connection(directory, &parent.id)?;
            Ok(format!("{}:{}/", remote(&underlying), parent.folder))
        })
        .transpose()
}
fn advance(
    directory: &Path,
    connection: &Connection,
    program: &Path,
    runner: &impl Runner,
    progress: Option<Progress>,
) -> Result<(), String> {
    let _lock = lock(directory, connection)?;
    advance_unlocked(directory, connection, program, runner, progress)
}
fn advance_unlocked(
    directory: &Path,
    connection: &Connection,
    program: &Path,
    runner: &impl Runner,
    progress: Option<Progress>,
) -> Result<(), String> {
    let mut args = vec![
        "config".into(),
        "create".into(),
        remote(connection).into(),
        connection.provider.clone().into(),
        "--all".into(),
        "--non-interactive".into(),
        "config_auth_no_browser=true".into(),
    ];
    if let Some(parent) = parent_remote(directory, connection)? {
        args.extend(["remote".into(), parent.into()]);
    }
    let output = run(
        runner,
        program,
        args,
        environment(directory, connection),
        None,
        progress,
        65_536,
    )?;
    accept(directory, connection, &output.stdout)
}
fn answer(
    directory: &Path,
    id: &str,
    revision: &str,
    value: &str,
    program: &Path,
    runner: &impl Runner,
    progress: Option<Progress>,
) -> Result<(), String> {
    let connection = connection(directory, id)?;
    if connection.ready {
        return Err("cloud connection is already configured".to_owned());
    }
    let _lock = lock(directory, &connection)?;
    let pending = pending(directory, &connection)?;
    if !valid_id(revision) || pending.revision != revision {
        return Err(
            "cloud question changed; review the current question before answering".to_owned(),
        );
    }
    let field = pending
        .option
        .as_ref()
        .ok_or("no cloud configuration question pending")?;
    if value.len() > 16_384 || value.contains('\0') {
        return Err("cloud answer exceeds limit".to_owned());
    }
    let mut value = zeroize::Zeroizing::new(if value.is_empty() && !sensitive(field) {
        default_value(&field.default)
    } else {
        value.to_owned()
    });
    if field.name == "remote"
        && let Some(parent) = parent_remote(directory, &connection)?
    {
        if !value.is_empty() && *value != parent {
            return Err("crypt remote must reference its managed parent".to_owned());
        }
        *value = parent;
    }
    if field.required && value.is_empty()
        || field.exclusive
            && !value.is_empty()
            && !field.examples.iter().any(|example| example.value == *value)
    {
        return Err("answer does not match the provider question".to_owned());
    }
    if field.is_password && !value.is_empty() {
        if value.contains(['\n', '\r']) {
            return Err("password must be a single line".to_owned());
        }
        let output = run(
            runner,
            program,
            vec!["obscure".into(), "-".into()],
            environment(directory, &connection),
            Some(zeroize::Zeroizing::new(value.as_bytes().to_vec())),
            None,
            65_536,
        )?;
        let obscured = std::str::from_utf8(&output.stdout)
            .map_err(|_| "invalid password obscuring response")?
            .trim();
        value.zeroize();
        obscured.clone_into(&mut value);
    }
    let mut environment = environment(directory, &connection);
    environment.push((
        "RCLONE_STATE".to_owned(),
        zeroize::Zeroizing::new(pending.state.clone()),
    ));
    environment.push(("RCLONE_RESULT".to_owned(), value));
    let output = run(
        runner,
        program,
        vec![
            "config".into(),
            "update".into(),
            remote(&connection).into(),
            "--continue".into(),
            "--non-interactive".into(),
            "config_auth_no_browser=true".into(),
        ],
        environment,
        None,
        progress,
        65_536,
    )?;
    accept(directory, &connection, &output.stdout)
}

pub(crate) fn authorization_url(bytes: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?;
    for prefix in [
        "http://127.0.0.1:53682/auth?state=",
        "http://localhost:53682/auth?state=",
    ] {
        for (offset, _) in text.match_indices(prefix) {
            let value = text
                .get(offset..)?
                .chars()
                .take_while(|character| {
                    !character.is_whitespace()
                        && !matches!(character, '"' | '\'' | '<' | '>' | '\\')
                })
                .take(1025)
                .collect::<String>();
            let state = value.strip_prefix(prefix)?;
            if (8..=512).contains(&state.len())
                && state
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'='))
            {
                return Some(value);
            }
        }
    }
    None
}
fn destination(connection: &Connection, folder_name: &str, name: &str) -> Result<String, String> {
    folder(folder_name)?;
    if !name.starts_with("elzevir-")
        || name
            .chars()
            .any(|character| !character.is_ascii_alphanumeric() && !matches!(character, '-' | '.'))
    {
        return Err("invalid managed cloud artifact name".to_owned());
    }
    Ok(format!("{}:{folder_name}/{name}", remote(connection)))
}
fn test(
    directory: &Path,
    id: &str,
    folder_name: &str,
    program: &Path,
    runner: &impl Runner,
    progress: Option<Progress>,
) -> Result<(), String> {
    require_limited_account()?;
    let connection = connection(directory, id)?;
    if !connection.ready {
        return Err("cloud connection is not ready".to_owned());
    }
    let _lock = lock(directory, &connection)?;
    folder(folder_name)?;
    let stage = crate::private_temp::directory().map_err(|_| "cannot prepare cloud test")?;
    let name = format!("elzevir-test-{}.txt", archive::identifier()?);
    let source = stage.path().join(&name);
    archive::write_new(
        &source,
        b"Elzevir fictional upload test. No institution data.\n",
    )?;
    let control: crate::cancellation::Control = std::sync::Arc::new(|| false);
    let digest = archive::hash(&source, &control)?;
    remote_files::Session::new(directory, &connection, program, runner, progress.clone()).upload(
        folder_name,
        &source,
        &digest,
    )?;
    run(
        runner,
        program,
        vec![
            "deletefile".into(),
            destination(&connection, folder_name, &name)?.into(),
            "--retries".into(),
            "1".into(),
        ],
        environment(directory, &connection),
        None,
        progress,
        65_536,
    )?;
    crate::broker::publish(directory,&directory.join(format!("cloud-test-{id}.json")),&serde_json::to_vec(&serde_json::json!({"schema":1,"id":id,"at":jiff::Timestamp::now().to_string(),"verified":true})).map_err(|_| "cannot encode cloud test result")?)
}
pub(crate) fn upload(
    directory: &Path,
    id: &str,
    folder_name: &str,
    source: &Path,
    digest: &str,
    progress: Option<Progress>,
) -> Result<(), String> {
    require_limited_account()?;
    let connection = connection(directory, id)?;
    if !connection.ready {
        return Err("cloud connection is not ready".to_owned());
    }
    let _lock = lock(directory, &connection)?;
    let lease = crate::managed_tools::lease(crate::managed_tools::Tool::Rclone)?;
    let program = &lease.path;
    remote_files::Session::new(directory, &connection, program, &Native, progress).upload(
        folder_name,
        source,
        digest,
    )
}
pub(crate) fn inspect_object(
    directory: &Path,
    plan: &crate::cloud_plans::Plan,
    id: &str,
    digest: Option<&str>,
    progress: Option<Progress>,
) -> Result<(), String> {
    require_limited_account()?;
    let connection = connection(directory, &plan.connection)?;
    if !connection.ready {
        return Err("connection authorization pending".into());
    }
    let _lock = lock(directory, &connection)?;
    let lease = crate::managed_tools::lease(crate::managed_tools::Tool::Rclone)?;
    let name = format!("elzevir-{id}.age");
    let target = destination(&connection, &plan.folder, &name)?;
    if let Some(digest) = digest {
        return remote_files::verify_remote(
            directory,
            &connection,
            &lease.path,
            &target,
            digest,
            progress,
        );
    }
    let folder = format!("{}:{}", remote(&connection), plan.folder);
    for args in [
        vec!["mkdir".into(), OsString::from(&folder)],
        vec![
            "lsf".into(),
            folder.into(),
            "--max-depth".into(),
            "1".into(),
            "--include".into(),
            format!("/{name}").into(),
        ],
    ] {
        let output = run(
            &Native,
            &lease.path,
            args,
            environment(directory, &connection),
            None,
            progress.clone(),
            65536,
        )?;
        if !output.stdout.is_empty() {
            return Err("remote name already in use".into());
        }
    }
    Ok(())
}
pub(crate) fn remove_owned(
    directory: &Path,
    id: &str,
    folder_name: &str,
    artifact_id: &str,
    digest: &str,
    progress: Option<Progress>,
) -> Result<(), String> {
    require_limited_account()?;
    if !valid_id(artifact_id) {
        return Err("invalid managed remote artifact identity".to_owned());
    }
    let connection = connection(directory, id)?;
    if !connection.ready {
        return Err("cloud connection is not ready".to_owned());
    }
    let _lock = lock(directory, &connection)?;
    let lease = crate::managed_tools::lease(crate::managed_tools::Tool::Rclone)?;
    let program = &lease.path;
    let target = destination(
        &connection,
        folder_name,
        &format!("elzevir-{artifact_id}.age"),
    )?;
    remote_files::delete_confirmed(directory, &connection, program, &target, digest, progress)
}

fn require_limited_account() -> Result<(), String> {
    if crate::service::require_operator().is_ok() {
        return Err(
            "cloud tasks must run under the limited institution service account".to_owned(),
        );
    }
    Ok(())
}
pub(crate) fn request_path(directory: &Path, id: &str) -> Result<PathBuf, String> {
    if !valid_id(id) {
        return Err("invalid cloud request identity".to_owned());
    }
    Ok(directory.join(format!("cloud-request-{id}.json")))
}
pub(crate) fn queue(directory: &Path, id: &str, action: &Action) -> Result<(), String> {
    let bytes = zeroize::Zeroizing::new(
        serde_json::to_vec(action).map_err(|_| "cannot encode private cloud request")?,
    );
    if bytes.len() > 24_000 {
        return Err("private cloud request exceeds limit".to_owned());
    }
    archive::write_new(&request_path(directory, id)?, &bytes)
}
pub(crate) fn execute_queued(
    directory: &Path,
    id: &str,
    control: crate::cancellation::Control,
) -> Result<(), String> {
    let path = request_path(directory, id)?;
    let action: Action = serde_json::from_slice(&read(&path, 24_000)?)
        .map_err(|_| "invalid private cloud request")?;
    std::fs::remove_file(path).map_err(|_| "cannot consume private cloud request")?;
    let target = directory.to_owned();
    let job = id.to_owned();
    let progress: Progress = std::sync::Arc::new(move |bytes| {
        crate::cancellation::check(&control)?;
        if bytes.is_empty() {
            return Ok(());
        }
        if let Some(uri) = authorization_url(bytes) {
            crate::broker::publish(
                &target,
                &target.join("cloud-progress.json"),
                &serde_json::to_vec(&serde_json::json!({"id":job,"uri":uri}))
                    .map_err(|_| "cannot encode cloud authorization progress")?,
            )?;
        }
        Ok(())
    });
    let result = execute(directory, &action, Some(progress));
    if result.is_err()
        && let Action::Test { id, .. } = &action
        && valid_id(id)
    {
        let record = serde_json::json!({"schema":1,"id":id,"at":jiff::Timestamp::now().to_string(),"verified":false});
        let bytes = serde_json::to_vec(&record).map_err(|_| "cannot encode test result")?;
        crate::broker::publish(
            directory,
            &directory.join(format!("cloud-test-{id}.json")),
            &bytes,
        )?;
    }
    let path = directory.join("cloud-progress.json");
    if path.exists() {
        std::fs::remove_file(path).map_err(|_| "cannot clear cloud authorization progress")?;
    }
    result
}
pub(crate) fn authorization(directory: &Path, id: &str) -> Option<String> {
    let bytes = read(&directory.join("cloud-progress.json"), 4096).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    if value.get("id")?.as_str()? != id {
        return None;
    }
    let uri = value.get("uri")?.as_str()?;
    (authorization_url(uri.as_bytes()).as_deref() == Some(uri)).then(|| uri.to_owned())
}

pub(crate) fn view(directory: &Path) -> Result<elzevir_panel::CloudView, String> {
    let catalog = catalog(directory)?;
    let providers = cached_providers(directory).unwrap_or_default();
    let mut view = elzevir_panel::CloudView {
        providers: providers
            .iter()
            .take(256)
            .map(|provider| {
                (
                    provider.name.clone(),
                    provider.description.chars().take(256).collect(),
                )
            })
            .collect(),
        ..elzevir_panel::CloudView::default()
    };
    for connection in catalog.connections {
        view.connections.push(elzevir_panel::CloudConnection {
            id: connection.id.clone(),
            name: connection.name.clone(),
            provider: connection.provider.clone(),
            ready: connection.ready,
            test_failed: test_failed(directory, &connection.id),
            tested_at: tested_at(directory, &connection.id),
        });
        if !connection.ready
            && view.question.is_none()
            && pending_path(directory, &connection).exists()
        {
            let pending = pending(directory, &connection)?;
            if let Some(field) = &pending.option {
                let secret = sensitive(field) || field.name.contains("verification");
                let help = providers
                    .iter()
                    .find(|provider| provider.name == connection.provider)
                    .and_then(|provider| {
                        provider
                            .options
                            .iter()
                            .find(|option| option.name == field.name)
                    })
                    .map_or_else(String::new, |field| field.help.chars().take(2048).collect());
                let default = if secret {
                    String::new()
                } else if field.name == "remote" {
                    parent_remote(directory, &connection)?
                        .unwrap_or_else(|| default_value(&field.default))
                } else {
                    default_value(&field.default)
                };
                view.question = Some(elzevir_panel::CloudQuestion {
                    id: connection.id.clone(),
                    revision: pending.revision.clone(),
                    name: field.name.clone(),
                    help,
                    examples: if secret {
                        vec![]
                    } else {
                        field
                            .examples
                            .iter()
                            .take(32)
                            .map(|example| {
                                (
                                    example.value.chars().take(256).collect(),
                                    example.help.chars().take(256).collect(),
                                )
                            })
                            .collect()
                    },
                    default,
                    secret,
                    refused: !pending.error.is_empty(),
                });
            }
        }
    }
    view.remote = remote_files::page(directory);
    view.download = remote_files::downloaded(directory);
    view.notice = remote_files::notice(directory);
    view.transfers = crate::cloud_plans::history(directory)?;
    view.plans = crate::cloud_plans::summaries(directory)?;
    Ok(view)
}
fn test_failed(directory: &Path, id: &str) -> bool {
    read(&directory.join(format!("cloud-test-{id}.json")), 4096)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .is_some_and(|v| {
            v.get("verified").and_then(serde_json::Value::as_bool) == Some(false)
                && v.get("id").and_then(serde_json::Value::as_str) == Some(id)
        })
}
mod remote_files;
fn tested_at(directory: &Path, id: &str) -> Option<String> {
    let bytes = read(&directory.join(format!("cloud-test-{id}.json")), 4096).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    if value.get("schema")?.as_u64()? != 1
        || value.get("id")?.as_str()? != id
        || !value.get("verified")?.as_bool()?
    {
        return None;
    }
    let at = value.get("at")?.as_str()?;
    at.parse::<jiff::Timestamp>().ok()?;
    Some(at.to_owned())
}
pub(crate) fn intention(operation: &elzevir_panel::CloudOperation) -> Result<Action, String> {
    use elzevir_panel::CloudOperation as Panel;
    Ok(match operation {
        Panel::SendGroup(targets, id) => Action::SendGroup {
            targets: targets.clone(),
            id: id.clone(),
        },
        Panel::Remote(target, command) => Action::Remote {
            target: target.clone(),
            command: command.clone(),
        },
        Panel::Plan {
            id,
            name,
            connection,
            folder,
            keep,
            time,
            zone,
        } => Action::Plan {
            definition: crate::cloud_plans::Definition {
                id: (!id.is_empty()).then(|| id.clone()),
                name: name.clone(),
                connection: connection.clone(),
                folder: folder.clone(),
                keep: keep.parse().map_err(|_| "invalid cloud retention")?,
                daily: daily(time, zone)?,
            },
        },
        Panel::RunPlan { id } => Action::RunPlan { id: id.clone() },
        Panel::DisablePlan { id } => Action::DisablePlan { id: id.clone() },
        Panel::RemovePlan { id } => Action::RemovePlan { id: id.clone() },
        Panel::RemoveConnection { id } => Action::RemoveConnection { id: id.clone() },
        Panel::Providers => Action::Providers,
        Panel::Connect { name, provider } => Action::Begin {
            name: name.clone(),
            provider: provider.clone(),
            parent: None,
        },
        Panel::Crypt {
            name,
            parent,
            folder,
        } => Action::Begin {
            name: name.clone(),
            provider: "crypt".to_owned(),
            parent: Some(Parent {
                id: parent.clone(),
                folder: folder.clone(),
            }),
        },
        Panel::Answer {
            id,
            revision,
            value,
            ..
        } => Action::Answer {
            id: id.clone(),
            revision: revision.clone(),
            value: value.clone(),
        },
        Panel::Test { id, folder } => Action::Test {
            id: id.clone(),
            folder: folder.clone(),
        },
    })
}

pub(crate) fn daily(
    time: &str,
    zone: &str,
) -> Result<Option<crate::operations_config::Daily>, String> {
    if time.is_empty() {
        return Ok(None);
    }
    let (hour, minute) = time.split_once(':').ok_or("time must be HH:MM")?;
    Ok(Some(crate::operations_config::Daily {
        hour: hour.parse().map_err(|_| "invalid hour")?,
        minute: minute.parse().map_err(|_| "invalid minute")?,
        time_zone: zone.to_owned(),
    }))
}
fn remove_connection(
    directory: &Path,
    id: &str,
    program: &Path,
    runner: &impl Runner,
    progress: Option<Progress>,
) -> Result<(), String> {
    let connection = connection(directory, id)?;
    let mut catalog = catalog(directory)?;
    if catalog
        .connections
        .iter()
        .any(|entry| entry.parent.as_ref().is_some_and(|parent| parent.id == id))
        || crate::cloud_plans::load(directory)?
            .iter()
            .any(|plan| plan.connection == id)
    {
        return Err("connection is still used by a crypt wrapper or cloud plan".to_owned());
    }
    let _lock = lock(directory, &connection)?;
    if connection.parent.is_some() {
        run(
            runner,
            program,
            vec!["config".into(), "delete".into(), remote(&connection).into()],
            environment(directory, &connection),
            None,
            progress,
            65_536,
        )?;
    } else if config(directory, &connection).exists() {
        std::fs::remove_file(config(directory, &connection))
            .map_err(|_| "cannot remove managed cloud credentials")?;
    }
    if pending_path(directory, &connection).exists() {
        std::fs::remove_file(pending_path(directory, &connection))
            .map_err(|_| "cannot remove private cloud configuration state")?;
    }
    catalog.connections.retain(|entry| entry.id != id);
    save(directory, &catalog)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    struct Fake(RefCell<Vec<Request>>);
    impl Runner for Fake {
        fn run(&self, request: Request) -> Result<Output, String> {
            let output = if request.args.first().is_some_and(|value| value == "obscure") {
                assert_eq!(
                    request.input.as_deref().map(Vec::as_slice),
                    Some(b"fictional-password".as_slice())
                );
                b"fictional_obscured_password_0123456789\n".to_vec()
            } else {
                assert!(
                    !request
                        .args
                        .iter()
                        .any(|value| value.to_string_lossy().contains("fictional-password"))
                );
                let environment = request.environment.as_ref().unwrap();
                assert_eq!(
                    environment
                        .iter()
                        .find(|(name, _)| name == "RCLONE_RESULT")
                        .unwrap()
                        .1
                        .as_str(),
                    "fictional_obscured_password_0123456789"
                );
                br#"{"State":"","Option":null,"Error":""}"#.to_vec()
            };
            self.0.borrow_mut().push(request);
            Ok(Output {
                success: true,
                stdout: output,
            })
        }
    }
    #[test]
    fn provider_password_uses_private_stdin_then_private_environment_without_argv() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join("secrets")).unwrap();
        let connection = Connection {
            id: "a".repeat(32),
            name: "Fictional SFTP".to_owned(),
            provider: "sftp".to_owned(),
            config_id: "a".repeat(32),
            ready: false,
            parent: None,
        };
        archive::write_new(
            &config(directory.path(), &connection),
            b"[fictional]\ntype = sftp\n",
        )
        .unwrap();
        save(
            directory.path(),
            &Catalog {
                connections: vec![connection.clone()],
            },
        )
        .unwrap();
        crate::broker::publish(directory.path(),&pending_path(directory.path(),&connection),br#"{"Revision":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","State":"*all-set,1,false","Option":{"Name":"pass","IsPassword":true,"Required":true},"Error":""}"#).unwrap();
        let runner = Fake(RefCell::new(Vec::new()));
        answer(
            directory.path(),
            &connection.id,
            &"b".repeat(32),
            "fictional-password",
            Path::new("fixture-rclone"),
            &runner,
            None,
        )
        .unwrap();
        assert!(
            self::connection(directory.path(), &connection.id)
                .unwrap()
                .ready
        );
        assert!(!pending_path(directory.path(), &connection).exists());
        assert_eq!(runner.0.borrow().len(), 2);
    }
    #[test]
    fn opaque_provider_state_secret_defaults_and_dynamic_help_never_reach_the_view() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join("secrets")).unwrap();
        let connection = Connection {
            id: "a".repeat(32),
            name: "Fictional OAuth".to_owned(),
            provider: "drive".to_owned(),
            config_id: "a".repeat(32),
            ready: false,
            parent: None,
        };
        save(
            directory.path(),
            &Catalog {
                connections: vec![connection.clone()],
            },
        )
        .unwrap();
        crate::broker::publish(directory.path(),&pending_path(directory.path(),&connection),br#"{"Revision":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","State":"opaque-fictional-private-state","Option":{"Name":"config_token","Help":"rclone authorize secret-blob","Default":"fictional-private-token","Examples":[{"Value":"private-example","Help":"private-help"}]},"Error":""}"#).unwrap();
        let view = view(directory.path()).unwrap();
        let encoded = serde_json::to_string(&view).unwrap();
        for private in [
            "opaque-fictional-private-state",
            "secret-blob",
            "fictional-private-token",
            "private-example",
            "private-help",
        ] {
            assert!(!encoded.contains(private));
        }
        assert!(view.question.as_ref().unwrap().secret);
        let runner = Fake(RefCell::new(Vec::new()));
        assert!(
            answer(
                directory.path(),
                &connection.id,
                &"c".repeat(32),
                "fictional-answer",
                Path::new("fixture-rclone"),
                &runner,
                None
            )
            .unwrap_err()
            .contains("question changed")
        );
        assert!(runner.0.borrow().is_empty());
    }
    #[test]
    fn destination_paths_never_escape_the_plans_owned_subdirectory() {
        for value in [
            "",
            "/absolute",
            "../other",
            "ok/../../other",
            "remote:other",
            "ok\\other",
            "ok/",
            "ok//other",
        ] {
            assert!(folder(value).is_err());
        }
        folder("Elzevir/library-one").unwrap();
    }
    #[test]
    fn oauth_progress_exposes_only_the_local_authorization_link() {
        assert_eq!(
            authorization_url(b"Open http://127.0.0.1:53682/auth?state=fictional-state\n")
                .as_deref(),
            Some("http://127.0.0.1:53682/auth?state=fictional-state")
        );
        for value in [
            "https://example.org/?access_token=fictional",
            "http://127.0.0.1:1234/auth?state=fictional",
            "http://127.0.0.1:53682/auth?state=a&token=secret",
        ] {
            assert!(authorization_url(value.as_bytes()).is_none());
        }
    }
}
