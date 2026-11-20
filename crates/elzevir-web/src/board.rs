// SPDX-License-Identifier: GPL-3.0-or-later
//! The notice board: the notes of both homes and the form that writes them
//! (docs/specs/interface/mural.md).

use std::sync::Arc;

use axum::Form;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use elzevir_app::board::{
    Audience, BoardCommand, BoardError, BoardNote, BoardNoteId, BoardRefusal, MAX_NOTES,
    NoteContent, NoteKind, NoteText, NoteTextError, NoticeBoard, OPAC_NOTES,
};
use elzevir_app::staff::Permission;
use elzevir_web_templates::{BoardCard, KindChoice, StaffBoardNote, note_color};
use serde::Deserialize;

use crate::security::tokens_match;
use crate::session::ActiveSession;
use crate::staff::current_session;
use crate::{AppState, render};

/// The Fluent key of a kind's name.
const fn kind_key(kind: NoteKind) -> &'static str {
    match kind {
        NoteKind::Staff => "board-kind-staff",
        NoteKind::Institutional => "board-kind-institutional",
        NoteKind::Personal => "board-kind-personal",
        NoteKind::Urgent => "board-kind-urgent",
    }
}

/// Whether `session` may change a note seen by `audience` (spec, item 3).
fn may_change(session: &ActiveSession, audience: Audience) -> bool {
    audience == Audience::Staff || session.identity.can(Permission::Administration)
}

/// The cards of `notes` in board order; `session` is the staff member looking, `None`
/// on the OPAC, which never names who wrote a note (spec, item 6).
fn cards(notes: Vec<BoardNote>, session: Option<&ActiveSession>) -> Vec<BoardCard> {
    notes
        .into_iter()
        .enumerate()
        .map(|(position, note)| BoardCard {
            edit: session
                .filter(|session| may_change(session, note.audience))
                .map(|_| format!("/equipe/mural/{}", note.id.0)),
            kind_key: kind_key(note.kind),
            text: note.text,
            author: session.map(|_| note.author),
            day: note.on.strftime("%d/%m/%Y").to_string(),
            color: note_color(position),
            public: note.audience == Audience::Public,
        })
        .collect()
}

/// Reads the board off the async runtime; a failure shows an empty board, since the home
/// still works without it.
async fn read_notes(
    board: Option<Arc<dyn NoticeBoard>>,
    audience: Option<Audience>,
) -> Vec<BoardNote> {
    let Some(board) = board else {
        return Vec::new();
    };
    let limit = if audience.is_some() {
        OPAC_NOTES
    } else {
        MAX_NOTES
    };
    match tokio::task::spawn_blocking(move || board.notes(audience, limit)).await {
        Ok(Ok(notes)) => notes,
        // The welcome note takes the board's place; the error is the store's to log.
        Ok(Err(_)) | Err(_) => Vec::new(),
    }
}

/// The notes of the staff home.
pub(crate) async fn staff_cards(state: &AppState, session: &ActiveSession) -> Vec<BoardCard> {
    cards(read_notes(state.board.clone(), None).await, Some(session))
}

/// The public notes of the OPAC home.
pub(crate) async fn opac_cards(state: &AppState) -> Vec<BoardCard> {
    cards(
        read_notes(state.board.clone(), Some(Audience::Public)).await,
        None,
    )
}

/// The fields the note form posts.
#[derive(Deserialize)]
pub(crate) struct NoteFields {
    csrf: String,
    text: String,
    kind: String,
    #[serde(default)]
    public: Option<String>,
    #[serde(default)]
    row_version: i64,
}

/// Fields of the remove button.
#[derive(Deserialize)]
pub(crate) struct RemoveFields {
    csrf: String,
}

/// The note as the form shows it.
struct Shown {
    id: Option<BoardNoteId>,
    row_version: i64,
    text: String,
    kind: NoteKind,
    public: bool,
    refused: Option<String>,
}

fn form(state: &AppState, session: &ActiveSession, shown: Shown, status: StatusCode) -> Response {
    let page = StaffBoardNote {
        t: &state.inner.i18n,
        csrf: &session.csrf,
        action: shown.id.map_or_else(
            || "/equipe/mural".to_owned(),
            |id| format!("/equipe/mural/{}", id.0),
        ),
        remove: shown.id.map(|id| format!("/equipe/mural/{}/remover", id.0)),
        row_version: shown.row_version,
        text: shown.text,
        kinds: NoteKind::ALL
            .into_iter()
            .map(|kind| KindChoice {
                value: kind.as_str(),
                label_key: kind_key(kind),
                selected: kind == shown.kind,
            })
            .collect(),
        public: shown.public,
        may_publish: session.identity.can(Permission::Administration),
        refused: shown.refused,
    };
    (status, render(&page)).into_response()
}

/// The signed-in operator and the board, or the answer that ends the request.
fn open(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(ActiveSession, Arc<dyn NoticeBoard>), Box<Response>> {
    let Some(session) = current_session(state, headers) else {
        return Err(Box::new(Redirect::to("/equipe/login").into_response()));
    };
    let Some(board) = state.board.clone() else {
        return Err(Box::new(StatusCode::NOT_FOUND.into_response()));
    };
    Ok((session, board))
}

/// One stored note, or the answer that ends the request.
async fn stored(board: &Arc<dyn NoticeBoard>, id: i64) -> Result<BoardNote, Box<Response>> {
    let board = Arc::clone(board);
    match tokio::task::spawn_blocking(move || board.note(BoardNoteId(id))).await {
        Ok(Ok(Some(note))) => Ok(note),
        Ok(Ok(None)) => Err(Box::new(StatusCode::NOT_FOUND.into_response())),
        Ok(Err(_)) | Err(_) => Err(Box::new(StatusCode::SERVICE_UNAVAILABLE.into_response())),
    }
}

/// `GET /equipe/mural/nova`: an empty note.
pub(crate) async fn new_note(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let (session, _) = match open(&state, &headers) {
        Ok(opened) => opened,
        Err(response) => return *response,
    };
    let shown = Shown {
        id: None,
        row_version: 0,
        text: String::new(),
        kind: NoteKind::Staff,
        public: false,
        refused: None,
    };
    form(&state, &session, shown, StatusCode::OK)
}

/// `GET /equipe/mural/{id}`: a stored note to edit.
pub(crate) async fn edit_note(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    let (session, board) = match open(&state, &headers) {
        Ok(opened) => opened,
        Err(response) => return *response,
    };
    let note = match stored(&board, id).await {
        Ok(note) => note,
        Err(response) => return *response,
    };
    if !may_change(&session, note.audience) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let shown = Shown {
        id: Some(note.id),
        row_version: note.row_version,
        text: note.text,
        kind: note.kind,
        public: note.audience == Audience::Public,
        refused: None,
    };
    form(&state, &session, shown, StatusCode::OK)
}

/// `POST /equipe/mural`: adds a note.
pub(crate) async fn add_note(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(fields): Form<NoteFields>,
) -> Response {
    save(state, headers, None, fields).await
}

/// `POST /equipe/mural/{id}`: rewrites a note.
pub(crate) async fn update_note(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Form(fields): Form<NoteFields>,
) -> Response {
    save(state, headers, Some(id), fields).await
}

async fn save(
    state: AppState,
    headers: HeaderMap,
    id: Option<i64>,
    fields: NoteFields,
) -> Response {
    let (session, board) = match open(&state, &headers) {
        Ok(opened) => opened,
        Err(response) => return *response,
    };
    if !tokens_match(&session.csrf, &fields.csrf) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(kind) = NoteKind::from_name(&fields.kind) else {
        return (StatusCode::BAD_REQUEST, "unknown kind").into_response();
    };
    let audience = if fields.public.is_some() {
        Audience::Public
    } else {
        Audience::Staff
    };
    let was = match id {
        Some(id) => match stored(&board, id).await {
            Ok(note) => Some(note),
            Err(response) => return *response,
        },
        None => None,
    };
    let touched = [Some(audience), was.as_ref().map(|note| note.audience)];
    if touched
        .into_iter()
        .flatten()
        .any(|audience| !may_change(&session, audience))
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    let t = &state.inner.i18n;
    let mut shown = Shown {
        id: was.as_ref().map(|note| note.id),
        row_version: fields.row_version,
        text: fields.text,
        kind,
        public: audience == Audience::Public,
        refused: None,
    };
    let text = match NoteText::new(&shown.text) {
        Ok(text) => text,
        Err(error) => {
            shown.refused = Some(t.get(text_error_key(error)));
            return form(&state, &session, shown, StatusCode::UNPROCESSABLE_ENTITY);
        }
    };
    let content = NoteContent {
        kind,
        audience,
        text,
        author: session.identity.id,
        on: jiff::Timestamp::now()
            .to_zoned(state.inner.config.time_zone.clone())
            .date(),
    };
    let command = match was {
        Some(note) => BoardCommand::Update {
            id: note.id,
            was: note.audience,
            row_version: fields.row_version,
            content,
        },
        None => BoardCommand::Add(content),
    };
    match apply(&board, &session, command).await {
        Ok(()) => Redirect::to("/equipe").into_response(),
        Err(BoardError::Refused(BoardRefusal::NotFound)) => StatusCode::NOT_FOUND.into_response(),
        Err(BoardError::Refused(refusal)) => {
            if refusal == BoardRefusal::Stale {
                // The form now carries the stored version, so saving again replaces it.
                if let Some(id) = shown.id
                    && let Ok(note) = stored(&board, id.0).await
                {
                    shown.row_version = note.row_version;
                }
            }
            shown.refused = Some(t.get(refusal_key(&refusal)));
            form(&state, &session, shown, StatusCode::UNPROCESSABLE_ENTITY)
        }
        Err(BoardError::Busy | BoardError::Unavailable(_)) => {
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

/// `POST /equipe/mural/{id}/remover`: takes a note down.
pub(crate) async fn remove_note(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Form(fields): Form<RemoveFields>,
) -> Response {
    let (session, board) = match open(&state, &headers) {
        Ok(opened) => opened,
        Err(response) => return *response,
    };
    if !tokens_match(&session.csrf, &fields.csrf) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let note = match stored(&board, id).await {
        Ok(note) => note,
        Err(response) => return *response,
    };
    if !may_change(&session, note.audience) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let command = BoardCommand::Remove {
        id: note.id,
        was: note.audience,
    };
    match apply(&board, &session, command).await {
        Ok(()) => Redirect::to("/equipe").into_response(),
        Err(BoardError::Refused(_)) => StatusCode::NOT_FOUND.into_response(),
        Err(BoardError::Busy | BoardError::Unavailable(_)) => {
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

async fn apply(
    board: &Arc<dyn NoticeBoard>,
    session: &ActiveSession,
    command: BoardCommand,
) -> Result<(), BoardError> {
    let board = Arc::clone(board);
    let actor = session.identity.id;
    tokio::task::spawn_blocking(move || board.change(actor, command).map(|_| ()))
        .await
        .map_err(|error| BoardError::Unavailable(error.to_string()))?
}

const fn text_error_key(error: NoteTextError) -> &'static str {
    match error {
        NoteTextError::Empty => "board-error-empty",
        NoteTextError::TooLong => "board-error-long",
    }
}

const fn refusal_key(refusal: &BoardRefusal) -> &'static str {
    match refusal {
        BoardRefusal::Full => "board-error-full",
        BoardRefusal::Stale | BoardRefusal::NotFound => "board-error-stale",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use elzevir_app::staff::{StaffId, StaffIdentity};
    use elzevir_web_templates::I18n;
    use std::collections::BTreeSet;

    fn session(permissions: &[Permission]) -> ActiveSession {
        ActiveSession {
            identity: StaffIdentity {
                id: StaffId(1),
                login: "ana".to_owned(),
                permissions: permissions.iter().copied().collect::<BTreeSet<_>>(),
            },
            csrf: String::new(),
        }
    }

    fn note(id: i64, audience: Audience) -> BoardNote {
        BoardNote {
            id: BoardNoteId(id),
            kind: NoteKind::Urgent,
            audience,
            text: "Reunião".to_owned(),
            author: "ana".to_owned(),
            on: jiff::civil::date(2026, 11, 20),
            row_version: 1,
        }
    }

    #[test]
    fn the_opac_never_names_the_author_nor_links_to_the_form() {
        let shown = cards(vec![note(1, Audience::Public)], None);
        assert_eq!(shown[0].author, None);
        assert_eq!(shown[0].edit, None);
        assert_eq!(shown[0].day, "20/11/2026");
        assert!(I18n::english().unwrap().has(shown[0].kind_key));
    }

    #[test]
    fn only_administrators_get_to_edit_public_notes() {
        let notes = || vec![note(1, Audience::Public), note(2, Audience::Staff)];
        let desk = cards(notes(), Some(&session(&[Permission::StaffInterface])));
        assert_eq!(desk[0].edit, None);
        assert_eq!(desk[1].edit.as_deref(), Some("/equipe/mural/2"));
        assert_eq!(desk[1].color, "note-blue");
        let admin = cards(notes(), Some(&session(&[Permission::Administration])));
        assert_eq!(admin[0].edit.as_deref(), Some("/equipe/mural/1"));
        assert_eq!(admin[0].author.as_deref(), Some("ana"));
    }
}
