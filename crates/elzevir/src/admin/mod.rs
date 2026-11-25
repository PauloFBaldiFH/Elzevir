// SPDX-License-Identifier: GPL-3.0-or-later
//! Instance-bound, bounded administrative protocol on the OS-private transport.

pub(crate) mod transport;

use crate::{
    health::{Integrity, Probe, Report},
    runtime_info::RuntimeInfo,
};
use interprocess::local_socket::tokio::{Stream, prelude::*};
use serde::{Deserialize, Serialize};
use std::{io, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader},
    task::JoinSet,
    time::timeout,
};

const DEADLINE: Duration = Duration::from_secs(2);
const ACCOUNT_DEADLINE: Duration = Duration::from_secs(30);
const MAX_FRAME: usize = 32_768;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: u8,
    instance: String,
    id: u64,
    command: Command,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Command {
    Status,
    CreateAdministrator {
        login: String,
        password: String,
    },
    Diagnose,
    BackupNow {
        #[serde(default)]
        options: elzevir_panel::BackupOptions,
    },
    ConfigureCloud {
        action: crate::cloud::Action,
    },
    Operations,
    Sessions,
    VerifyBackup {
        id: String,
    },
    BackupPolicy {
        policy: crate::operations_config::Policy,
    },
    /// Ordered shutdown, for a server running in the background.
    Stop,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Response {
    schema: u8,
    instance: String,
    id: Option<u64>,
    result: Outcome,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Outcome {
    Status { health: Report },
    Diagnose { pid: u32, quick_check: Integrity },
    Sessions { count: usize },
    Job { job: crate::operations::Job },
    Operations { view: crate::operations::View },
    AdministratorCreated,
    Stopping,
    Error { code: ErrorCode },
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ErrorCode {
    InvalidRequest,
    UnsupportedSchema,
    InstanceMismatch,
    OperationRefused,
}

async fn frame(reader: impl AsyncRead + Unpin, limit: usize) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    BufReader::new(
        reader.take(u64::try_from(limit + 1).map_err(|_| io::Error::other("invalid frame limit"))?),
    )
    .read_until(b'\n', &mut bytes)
    .await?;
    if bytes.len() > limit || bytes.last() != Some(&b'\n') {
        return Err(io::Error::other("invalid administrative frame"));
    }
    Ok(bytes)
}

async fn handle(
    stream: Stream,
    probe: Arc<Probe>,
    operations: crate::operations::Client,
    sessions: elzevir_web::AppState,
    control: crate::shutdown::Control,
) -> io::Result<()> {
    // Collapse decoding failures to a static code; never echo untrusted frame contents.
    let request = timeout(DEADLINE, frame(&stream, MAX_FRAME))
        .await
        .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))?
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Request>(&zeroize::Zeroizing::new(bytes)).ok());
    let id = request.as_ref().map(|request| request.id);
    let result = match request {
        None => Outcome::Error {
            code: ErrorCode::InvalidRequest,
        },
        Some(request) if request.schema != 1 => Outcome::Error {
            code: ErrorCode::UnsupportedSchema,
        },
        Some(request) if request.instance != probe.instance() => Outcome::Error {
            code: ErrorCode::InstanceMismatch,
        },
        Some(Request {
            command: Command::CreateAdministrator { login, password },
            ..
        }) => {
            let password = zeroize::Zeroizing::new(password);
            match operations.create_administrator(login, password).await {
                Ok(()) => Outcome::AdministratorCreated,
                Err(()) => Outcome::Error {
                    code: ErrorCode::OperationRefused,
                },
            }
        }
        Some(Request {
            command: Command::Status,
            ..
        }) => Outcome::Status {
            health: probe.snapshot().await,
        },
        Some(Request {
            command: Command::Diagnose,
            ..
        }) => Outcome::Diagnose {
            pid: std::process::id(),
            quick_check: probe.integrity().await,
        },
        Some(Request {
            command: Command::Sessions,
            ..
        }) => Outcome::Sessions {
            count: sessions.staff_session_count(),
        },
        Some(Request {
            command: Command::BackupNow { options },
            ..
        }) => operation_result(
            operations
                .submit(crate::operations::Task::Backup { options })
                .map(|job| Outcome::Job { job }),
        ),
        Some(Request {
            command: Command::VerifyBackup { id },
            ..
        }) => operation_result(
            operations
                .submit(crate::operations::Task::Verify { id })
                .map(|job| Outcome::Job { job }),
        ),
        Some(Request {
            command: Command::ConfigureCloud { action },
            ..
        }) => operation_result(operations.cloud(&action).map(|job| Outcome::Job { job })),
        Some(Request {
            command: Command::Operations,
            ..
        }) => operation_result(operations.view().map(|view| Outcome::Operations { view })),
        Some(Request {
            command: Command::BackupPolicy { policy },
            ..
        }) => operation_result(
            operations
                .policy(&policy)
                .map(|view| Outcome::Operations { view }),
        ),
        Some(Request {
            command: Command::Stop,
            ..
        }) => {
            // The same durable request as a console signal: the reply is
            // written before the listeners close.
            control.request();
            Outcome::Stopping
        }
    };
    let response = Response {
        schema: 1,
        instance: probe.instance().to_owned(),
        id,
        result,
    };
    let mut bytes = serde_json::to_vec(&response)
        .map_err(|_| io::Error::other("cannot encode administrative response"))?;
    bytes.push(b'\n');
    let mut writer = &stream;
    writer.write_all(&bytes).await?;
    writer.flush().await
}

pub(crate) async fn serve(
    bound: transport::Bound,
    probe: Arc<Probe>,
    operations: crate::operations::Client,
    sessions: elzevir_web::AppState,
    control: crate::shutdown::Control,
) -> io::Result<()> {
    let mut shutdown = control.receiver();
    let mut pending = JoinSet::new();
    loop {
        tokio::select! {
            _ = shutdown.wait_for(|stopping| *stopping) => break,
            accepted = bound.listener.accept() => {
                let stream = accepted?;
                if pending.len() < 8 {
                    let probe = Arc::clone(&probe);
                    let operations = operations.clone();
                    let sessions=sessions.clone();
                    let control = control.clone();
                    pending.spawn(async move { timeout(ACCOUNT_DEADLINE, handle(stream, probe, operations, sessions, control)).await });
                }
                // Dropping excess connections applies a fixed resource bound.
            }
            Some(completed) = pending.join_next(), if !pending.is_empty() => {
                // Timeouts and peer disconnects are contained to that connection.
                if completed.is_err() {
                    crate::logging::emit(probe.instance(), crate::logging::Event::AdminHandlerFailed);
                }
            }
        }
    }
    pending.abort_all();
    while pending.join_next().await.is_some() {}
    Ok(())
}

pub(crate) fn status(info: &RuntimeInfo) -> Result<Report, String> {
    match call(info, Command::Status)? {
        Outcome::Status { health }
            if health.schema == 1 && health.instance == info.instance && health.pid == info.pid =>
        {
            Ok(health)
        }
        _ => Err("administrative command refused".to_owned()),
    }
}

pub(crate) fn create_administrator(
    info: &RuntimeInfo,
    login: &str,
    password: &str,
) -> Result<(), String> {
    match call(
        info,
        Command::CreateAdministrator {
            login: login.into(),
            password: password.into(),
        },
    )? {
        Outcome::AdministratorCreated => Ok(()),
        _ => Err("administrator creation refused".into()),
    }
}

/// Asks a running server for an ordered shutdown.
pub(crate) fn stop(info: &RuntimeInfo) -> Result<(), String> {
    match call(info, Command::Stop)? {
        Outcome::Stopping => Ok(()),
        _ => Err("stop request refused".to_owned()),
    }
}

pub(crate) fn diagnose(info: &RuntimeInfo) -> Result<Integrity, String> {
    match call(info, Command::Diagnose)? {
        Outcome::Diagnose { pid, quick_check } if pid == info.pid => Ok(quick_check),
        _ => Err("administrative command refused".to_owned()),
    }
}

fn call(info: &RuntimeInfo, command: Command) -> Result<Outcome, String> {
    let deadline = if matches!(command, Command::CreateAdministrator { .. }) {
        ACCOUNT_DEADLINE
    } else {
        DEADLINE
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "cannot initialize administrative client")?;
    runtime
        .block_on(async {
            timeout(deadline, query(info, command))
                .await
                .map_err(|_| "administrative request timed out")?
        })
        .map_err(|_| "administrative channel unavailable or incompatible".to_owned())
}

async fn query(info: &RuntimeInfo, command: Command) -> Result<Outcome, String> {
    let endpoint = info
        .admin
        .as_ref()
        .ok_or("administrative endpoint not announced")?;
    if !endpoint.valid(&info.instance) {
        return Err("invalid administrative endpoint".to_owned());
    }
    let stream = endpoint
        .connect()
        .await
        .map_err(|_| "administrative connection refused")?;
    let request = Request {
        schema: 1,
        instance: info.instance.clone(),
        id: 1,
        command,
    };
    let mut bytes = zeroize::Zeroizing::new(
        serde_json::to_vec(&request).map_err(|_| "cannot encode administrative request")?,
    );
    bytes.push(b'\n');
    let mut writer = &stream;
    writer
        .write_all(&bytes)
        .await
        .map_err(|_| "cannot write administrative request")?;
    writer
        .flush()
        .await
        .map_err(|_| "cannot flush administrative request")?;
    let bytes = frame(&stream, 65_536)
        .await
        .map_err(|_| "cannot read administrative response")?;
    let response: Response =
        serde_json::from_slice(&bytes).map_err(|_| "invalid administrative response")?;
    if response.schema != 1 || response.instance != info.instance || response.id != Some(1) {
        return Err("administrative response identity mismatch".to_owned());
    }
    Ok(response.result)
}

fn operation_result(result: Result<Outcome, String>) -> Outcome {
    result.unwrap_or(Outcome::Error {
        code: ErrorCode::OperationRefused,
    })
}
pub(crate) fn start_backup(info: &RuntimeInfo) -> Result<crate::operations::Job, String> {
    start_selected_backup(info, elzevir_panel::BackupOptions::default())
}
pub(crate) fn start_selected_backup(
    info: &RuntimeInfo,
    options: elzevir_panel::BackupOptions,
) -> Result<crate::operations::Job, String> {
    match call(info, Command::BackupNow { options })? {
        Outcome::Job { job } => Ok(job),
        _ => Err("backup job refused".to_owned()),
    }
}
pub(crate) fn operations(info: &RuntimeInfo) -> Result<crate::operations::View, String> {
    match call(info, Command::Operations)? {
        Outcome::Operations { view } => Ok(view),
        _ => Err("operational observation refused".to_owned()),
    }
}
pub(crate) fn verify_backup(
    info: &RuntimeInfo,
    id: String,
) -> Result<crate::operations::Job, String> {
    match call(info, Command::VerifyBackup { id })? {
        Outcome::Job { job } => Ok(job),
        _ => Err("verification job refused".to_owned()),
    }
}
pub(crate) fn backup_policy(
    info: &RuntimeInfo,
    policy: crate::operations_config::Policy,
) -> Result<crate::operations::View, String> {
    match call(info, Command::BackupPolicy { policy })? {
        Outcome::Operations { view } => Ok(view),
        _ => Err("backup policy refused".to_owned()),
    }
}

pub(crate) fn sessions(info: &RuntimeInfo) -> Result<usize, String> {
    match call(info, Command::Sessions)? {
        Outcome::Sessions { count } => Ok(count),
        _ => Err("session observation unavailable".to_owned()),
    }
}

pub(crate) fn cloud(
    info: &RuntimeInfo,
    action: crate::cloud::Action,
) -> Result<crate::operations::Job, String> {
    match call(info, Command::ConfigureCloud { action })? {
        Outcome::Job { job } => Ok(job),
        _ => Err("cloud configuration request refused".to_owned()),
    }
}
