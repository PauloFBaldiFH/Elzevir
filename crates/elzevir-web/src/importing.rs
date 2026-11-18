// SPDX-License-Identifier: GPL-3.0-or-later
//! Authenticated staff access to prepared import batches.

use std::collections::VecDeque;
use std::io::{self, Read};
use std::sync::{Arc, Mutex, PoisonError};

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, FromRequestParts, Multipart, Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header, request::Parts};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Form, Json, Router};
use elzevir_app::import::batches::{
    BatchCommand, BatchError, BatchPort, BatchPreview, BatchReply, BatchState, BatchSummary,
    BatchView, Completion, FormatChoice, MAX_BATCH_MANIFEST_BYTES,
};
use elzevir_app::import::preview::{IssueKind, MAX_IMPORT_FILE_BYTES, MAX_PREVIEW_RECORDS};
use elzevir_app::import_detection::{Evidence, ImportFormat};
use elzevir_app::matching::{ItemAction, Matching, OnMatch};
use elzevir_app::staff::{Permission, StaffId};
use elzevir_web_templates::{
    I18n, I18nError, ImportBatchLink, ImportDraft, ImportPage, ImportPreview, ImportReceipt,
    ImportSample, ImportUpload, ImportView,
};
use serde::Deserialize;
use tokio::sync::mpsc;

use crate::security::tokens_match;
use crate::session::ActiveSession;
use crate::{AppState, render, staff::current_session};

const ROOT: &str = "/equipe/acervo/importar";
const MAX_UPLOAD_BYTES: usize = MAX_IMPORT_FILE_BYTES + 64 * 1024;
const MAX_RETAINED_RECEIPTS: usize = 10;
const DIAGNOSTICS_PER_PAGE: usize = 20;
const MAX_FORM_FIELD_BYTES: usize = 64;

type SavedReceipt = (StaffId, String, Arc<Completion>);

pub(crate) struct Imports {
    port: Arc<dyn BatchPort>,
    portuguese: I18n,
    receipts: Mutex<VecDeque<SavedReceipt>>,
}

impl Imports {
    pub(crate) fn new(port: Arc<dyn BatchPort>) -> Result<Self, I18nError> {
        Ok(Self {
            port,
            portuguese: I18n::import_portuguese()?,
            receipts: Mutex::default(),
        })
    }

    fn remember(&self, operator: StaffId, id: String, completion: Completion) {
        let mut receipts = self.receipts.lock().unwrap_or_else(PoisonError::into_inner);
        receipts.retain(|(who, batch, _)| *who != operator || *batch != id);
        if receipts.len() == MAX_RETAINED_RECEIPTS {
            receipts.pop_front();
        }
        receipts.push_back((operator, id, Arc::new(completion)));
    }

    fn receipt(&self, operator: StaffId, id: &str) -> Option<Arc<Completion>> {
        self.receipts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .find(|(who, batch, _)| *who == operator && batch == id)
            .map(|(_, _, receipt)| Arc::clone(receipt))
    }

    fn recent(&self, operator: StaffId) -> Vec<String> {
        self.receipts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .rev()
            .filter(|(owner, _, _)| *owner == operator)
            .map(|(_, id, _)| id.clone())
            .collect()
    }
}

struct ImportSession {
    active: ActiveSession,
    imports: Arc<Imports>,
    pt: bool,
}

impl FromRequestParts<AppState> for ImportSession {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Response> {
        let Some(active) = current_session(state, &parts.headers) else {
            return Err(Redirect::to("/equipe/login").into_response());
        };
        let Some(imports) = state.imports.clone() else {
            return Err(StatusCode::SERVICE_UNAVAILABLE.into_response());
        };
        let session = Self {
            active,
            imports,
            pt: prefers_portuguese(&parts.headers),
        };
        if !session.active.identity.can(Permission::Cataloguing) {
            return Err(session.error(state, StatusCode::FORBIDDEN, "import-permission", None));
        }
        Ok(session)
    }
}

fn prefers_portuguese(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT_LANGUAGE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(',')
                .find_map(|part| {
                    let mut pieces = part.split(';');
                    let tag = pieces.next()?.trim().split('-').next()?;
                    if pieces.any(|param| {
                        param.trim().strip_prefix("q=").is_some_and(|value| {
                            !value.is_empty() && value.bytes().all(|b| matches!(b, b'0' | b'.'))
                        })
                    }) {
                        return None;
                    }
                    match tag.to_ascii_lowercase().as_str() {
                        "pt" => Some(true),
                        "en" => Some(false),
                        _ => None,
                    }
                })
                .unwrap_or(false)
        })
}

impl ImportSession {
    fn translations<'a>(&'a self, state: &'a AppState) -> &'a I18n {
        if self.pt {
            &self.imports.portuguese
        } else {
            &state.inner.i18n
        }
    }

    fn page(
        &self,
        state: &AppState,
        view: ImportView,
        status: StatusCode,
        notice: Option<&'static str>,
        detail: Option<String>,
    ) -> Response {
        let mut response = render(&ImportPage {
            t: self.translations(state),
            csrf: &self.active.csrf,
            view,
            notice,
            detail,
        });
        if response.status().is_success() {
            *response.status_mut() = status;
        }
        response
    }

    fn error(
        &self,
        state: &AppState,
        status: StatusCode,
        key: &'static str,
        detail: Option<String>,
    ) -> Response {
        self.page(state, ImportView::Error, status, Some(key), detail)
    }

    fn failure(&self, state: &AppState, error: BatchError) -> Response {
        let (status, key, detail) = error_view(error);
        self.error(state, status, key, detail)
    }

    fn start(
        &self,
        command: BatchCommand,
    ) -> tokio::task::JoinHandle<Result<BatchReply, BatchError>> {
        let port = Arc::clone(&self.imports.port);
        let operator = self.active.identity.id;
        tokio::task::spawn_blocking(move || port.execute(operator, command))
    }

    async fn run(&self, command: BatchCommand) -> Result<BatchReply, BatchError> {
        join(self.start(command)).await
    }
}

async fn join(
    job: tokio::task::JoinHandle<Result<BatchReply, BatchError>>,
) -> Result<BatchReply, BatchError> {
    job.await
        .map_err(|error| BatchError::Unavailable(error.to_string()))?
}

fn error_view(error: BatchError) -> (StatusCode, &'static str, Option<String>) {
    match error {
        BatchError::Missing(_) => (StatusCode::NOT_FOUND, "import-missing", None),
        BatchError::Conflict(_) => (StatusCode::CONFLICT, "import-stale", None),
        BatchError::Invalid(detail) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "import-invalid",
            Some(detail),
        ),
        BatchError::Unavailable(_) => (StatusCode::SERVICE_UNAVAILABLE, "import-unavailable", None),
    }
}

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            ROOT,
            get(home).merge(post(upload).layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES))),
        )
        .route("/equipe/acervo/importar/{id}", get(batch))
        .route("/equipe/acervo/importar/modelo/{format}", get(model))
        .route("/equipe/acervo/importar/{id}/resultado", get(result))
        .route("/equipe/acervo/importar/{id}/{action}", post(change))
        .layer(axum::middleware::map_response(private))
}

async fn private(mut response: Response) -> Response {
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    response
}

async fn home(
    State(state): State<AppState>,
    session: ImportSession,
    Query(page): Query<Page>,
) -> Response {
    match session.run(BatchCommand::List).await {
        Ok(BatchReply::List(batches)) => {
            let t = session.translations(&state);
            let batches = batches
                .into_iter()
                .map(|batch| batch_link(t, batch))
                .collect();
            session.page(
                &state,
                ImportView::Home(
                    batches,
                    session.imports.recent(session.active.identity.id),
                    if page.koha_sql {
                        ImportUpload::KohaSql
                    } else if page.tabular {
                        ImportUpload::Table
                    } else {
                        ImportUpload::Auto
                    },
                ),
                StatusCode::OK,
                None,
                None,
            )
        }
        Err(error) => session.failure(&state, error),
        Ok(_) => session.error(
            &state,
            StatusCode::SERVICE_UNAVAILABLE,
            "import-unavailable",
            None,
        ),
    }
}

async fn upload(
    State(state): State<AppState>,
    session: ImportSession,
    form: Result<Multipart, axum::extract::multipart::MultipartRejection>,
) -> Response {
    let Ok(mut form) = form else {
        return session.error(
            &state,
            StatusCode::BAD_REQUEST,
            "import-upload-invalid",
            None,
        );
    };
    if !upload_token(&mut form, &session.active.csrf).await {
        return session.error(&state, StatusCode::FORBIDDEN, "import-forbidden", None);
    }
    let invalid = || {
        session.error(
            &state,
            StatusCode::BAD_REQUEST,
            "import-upload-invalid",
            None,
        )
    };
    let Ok(Some(mut field)) = form.next_field().await else {
        return invalid();
    };
    let mut selected = None;
    if field.name() == Some("format") {
        let Some(text) = short_text(field).await else {
            return invalid();
        };
        let Ok(choice) = parse_format(&text) else {
            return invalid();
        };
        selected = choice;
        let Ok(Some(next)) = form.next_field().await else {
            return invalid();
        };
        field = next;
    }
    if field.name() != Some("file") || field.file_name().is_none() {
        return invalid();
    }
    let (send, receive) = mpsc::channel(2);
    let job = session.start(BatchCommand::Stage(
        Box::new(UploadReader {
            receive,
            chunk: Bytes::new(),
            ended: false,
        }),
        selected,
    ));
    if transfer(field, &send).await.is_ok() && matches!(form.next_field().await, Ok(None)) {
        // The reader may have rejected the batch already. Its reply is authoritative.
        drop(send.send(None).await);
    }
    drop(send);
    match join(job).await {
        Ok(BatchReply::Batch(view)) => {
            Redirect::to(&format!("{ROOT}/{}", view.batch.id)).into_response()
        }
        Err(error) => session.failure(&state, error),
        Ok(_) => session.error(
            &state,
            StatusCode::SERVICE_UNAVAILABLE,
            "import-unavailable",
            None,
        ),
    }
}

async fn upload_token(form: &mut Multipart, expected: &str) -> bool {
    let Ok(Some(field)) = form.next_field().await else {
        return false;
    };
    if field.name() != Some("csrf") || field.file_name().is_some() {
        return false;
    }
    short_text(field)
        .await
        .is_some_and(|token| tokens_match(expected, &token))
}

async fn short_text(mut field: axum::extract::multipart::Field<'_>) -> Option<String> {
    let mut text = Vec::with_capacity(MAX_FORM_FIELD_BYTES);
    loop {
        match field.chunk().await {
            Ok(Some(chunk)) if chunk.len() <= MAX_FORM_FIELD_BYTES.saturating_sub(text.len()) => {
                text.extend_from_slice(&chunk);
            }
            Ok(None) => return String::from_utf8(text).ok(),
            _ => return None,
        }
    }
}

async fn transfer(
    mut field: axum::extract::multipart::Field<'_>,
    send: &mpsc::Sender<Option<Bytes>>,
) -> Result<(), ()> {
    while let Some(chunk) = field.chunk().await.map_err(drop)? {
        send.send(Some(chunk)).await.map_err(drop)?;
    }
    Ok(())
}

struct UploadReader {
    receive: mpsc::Receiver<Option<Bytes>>,
    chunk: Bytes,
    ended: bool,
}

impl Read for UploadReader {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() || self.ended {
            return Ok(0);
        }
        while self.chunk.is_empty() {
            match self.receive.blocking_recv() {
                Some(Some(chunk)) => self.chunk = chunk,
                Some(None) => {
                    self.ended = true;
                    return Ok(0);
                }
                None => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "upload did not finish",
                    ));
                }
            }
        }
        self.chunk
            .split_to(output.len().min(self.chunk.len()))
            .as_ref()
            .read(output)
    }
}

#[derive(Default, Deserialize)]
struct Page {
    from: Option<u64>,
    #[serde(default)]
    problems: usize,
    #[serde(default)]
    download: bool,
    #[serde(default)]
    tabular: bool,
    #[serde(default)]
    koha_sql: bool,
}

async fn batch(
    State(state): State<AppState>,
    session: ImportSession,
    Path(id): Path<String>,
    Query(page): Query<Page>,
) -> Response {
    match session.run(BatchCommand::View(id, page.from)).await {
        Ok(BatchReply::Batch(view)) => show_batch(&state, &session, *view, &page, None),
        Err(error) => session.failure(&state, error),
        Ok(_) => session.error(
            &state,
            StatusCode::SERVICE_UNAVAILABLE,
            "import-unavailable",
            None,
        ),
    }
}

#[derive(Deserialize)]
struct OperationForm {
    csrf: String,
    revision: u64,
    #[serde(default)]
    recipe: String,
    format: Option<String>,
    acknowledged: Option<String>,
}

async fn change(
    State(state): State<AppState>,
    session: ImportSession,
    Path((id, action)): Path<(String, String)>,
    form: Result<Form<OperationForm>, axum::extract::rejection::FormRejection>,
) -> Response {
    let Ok(Form(form)) = form else {
        return session.error(&state, StatusCode::BAD_REQUEST, "import-invalid", None);
    };
    if !tokens_match(&session.active.csrf, &form.csrf) {
        return session.error(&state, StatusCode::FORBIDDEN, "import-forbidden", None);
    }
    let command = match action.as_str() {
        "editar" => match parse_recipe(&form.recipe).and_then(|recipe| {
            let format = match form.format.as_deref() {
                None => FormatChoice::Keep,
                Some(text) => parse_format(text)?.map_or(FormatChoice::Detect, FormatChoice::Use),
            };
            Ok(BatchCommand::Edit(
                id.clone(),
                form.revision,
                recipe,
                format,
            ))
        }) {
            Ok(command) => command,
            Err(error) => return edit_failure(&state, &session, id, form.recipe, error).await,
        },
        "prever" => BatchCommand::Preview(id.clone(), Some(form.revision), Some(0)),
        "confirmar" => return confirm(&state, &session, id, form.revision).await,
        "cancelar" => BatchCommand::Cancel(
            id.clone(),
            Some(form.revision),
            form.acknowledged.as_deref() == Some("yes"),
        ),
        _ => return session.error(&state, StatusCode::NOT_FOUND, "import-missing", None),
    };
    match session.run(command).await {
        Ok(BatchReply::Batch(view)) if action == "prever" => {
            show_batch(&state, &session, *view, &Page::default(), None)
        }
        Ok(BatchReply::Batch(view)) => {
            Redirect::to(&format!("{ROOT}/{}", view.batch.id)).into_response()
        }
        Ok(BatchReply::Cancelled) => Redirect::to(ROOT).into_response(),
        Ok(BatchReply::List(_) | BatchReply::Completed(_)) => session.error(
            &state,
            StatusCode::SERVICE_UNAVAILABLE,
            "import-unavailable",
            None,
        ),
        Err(error) if action == "editar" => {
            edit_failure(&state, &session, id, form.recipe, error).await
        }
        Err(error) => session.failure(&state, error),
    }
}

async fn confirm(state: &AppState, session: &ImportSession, id: String, revision: u64) -> Response {
    let url = format!("{ROOT}/{id}/resultado");
    let imports = Arc::clone(&session.imports);
    let actor = session.active.identity.id;
    // Record the outcome even if the request disappears while the writer is working.
    match tokio::task::spawn_blocking(move || {
        match imports
            .port
            .execute(actor, BatchCommand::Confirm(id.clone(), revision))?
        {
            BatchReply::Completed(completion) => {
                imports.remember(actor, id, completion);
                Ok(())
            }
            _ => Err(BatchError::Unavailable(
                "unexpected confirmation reply".into(),
            )),
        }
    })
    .await
    {
        Ok(Ok(())) => Redirect::to(&url).into_response(),
        Ok(Err(error)) => session.failure(state, error),
        Err(_) => session.error(
            state,
            StatusCode::SERVICE_UNAVAILABLE,
            "import-unavailable",
            None,
        ),
    }
}

fn parse_format(value: &str) -> Result<Option<ImportFormat>, BatchError> {
    match value {
        "auto" => Ok(None),
        "iso" => Ok(Some(ImportFormat::Iso2709)),
        "xml" => Ok(Some(ImportFormat::MarcXml)),
        "mrk" => Ok(Some(ImportFormat::MarcText)),
        "table" => Ok(Some(ImportFormat::Delimited)),
        "sql" => Ok(Some(ImportFormat::KohaSqlDump)),
        _ => Err(BatchError::Invalid("invalid source format".into())),
    }
}

fn parse_recipe(value: &str) -> Result<elzevir_app::import::edits::Recipe, BatchError> {
    if value.len() as u64 > MAX_BATCH_MANIFEST_BYTES {
        return Err(BatchError::Invalid("recipe exceeds its byte limit".into()));
    }
    elzevir_app::import::edits::Recipe::parse(value)
        .map_err(|error| BatchError::Invalid(error.to_string()))
}

async fn edit_failure(
    state: &AppState,
    session: &ImportSession,
    id: String,
    recipe: String,
    error: BatchError,
) -> Response {
    match session.run(BatchCommand::View(id, None)).await {
        Ok(BatchReply::Batch(view)) => show_batch(
            state,
            session,
            *view,
            &Page::default(),
            Some((recipe, error)),
        ),
        _ => session.failure(state, error),
    }
}

fn show_batch(
    state: &AppState,
    session: &ImportSession,
    view: BatchView,
    page: &Page,
    failed: Option<(String, BatchError)>,
) -> Response {
    let t = session.translations(state);
    let BatchView {
        batch,
        recipe,
        preview,
    } = view;
    let editable = matches!(batch.state, BatchState::Review | BatchState::Ready);
    let ready = batch.state == BatchState::Ready
        && preview
            .as_ref()
            .is_some_and(|preview| preview.report.issues.is_empty());
    let interrupted = batch.state == BatchState::Confirming;
    let preview =
        preview.map(|preview| preview_page(t, &batch.id, page, &preview, recipe.matching));
    let revision = batch.revision;
    let format = match batch.selected {
        Some(ImportFormat::Iso2709) => "iso",
        Some(ImportFormat::MarcXml) => "xml",
        Some(ImportFormat::MarcText) => "mrk",
        Some(ImportFormat::Delimited) => "table",
        Some(ImportFormat::KohaSqlDump) => "sql",
        _ if batch.format == "Delimited" => "table",
        _ if batch.format == "KohaSqlDump" => "sql",
        _ => "auto",
    };
    let link = batch_link(t, batch);
    let (recipe, status, notice, detail) = if let Some((recipe, failure)) = failed {
        let (status, key, detail) = error_view(failure);
        (recipe, status, Some(key), detail)
    } else {
        let Ok(recipe) = serde_json::to_string_pretty(&recipe) else {
            return session.error(
                state,
                StatusCode::SERVICE_UNAVAILABLE,
                "import-unavailable",
                None,
            );
        };
        (recipe, StatusCode::OK, None, None)
    };
    session.page(
        state,
        ImportView::Draft(Box::new(ImportDraft {
            link,
            revision,
            recipe,
            format,
            editable,
            ready,
            interrupted,
            preview,
        })),
        status,
        notice,
        detail,
    )
}

fn batch_link(t: &I18n, batch: BatchSummary) -> ImportBatchLink {
    let format = match batch.format.as_str() {
        "Iso2709" => "import-iso",
        "MarcXml" => "import-xml",
        "MarcText" => "import-mrk",
        "Delimited" => "import-table",
        "KohaSqlDump" => "import-sql",
        _ => "import-unknown",
    };
    let state = match batch.state {
        BatchState::Preparing => "import-preparing",
        BatchState::Review => "import-review",
        BatchState::Ready => "import-ready",
        BatchState::Confirming => "import-interrupted",
        BatchState::Closed => "import-closed",
    };
    ImportBatchLink {
        id: batch.id,
        label: format!("{} · {}", t.get(format), t.get(state)),
    }
}

fn preview_page(
    t: &I18n,
    id: &str,
    page: &Page,
    preview: &BatchPreview,
    matching: Option<Matching>,
) -> ImportPreview {
    let BatchPreview { rendered, report } = preview;
    let from = page.from.unwrap_or(0);
    let evidence = match report.detection.evidence {
        Evidence::IsoLeader => "import-evidence-iso",
        Evidence::MarcXmlNamespace => "import-evidence-xml",
        Evidence::MarcTextLeader => "import-evidence-mrk",
        Evidence::DelimitedRows => "import-evidence-table",
        Evidence::KohaCatalogTables => "import-evidence-sql",
        Evidence::EmptyInput => "import-evidence-empty",
        _ => "import-evidence-unknown",
    };
    let diagnose = |issue: &elzevir_app::import::preview::PreviewIssue| {
        let index = rendered.selected_index(issue.index).unwrap_or_default();
        let url = format!(
            "{ROOT}/{id}?from={index}&problems={}#record-{}",
            page.problems,
            issue.index + 1
        );
        let where_ = location(
            t,
            issue.index + 1,
            issue.offset,
            issue.item.map(|item| item + 1),
        );
        let detail = matches!(
            issue.kind,
            IssueKind::InvalidRecord | IssueKind::InvalidDescription | IssueKind::ItemRegistry
        )
        .then(|| issue.reason.clone());
        (
            url,
            format!("{where_} · {}", problem(t, issue.kind)),
            detail,
        )
    };
    let diagnostics = report
        .issues
        .iter()
        .skip(page.problems)
        .take(DIAGNOSTICS_PER_PAGE)
        .map(diagnose)
        .collect();
    let record_pages = pages(from, MAX_PREVIEW_RECORDS as u64, report.read, |from| {
        format!(
            "{ROOT}/{id}?from={from}&problems={}#import-preview",
            page.problems
        )
    });
    let diagnostic_pages = pages(
        page.problems as u64,
        DIAGNOSTICS_PER_PAGE as u64,
        report.issues.len() as u64,
        |problems| format!("{ROOT}/{id}?from={from}&problems={problems}#import-diagnostics"),
    );
    ImportPreview {
        counts: t.get_numbers(
            "import-summary",
            &[
                ("read", rendered.read),
                ("excluded", rendered.excluded),
                ("changed", rendered.changed),
                ("items", report.items),
                ("matches", report.matches.len() as u64),
                (
                    "replacements",
                    if matching.is_some_and(|rules| rules.on_match == OnMatch::Replace) {
                        report.matches.len() as u64
                    } else {
                        0
                    },
                ),
            ],
        ),
        evidence: t.get(evidence),
        replace_items: matching.is_some_and(|rules| rules.items == ItemAction::Replace),
        records: record_samples(preview),
        diagnostics,
        record_pages,
        diagnostic_pages,
    }
}

fn record_samples(preview: &BatchPreview) -> Vec<ImportSample> {
    let BatchPreview { rendered, report } = preview;
    rendered
        .source
        .iter()
        .map(|entry| {
            let index = entry.record - 1;
            let mapped = report
                .samples
                .iter()
                .find(|sample| sample.index == index)
                .map(|sample| (sample.work.clone(), sample.instance.clone(), sample.items));
            let candidate = report
                .matches
                .iter()
                .find(|(source, _)| *source == index)
                .map(|(_, id)| id.0);
            ImportSample {
                record: entry.record,
                mapped,
                candidate,
                before: entry.before.clone(),
                after: entry.after.clone(),
            }
        })
        .collect()
}

fn pages(
    start: u64,
    step: u64,
    total: u64,
    url: impl Fn(u64) -> String,
) -> (Option<String>, Option<String>) {
    (
        (start > 0).then(|| url(start.saturating_sub(step))),
        (start.saturating_add(step) < total).then(|| url(start + step)),
    )
}

fn problem(t: &I18n, kind: IssueKind) -> String {
    t.get(match kind {
        IssueKind::InvalidRecord => "import-problem-record",
        IssueKind::InvalidDescription => "import-problem-description",
        IssueKind::UnknownBranch => "import-problem-branch",
        IssueKind::UnknownItemType => "import-problem-item-type",
        IssueKind::ItemRegistry => "import-problem-registry",
        IssueKind::MissingBarcode => "import-problem-missing-barcode",
        IssueKind::RepeatedBarcode => "import-problem-repeated-barcode",
        IssueKind::UsedBarcode => "import-problem-used-barcode",
        IssueKind::BatchMatch(index) => {
            return t.get_numbers("import-problem-match", &[("record", index + 1)]);
        }
    })
}

fn location(t: &I18n, record: u64, byte: u64, item: Option<usize>) -> String {
    if let Some(item) = item {
        t.get_numbers(
            "import-diagnostic-item",
            &[("record", record), ("byte", byte), ("item", item as u64)],
        )
    } else {
        t.get_numbers(
            "import-diagnostic-record",
            &[("record", record), ("byte", byte)],
        )
    }
}

async fn result(
    State(state): State<AppState>,
    session: ImportSession,
    Path(id): Path<String>,
    Query(page): Query<Page>,
) -> Response {
    let Some(result) = session.imports.receipt(session.active.identity.id, &id) else {
        return session.error(&state, StatusCode::NOT_FOUND, "import-missing", None);
    };
    if page.download {
        return (
            [(header::CONTENT_DISPOSITION, "attachment; filename=\"elzevir-import.json\"")],
            Json(serde_json::json!({
                "receipt": result.receipt, "refusals": result.refusals, "removed": result.error.is_none(),
            })),
        ).into_response();
    }
    let t = session.translations(&state);
    let counts = result.receipt.as_ref().map_or_else(Vec::new, |receipt| {
        vec![
            ("import-read", receipt.read),
            ("import-prepared", receipt.prepared),
            ("import-excluded", receipt.excluded),
            ("import-stored", receipt.stored),
            ("import-matched", receipt.matched as u64),
            ("import-replaced", receipt.replaced),
            ("import-skipped", receipt.skipped),
            ("import-failed", receipt.failed as u64),
            ("import-items-stored", receipt.items_stored),
            ("import-items-replaced", receipt.items_replaced),
            ("import-items-refused", receipt.items_refused as u64),
        ]
    });
    let refusals = result
        .refusals
        .iter()
        .take(DIAGNOSTICS_PER_PAGE)
        .map(|refusal| match &refusal.detail {
            Some(detail) => format!(
                "{} · {detail}",
                location(t, refusal.record, refusal.offset, None)
            ),
            None => location(t, refusal.record, refusal.offset, refusal.item),
        })
        .collect();
    let page = ImportReceipt {
        download: format!("{ROOT}/{id}/resultado?download=true"),
        counts,
        refusals,
        more: result.refusals.len() > DIAGNOSTICS_PER_PAGE,
        removed: result.error.is_none(),
    };
    let notice = result.error.is_some().then_some("import-incomplete");
    session.page(
        &state,
        ImportView::Result(page),
        StatusCode::OK,
        notice,
        None,
    )
}

async fn model(
    State(state): State<AppState>,
    session: ImportSession,
    Path(format): Path<String>,
) -> Response {
    use elzevir_app::import::table::{Delimiter, write_model_header};
    let (delimiter, content_type, file) = match format.as_str() {
        "csv" => (
            Delimiter::Comma,
            "text/csv; charset=utf-8",
            "attachment; filename=\"elzevir-model.csv\"",
        ),
        "tsv" => (
            Delimiter::Tab,
            "text/tab-separated-values; charset=utf-8",
            "attachment; filename=\"elzevir-model.tsv\"",
        ),
        _ => return session.error(&state, StatusCode::NOT_FOUND, "import-missing", None),
    };
    let mut output = Vec::new();
    if write_model_header(&mut output, delimiter).is_err() {
        return session.error(
            &state,
            StatusCode::SERVICE_UNAVAILABLE,
            "import-unavailable",
            None,
        );
    }
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CONTENT_DISPOSITION, file),
        ],
        output,
    )
        .into_response()
}
