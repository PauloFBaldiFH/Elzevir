// SPDX-License-Identifier: GPL-3.0-or-later
//! Cataloguer-only interface for a single institution.
mod transfer;
use crate::{AppState, render, security::tokens_match, staff::current_session};
use axum::{
    Form, Router,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    middleware,
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use elzevir_app::repository as r;
use elzevir_web_templates::{StaffDeposit, StaffDepositFile};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::Semaphore;

#[derive(Clone)]
pub(super) struct Desk {
    port: Arc<dyn r::RepositoryDesk>,
    upload: Arc<Semaphore>,
    reads: Arc<Semaphore>,
}
impl Desk {
    pub(super) fn new(port: Arc<dyn r::RepositoryDesk>) -> Self {
        Self {
            port,
            upload: Arc::new(Semaphore::new(1)),
            reads: Arc::new(Semaphore::new(2)),
        }
    }
}
const ROOT: &str = "/equipe/repositorio";
const MAX_FILE: usize = 1 << 30;
#[derive(Clone, Copy)]
struct Failure(StatusCode, &'static str);
impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        (self.0, axum::Extension(self), ()).into_response()
    }
}
fn bad() -> Failure {
    Failure(StatusCode::BAD_REQUEST, "invalid_request")
}
fn error(e: r::RepositoryError) -> Failure {
    use r::RepositoryError as E;
    let status = match e {
        E::InstanceNotFound | E::NotVisible => StatusCode::NOT_FOUND,
        E::PermissionDenied => StatusCode::FORBIDDEN,
        E::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
        E::InsufficientSpace => StatusCode::INSUFFICIENT_STORAGE,
        E::ChecksumMismatch => StatusCode::UNPROCESSABLE_ENTITY,
        E::Unavailable | E::ObjectUnavailable | E::CommitUnknown | E::UploadBusy => {
            StatusCode::SERVICE_UNAVAILABLE
        }
        _ => StatusCode::CONFLICT,
    };
    Failure(status, e.code())
}
async fn work<T: Send + 'static>(
    run: impl FnOnce() -> r::Result<T> + Send + 'static,
) -> Result<T, Failure> {
    tokio::task::spawn_blocking(run)
        .await
        .map_err(|_| error(r::RepositoryError::Unavailable))?
        .map_err(error)
}
fn id(text: &str) -> Result<u128, Failure> {
    crate::repository::id(text).map_err(|_| bad())
}
fn fresh() -> Result<u128, Failure> {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes).map_err(|_| error(r::RepositoryError::Unavailable))?;
    Ok(u128::from_be_bytes(bytes))
}
fn authorized(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(Desk, r::Operator, String), Failure> {
    let session = current_session(state, headers)
        .ok_or(Failure(StatusCode::UNAUTHORIZED, "login_required"))?;
    let actor = r::Operator::try_from(&session.identity).map_err(error)?;
    let desk = state
        .desk
        .clone()
        .ok_or(Failure(StatusCode::NOT_FOUND, "not_visible"))?;
    Ok((desk, actor, session.csrf))
}
fn csrf(expected: &str, supplied: &str) -> Result<(), Failure> {
    if tokens_match(expected, supplied) {
        Ok(())
    } else {
        Err(Failure(StatusCode::FORBIDDEN, "invalid_csrf"))
    }
}
pub(super) fn routes(state: &AppState) -> Router<AppState> {
    let inner = Arc::clone(&state.inner);
    Router::new()
        .route(ROOT, get(home).post(draft))
        .route("/equipe/repositorio/{deposit}", get(view).post(change))
        .route(
            "/equipe/repositorio/{deposit}/files/{file}/upload",
            post(transfer::upload).layer(DefaultBodyLimit::max(MAX_FILE + 65536)),
        )
        .route(
            "/equipe/repositorio/{deposit}/files/{file}/download",
            get(transfer::download),
        )
        .layer(middleware::map_response(move |mut response: Response| {
            let inner = Arc::clone(&inner);
            async move {
                if let Some(failure) = response.extensions().get::<Failure>() {
                    response = (
                        failure.0,
                        inner.i18n.get(&format!("desk-error-{}", failure.1)),
                    )
                        .into_response();
                }
                response.headers_mut().insert(
                    header::CACHE_CONTROL,
                    header::HeaderValue::from_static("no-store"),
                );
                response
            }
        }))
}
fn redirect(id: r::DepositId) -> Response {
    Redirect::to(&format!("{ROOT}/{:032x}", id.0)).into_response()
}
#[derive(Default, Deserialize)]
struct Lookup {
    instance: Option<i64>,
    #[serde(default)]
    filter: String,
}
async fn home(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<Lookup>,
) -> Result<Response, Failure> {
    let (desk, actor, token) = authorized(&state, &headers)?;
    let title = if let Some(instance) = q.instance {
        let target = work(move || desk.port.find(actor, r::InstanceId(instance))).await?;
        if let Some(id) = target.deposit {
            return Ok(redirect(id));
        }
        target.title
    } else {
        String::new()
    };
    Ok(render(&StaffDeposit {
        t: &state.inner.i18n,
        csrf: token,
        instance: q.instance,
        title,
        opac_hidden: false,
        id: None,
        state_key: "",
        files: vec![],
        collections: vec![],
        filter: String::new(),
    }))
}
#[derive(Deserialize)]
struct Draft {
    instance: i64,
    csrf: String,
}
async fn draft(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(f): Form<Draft>,
) -> Result<Response, Failure> {
    let (desk, actor, token) = authorized(&state, &headers)?;
    csrf(&token, &f.csrf)?;
    let id = r::DepositId(fresh()?);
    work(move || desk.port.draft(actor, id, r::InstanceId(f.instance))).await?;
    Ok(redirect(id))
}
async fn view(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(key): Path<String>,
    Query(q): Query<Lookup>,
) -> Result<Response, Failure> {
    let (desk, actor, token) = authorized(&state, &headers)?;
    let deposit = r::DepositId(id(&key)?);
    if q.filter.len() > 1024 {
        return Err(bad());
    }
    let filter = q.filter.clone();
    let data = work(move || desk.port.view(actor, deposit, &filter)).await?;
    let state_key = match data.deposit.state {
        r::DepositState::Draft => "desk-draft",
        r::DepositState::Published => "desk-published",
        r::DepositState::Withdrawn => "desk-withdrawn",
    };
    let files = data
        .deposit
        .files
        .into_values()
        .map(|f| {
            Ok(StaffDepositFile {
                id: format!("{:032x}", f.description.id.0),
                label: f.description.label,
                role: match f.description.role {
                    r::FileRole::Primary => "desk-primary",
                    r::FileRole::Supplement => "desk-supplement",
                    r::FileRole::Administrative => "desk-administrative",
                    r::FileRole::Label => "desk-label",
                },
                restricted: f.description.access == r::FileAccess::Staff,
                current: f.current.map_or(0, |r| r.version.seq),
                last: f.last_seq,
                operation: format!("{:032x}", fresh()?),
            })
        })
        .collect::<Result<_, Failure>>()?;
    let collections = data
        .collections
        .into_iter()
        .map(|(id, name)| {
            (
                format!("{:032x}", id.0),
                name,
                data.deposit.collections.contains(&id),
            )
        })
        .collect();
    Ok(render(&StaffDeposit {
        t: &state.inner.i18n,
        csrf: token,
        instance: Some(data.deposit.instance.0),
        title: data.title,
        opac_hidden: !data.opac_visible,
        id: Some(key),
        state_key,
        files,
        collections,
        filter: q.filter,
    }))
}
#[derive(Deserialize)]
struct Change {
    csrf: String,
    action: String,
    name: Option<String>,
    role: Option<String>,
    access: Option<String>,
    collection: Option<String>,
    file: Option<String>,
    seq: Option<u64>,
    together: Option<String>,
}
impl Change {
    fn edit(self) -> Result<r::DeskEdit, Failure> {
        let name = || {
            self.name
                .filter(|s| !s.trim().is_empty() && s.chars().count() <= 256)
                .ok_or_else(bad)
        };
        Ok(match self.action.as_str() {
            "file" => r::DeskEdit::File(r::FileDescription {
                id: r::FileId(fresh()?),
                label: name()?,
                role: match self.role.as_deref() {
                    Some("primary") => r::FileRole::Primary,
                    Some("supplement") => r::FileRole::Supplement,
                    Some("administrative") => r::FileRole::Administrative,
                    Some("label") => r::FileRole::Label,
                    _ => return Err(bad()),
                },
                access: match self.access.as_deref() {
                    Some("public") => r::FileAccess::Public,
                    Some("staff") => r::FileAccess::Staff,
                    _ => return Err(bad()),
                },
            }),
            "collection" => r::DeskEdit::Collection(r::DigitalCollectionId(fresh()?), name()?),
            "join" => r::DeskEdit::Join(r::DigitalCollectionId(id(self
                .collection
                .as_deref()
                .ok_or_else(bad)?)?)),
            "publish" => r::DeskEdit::Publish,
            "withdraw" => r::DeskEdit::Withdraw,
            "retire" => r::DeskEdit::Retire(
                r::FileId(id(self.file.as_deref().ok_or_else(bad)?)?),
                self.seq.filter(|n| *n > 0).ok_or_else(bad)?,
                self.together.as_deref() == Some("yes"),
            ),
            _ => return Err(bad()),
        })
    }
}
async fn change(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(key): Path<String>,
    Form(f): Form<Change>,
) -> Result<Response, Failure> {
    let (desk, actor, token) = authorized(&state, &headers)?;
    csrf(&token, &f.csrf)?;
    let deposit = r::DepositId(id(&key)?);
    let edit = f.edit()?;
    let location = match &edit {
        r::DeskEdit::Collection(_, name) => Some(format!(
            "{ROOT}/{key}?filter={}",
            elzevir_web_templates::url_encode(name)
        )),
        _ => None,
    };
    work(move || desk.port.change(actor, deposit, edit)).await?;
    Ok(location.map_or_else(
        || redirect(deposit),
        |path| Redirect::to(&path).into_response(),
    ))
}
