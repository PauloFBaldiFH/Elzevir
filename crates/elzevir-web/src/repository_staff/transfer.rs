// SPDX-License-Identifier: GPL-3.0-or-later
use super::{AppState, Failure, MAX_FILE, authorized, bad, csrf, error, id, r, redirect, work};
use axum::{
    body::Bytes,
    extract::{Multipart, Path, Query, State, multipart::MultipartError},
    http::{HeaderMap, Method, StatusCode},
    response::Response,
};
use serde::Deserialize;
use std::{
    future::Future,
    io::{self, Read},
    time::Duration,
};
use tokio::sync::mpsc;

async fn part<T>(read: impl Future<Output = Result<T, MultipartError>>) -> Result<T, Failure> {
    tokio::time::timeout(Duration::from_secs(90), read)
        .await
        .map_err(|_| Failure(StatusCode::REQUEST_TIMEOUT, "upload_timeout"))?
        .map_err(|e| Failure(e.status(), "invalid_multipart"))
}

async fn parameter(form: &mut Multipart, name: &str) -> Result<String, Failure> {
    let mut field = part(form.next_field()).await?.ok_or_else(bad)?;
    if field.name() != Some(name) || field.file_name().is_some() {
        return Err(bad());
    }
    let mut value = Vec::new();
    while let Some(chunk) = part(field.chunk()).await? {
        if value.len() + chunk.len() > 128 {
            return Err(bad());
        }
        value.extend_from_slice(&chunk);
    }
    String::from_utf8(value).map_err(|_| bad())
}

// Only a complete multipart closing boundary may be translated to EOF for the object store.
enum Packet {
    Data(Bytes),
    End,
}
struct UploadReader {
    receiver: mpsc::Receiver<Packet>,
    pending: Bytes,
    finished: bool,
}
impl Read for UploadReader {
    fn read(&mut self, dest: &mut [u8]) -> io::Result<usize> {
        if self.finished || dest.is_empty() {
            return Ok(0);
        }
        while self.pending.is_empty() {
            match self.receiver.blocking_recv() {
                Some(Packet::Data(bytes)) => self.pending = bytes,
                Some(Packet::End) => {
                    self.finished = true;
                    return Ok(0);
                }
                None => return Err(io::ErrorKind::UnexpectedEof.into()),
            }
        }
        let count = dest.len().min(self.pending.len());
        dest.get_mut(..count)
            .ok_or(io::ErrorKind::InvalidInput)?
            .copy_from_slice(&self.pending.split_to(count));
        Ok(count)
    }
}

pub(super) async fn upload(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((deposit, file)): Path<(String, String)>,
    mut form: Multipart,
) -> Result<Response, Failure> {
    let (desk, actor, token) = authorized(&state, &headers)?;
    let deposit = r::DepositId(id(&deposit)?);
    let file = r::FileId(id(&file)?);
    let permit = desk
        .upload
        .try_acquire_owned()
        .map_err(|_| error(r::RepositoryError::UploadBusy))?;
    csrf(&token, &parameter(&mut form, "csrf").await?)?;
    let operation = r::OperationId(id(&parameter(&mut form, "operation").await?)?);
    let expected = parameter(&mut form, "expected")
        .await?
        .parse::<u64>()
        .map_err(|_| bad())?;
    let field = part(form.next_field()).await?.ok_or_else(bad)?;
    if field.name() != Some("file") {
        return Err(bad());
    }
    let name = field
        .file_name()
        .and_then(|s| s.rsplit(['/', '\\']).next())
        .filter(|s| !s.is_empty() && s.len() <= 255 && !s.chars().any(char::is_control))
        .ok_or_else(bad)?;
    let upload = r::Upload {
        deposit,
        file,
        operation,
        expected_current_seq: (expected != 0).then_some(expected),
        display_name: name.to_owned(),
        media_type: field
            .content_type()
            .unwrap_or("application/octet-stream")
            .to_owned(),
    };
    let (sender, receiver) = mpsc::channel(1);
    let worker = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        desk.port.upload(
            actor,
            upload,
            &mut UploadReader {
                receiver,
                pending: Bytes::new(),
                finished: false,
            },
        )
    });
    let forwarded = forward(field, &sender).await;
    let completed = match forwarded {
        Ok(true) => match part(form.next_field()).await {
            Ok(None) => {
                drop(sender.send(Packet::End).await);
                Ok(())
            }
            Ok(Some(_)) => Err(bad()),
            Err(error) => Err(error),
        },
        other => other.map(drop),
    };
    drop(sender);
    let result = worker
        .await
        .map_err(|_| error(r::RepositoryError::Unavailable))?;
    completed?;
    result.map_err(error)?;
    Ok(redirect(deposit))
}

// False means the installer already returned an error; preserve its diagnostic.
async fn forward(
    mut field: axum::extract::multipart::Field<'_>,
    sender: &mpsc::Sender<Packet>,
) -> Result<bool, Failure> {
    let mut total = 0usize;
    while let Some(chunk) = part(field.chunk()).await? {
        total = total
            .checked_add(chunk.len())
            .filter(|size| *size <= MAX_FILE)
            .ok_or_else(|| error(r::RepositoryError::TooLarge))?;
        if sender.send(Packet::Data(chunk)).await.is_err() {
            return Ok(false);
        }
    }
    Ok(true)
}

#[derive(Deserialize)]
pub(super) struct Revision {
    seq: u64,
}

pub(super) async fn download(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((deposit, file)): Path<(String, String)>,
    Query(revision): Query<Revision>,
    method: Method,
) -> Result<Response, Failure> {
    let (desk, actor, _) = authorized(&state, &headers)?;
    if revision.seq == 0 {
        return Err(bad());
    }
    let version = r::VersionRef {
        deposit: r::DepositId(id(&deposit)?),
        file: r::FileId(id(&file)?),
        seq: revision.seq,
    };
    let permit = desk
        .reads
        .try_acquire_owned()
        .map_err(|_| error(r::RepositoryError::Unavailable))?;
    let (download, permit) =
        work(move || desk.port.download(actor, version).map(|d| (d, permit))).await?;
    Ok(crate::repository::download_response(
        download, permit, &method,
    ))
}
