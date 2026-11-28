// SPDX-License-Identifier: GPL-3.0-or-later
//! Multi-file inventory on the authenticated import screen.

use axum::extract::{Form, Multipart, Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use elzevir_app::import::batches::{
    BatchCommand, BatchReply, BatchSummary, MAX_SOURCE_MEMBERS, SourceInput, SourceSet,
};
use elzevir_app::import::containers::Inventory;
use elzevir_app::import_detection::ImportFormat;
use elzevir_web_templates::{ImportView, SourceMemberRow};
use tokio::sync::mpsc;

use super::{
    AppState, ImportSession, ROOT, UploadReader, join, limited_text, tokens_match, transfer,
    upload_token,
};

pub(super) async fn archive_set(
    State(state): State<AppState>,
    session: ImportSession,
    Path(id): Path<String>,
    Form(form): Form<Vec<(String, String)>>,
) -> Response {
    let field = |name| crate::import_sheets::field(&form, name).unwrap_or_default();
    if !session.can_inventory_sources() || !tokens_match(&session.active.csrf, field("csrf")) {
        return session.error(&state, StatusCode::FORBIDDEN, "import-forbidden", None);
    }
    let positions = form
        .iter()
        .filter(|(k, _)| k == "member")
        .map(|(_, v)| v.parse::<usize>())
        .take(MAX_SOURCE_MEMBERS + 1)
        .collect::<Result<Vec<_>, _>>();
    let (Ok(revision), Ok(positions)) = (field("revision").parse(), positions) else {
        return session.error(&state, StatusCode::BAD_REQUEST, "import-invalid", None);
    };
    let command = BatchCommand::ArchiveSet(id, revision, field("origin").to_owned(), positions);
    match session.run(command).await {
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

pub(super) async fn upload_set(
    State(state): State<AppState>,
    session: ImportSession,
    mut form: Multipart,
) -> Response {
    if !session.can_inventory_sources() || !upload_token(&mut form, &session.active.csrf).await {
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
    let Ok(Some(field)) = form.next_field().await else {
        return invalid();
    };
    if field.name() != Some("origin") || field.file_name().is_some() {
        return invalid();
    }
    let Some(origin) = limited_text(field, 256).await else {
        return invalid();
    };
    let (send, mut receive) = mpsc::channel::<Option<SourceInput>>(1);
    // A final explicit marker distinguishes a complete multipart form from a
    // disconnected request, including a disconnect between two complete files.
    let inputs = std::iter::from_fn(move || match receive.blocking_recv() {
        Some(member) => member.map(Ok),
        None => Some(Err("source upload interrupted".into())),
    });
    let job = session.start(BatchCommand::StageSet(origin, Box::new(inputs)));
    loop {
        let field = match form.next_field().await {
            Ok(Some(field)) if field.name() == Some("file") => field,
            Ok(None) => {
                drop(send.send(None).await);
                break;
            }
            _ => break,
        };
        let Some(name) = field.file_name().map(str::to_owned) else {
            break;
        };
        let (chunks, reader) = UploadReader::channel();
        if send
            .send(Some(SourceInput {
                name,
                input: Box::new(reader),
            }))
            .await
            .is_err()
            || transfer(field, &chunks).await.is_err()
            || chunks.send(None).await.is_err()
        {
            break;
        }
    }
    drop(send);
    match join(job).await {
        Ok(BatchReply::Batch(view)) => {
            Redirect::to(&format!("{ROOT}/{}", view.batch.id)).into_response()
        }
        Err(error) => session.failure(&state, error),
        Ok(_) => invalid(),
    }
}

pub(super) fn show_set(
    state: &AppState,
    session: &ImportSession,
    batch: BatchSummary,
    set: SourceSet,
    notice: Option<&'static str>,
) -> Response {
    let t = session.translations(state);
    let members = set
        .members
        .into_iter()
        .map(|m| SourceMemberRow {
            position: m.position,
            name: m.archive_path.map_or(m.name, |path| path.join(" › ")),
            bytes: m.bytes,
            sha256: m.sha256,
            format: t.get(match m.format {
                ImportFormat::Iso2709 => "import-iso",
                ImportFormat::MarcXml => "import-xml",
                ImportFormat::MarcText => "import-mrk",
                ImportFormat::Delimited => "import-table",
                ImportFormat::Archive => "import-archive",
                ImportFormat::KohaSqlDump => "import-sql",
                ImportFormat::KohaFullBackup => "import-source-koha-backup",
                ImportFormat::Unknown => "import-unknown",
            }),
            duplicate_of: m.duplicate_of,
        })
        .collect();
    session.page(
        state,
        ImportView::Sources(
            batch.id,
            batch.revision,
            set.origin,
            set.archive_sha256,
            members,
        ),
        StatusCode::OK,
        notice,
        None,
    )
}

pub(super) fn member_choices(archive: Option<&Inventory>, enabled: bool) -> Vec<(usize, String)> {
    archive
        .filter(|_| enabled)
        .into_iter()
        .flat_map(|inventory| inventory.entries.iter().enumerate())
        .filter(|(_, entry)| !entry.member.is_empty())
        .map(|(i, entry)| (i + 1, entry.member.join(" › ")))
        .collect()
}
