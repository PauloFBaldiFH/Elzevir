// SPDX-License-Identifier: GPL-3.0-or-later
//! Anonymous OPAC transport. Every request resolves the link before opening bytes.
use crate::{AppState, render};
use axum::{
    Router,
    body::Body,
    extract::{Path, State},
    http::{Method, StatusCode, header},
    middleware,
    response::{IntoResponse, Response},
    routing::get,
};
use elzevir_app::repository::{DepositId, FileId, PublicRepository, RepositoryError, VersionRef};
use std::{
    io::{self, Read},
    sync::Arc,
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

#[derive(Clone)]
pub(super) struct Access {
    pub(super) port: Arc<dyn PublicRepository>,
    slots: Arc<Semaphore>,
}
impl Access {
    pub(super) fn new(port: Arc<dyn PublicRepository>) -> Self {
        Self {
            port,
            slots: Arc::new(Semaphore::new(2)),
        }
    }
    pub(super) fn slot(&self) -> Result<OwnedSemaphorePermit, StatusCode> {
        Arc::clone(&self.slots)
            .try_acquire_owned()
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)
    }
}
pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/repository", get(crate::repository_discovery::search))
        .route("/repository/{deposit}", get(page))
        .route(
            "/repository/{deposit}/files/{file}/versions/{seq}",
            get(download),
        )
        .layer(middleware::map_response(
            |mut response: Response| async move {
                response.headers_mut().insert(
                    header::CACHE_CONTROL,
                    header::HeaderValue::from_static("no-store"),
                );
                response
            },
        ))
}

pub(super) fn id(value: &str) -> Result<u128, StatusCode> {
    if value.len() != 32
        || !value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        return Err(StatusCode::NOT_FOUND);
    }
    u128::from_str_radix(value, 16).map_err(|_| StatusCode::NOT_FOUND)
}
fn status(error: RepositoryError) -> StatusCode {
    match error {
        RepositoryError::Withdrawn => StatusCode::GONE,
        RepositoryError::NotVisible => StatusCode::NOT_FOUND,
        _ => StatusCode::SERVICE_UNAVAILABLE,
    }
}
async fn page(
    State(state): State<AppState>,
    Path(deposit): Path<String>,
) -> Result<Response, StatusCode> {
    let deposit = DepositId(id(&deposit)?);
    let access = state.repository.as_ref().ok_or(StatusCode::NOT_FOUND)?;
    let permit = access.slot()?;
    let port = Arc::clone(&access.port);
    let page = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        port.page(deposit)
    })
    .await
    .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
    .map_err(status)?;
    let files = page
        .files
        .into_iter()
        .filter_map(|(file, revision)| {
            let v = revision?;
            Some((
                file.label,
                format!(
                    "/repository/{:032x}/files/{:032x}/versions/{}",
                    v.deposit.0, v.file.0, v.seq
                ),
            ))
        })
        .collect();
    Ok(render(&elzevir_web_templates::RepositoryPage {
        t: &state.inner.i18n,
        instance: page.instance.0,
        files,
    }))
}

async fn download(
    State(state): State<AppState>,
    Path((deposit, file, seq)): Path<(String, String, String)>,
    method: Method,
) -> Result<Response, StatusCode> {
    let seq = seq
        .parse::<u64>()
        .ok()
        .filter(|n| *n > 0 && n.to_string() == seq)
        .ok_or(StatusCode::NOT_FOUND)?;
    let version = VersionRef {
        deposit: DepositId(id(&deposit)?),
        file: FileId(id(&file)?),
        seq,
    };
    let access = state.repository.as_ref().ok_or(StatusCode::NOT_FOUND)?;
    let permit = access.slot()?;
    let port = Arc::clone(&access.port);
    let (download, permit) =
        tokio::task::spawn_blocking(move || port.download(version).map(|d| (d, permit)))
            .await
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
            .map_err(status)?;
    Ok(download_response(download, permit, &method))
}

pub(super) fn download_response(
    download: elzevir_app::repository::Download,
    permit: OwnedSemaphorePermit,
    method: &Method,
) -> Response {
    let name = download
        .name
        .rsplit(['/', '\\'])
        .next()
        .filter(|s| !s.is_empty() && s.len() <= 255 && !s.chars().any(char::is_control))
        .unwrap_or("download");
    let disposition = format!(
        "attachment; filename*=UTF-8''{}",
        elzevir_web_templates::url_encode(name)
    );
    let body = if method == Method::HEAD {
        Body::empty()
    } else {
        stream(download.reader.take(download.bytes), permit)
    };
    (
        [
            (header::CONTENT_TYPE, "application/octet-stream".to_owned()),
            (header::CONTENT_DISPOSITION, disposition),
            (header::CONTENT_LENGTH, download.bytes.to_string()),
        ],
        body,
    )
        .into_response()
}

fn stream(reader: io::Take<Box<dyn Read + Send>>, permit: OwnedSemaphorePermit) -> Body {
    // One blocking read per demand from the socket. Dropping the body releases its slot.
    Body::from_stream(futures_util::stream::try_unfold(
        (reader, permit),
        |(mut reader, permit)| async move {
            if reader.limit() == 0 {
                return Ok(None);
            }
            tokio::task::spawn_blocking(move || {
                let mut bytes = vec![
                    0;
                    usize::try_from(reader.limit().min(256 * 1024))
                        .map_err(io::Error::other)?
                ];
                let count = reader.read(&mut bytes)?;
                if count == 0 {
                    return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
                }
                bytes.truncate(count);
                Ok(Some((bytes, (reader, permit))))
            })
            .await
            .map_err(io::Error::other)?
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt as _;

    #[tokio::test]
    async fn transfer_is_bounded_and_dropping_the_body_releases_its_slot() {
        let slots = Arc::new(Semaphore::new(1));
        let reader: Box<dyn Read + Send> = Box::new(io::repeat(42));
        let mut body = stream(
            reader.take(1024 * 1024),
            Arc::clone(&slots).try_acquire_owned().unwrap(),
        );
        assert_eq!(slots.available_permits(), 0);
        let frame = body.frame().await.unwrap().unwrap().into_data().unwrap();
        assert_eq!(frame.len(), 256 * 1024);
        assert_eq!(slots.available_permits(), 0);
        drop(body);
        assert_eq!(slots.available_permits(), 1);
    }
}
