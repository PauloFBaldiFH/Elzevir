// SPDX-License-Identifier: GPL-3.0-or-later
//! The notice board on SQLite (docs/specs/interface/mural.md).

use elzevir_app::board::{
    Audience, BoardCommand, BoardError, BoardNote, BoardNoteId, BoardOutput, BoardRefusal,
    MAX_NOTES, NoteContent, NoteKind, NoticeBoard,
};
use elzevir_app::commands::{WriteCommand, WriteOutput, WriteRefusal};
use elzevir_app::staff::StaffId;
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{CommandFailed, ElzevirStore, StoreError, SubmitError, WriteTx};

const COLUMNS: &str = "board_notes.id, board_notes.kind, board_notes.audience, board_notes.text,
     staff_accounts.login, board_notes.day, board_notes.row_version
     FROM board_notes JOIN staff_accounts ON staff_accounts.id = board_notes.author_id";

fn refused(refusal: BoardRefusal) -> CommandFailed<WriteRefusal> {
    CommandFailed::Rejected(WriteRefusal::Board(refusal))
}

/// Applies a board command.
pub(crate) fn apply(
    tx: &WriteTx<'_>,
    command: &BoardCommand,
) -> Result<WriteOutput, CommandFailed<WriteRefusal>> {
    let connection = tx.connection();
    let output = match command {
        BoardCommand::Add(content) => add(connection, content)?,
        BoardCommand::Update {
            id,
            was,
            row_version,
            content,
        } => {
            let stored = stored_version(connection, *id, *was)?;
            if stored != *row_version {
                return Err(refused(BoardRefusal::Stale));
            }
            connection
                .execute(
                    "UPDATE board_notes SET kind = ?1, audience = ?2, text = ?3, author_id = ?4,
                         day = ?5, row_version = row_version + 1
                     WHERE id = ?6",
                    params![
                        content.kind.as_str(),
                        content.audience.as_str(),
                        content.text.as_str(),
                        content.author.0,
                        content.on.to_string(),
                        id.0
                    ],
                )
                .map_err(StoreError::from)?;
            BoardOutput::Done
        }
        BoardCommand::Remove { id, was } => {
            stored_version(connection, *id, *was)?;
            connection
                .execute("DELETE FROM board_notes WHERE id = ?1", params![id.0])
                .map_err(StoreError::from)?;
            BoardOutput::Done
        }
    };
    Ok(WriteOutput::Board(output))
}

fn add(
    connection: &Connection,
    content: &NoteContent,
) -> Result<BoardOutput, CommandFailed<WriteRefusal>> {
    let count: i64 = connection
        .query_row("SELECT count(*) FROM board_notes", [], |row| row.get(0))
        .map_err(StoreError::from)?;
    if usize::try_from(count).unwrap_or(usize::MAX) >= MAX_NOTES {
        return Err(refused(BoardRefusal::Full));
    }
    connection
        .execute(
            "INSERT INTO board_notes (kind, audience, text, author_id, day)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                content.kind.as_str(),
                content.audience.as_str(),
                content.text.as_str(),
                content.author.0,
                content.on.to_string()
            ],
        )
        .map_err(StoreError::from)?;
    Ok(BoardOutput::Added(BoardNoteId(
        connection.last_insert_rowid(),
    )))
}

/// The version of note `id`, which must have audience `was`: a note seen by another
/// audience is refused as missing, so the permission checked from the command holds.
fn stored_version(
    connection: &Connection,
    id: BoardNoteId,
    was: Audience,
) -> Result<i64, CommandFailed<WriteRefusal>> {
    connection
        .query_row(
            "SELECT row_version FROM board_notes WHERE id = ?1 AND audience = ?2",
            params![id.0, was.as_str()],
            |row| row.get(0),
        )
        .optional()
        .map_err(StoreError::from)?
        .ok_or_else(|| refused(BoardRefusal::NotFound))
}

fn note(row: &Row<'_>) -> rusqlite::Result<BoardNote> {
    let text_column = |index: usize, name: &str| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Text,
            format!("unexpected board value `{name}`").into(),
        )
    };
    let kind: String = row.get(1)?;
    let audience: String = row.get(2)?;
    let day: String = row.get(5)?;
    Ok(BoardNote {
        id: BoardNoteId(row.get(0)?),
        kind: NoteKind::from_name(&kind).ok_or_else(|| text_column(1, &kind))?,
        audience: Audience::from_name(&audience).ok_or_else(|| text_column(2, &audience))?,
        text: row.get(3)?,
        author: row.get(4)?,
        on: day.parse().map_err(|_| text_column(5, &day))?,
        row_version: row.get(6)?,
    })
}

impl NoticeBoard for ElzevirStore {
    fn notes(
        &self,
        audience: Option<Audience>,
        limit: usize,
    ) -> Result<Vec<BoardNote>, BoardError> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        self.readers()
            .read(|c| {
                // The newest change first; the id breaks ties of the same day.
                let mut statement = c.prepare_cached(&format!(
                    "SELECT {COLUMNS}
                     WHERE ?1 IS NULL OR board_notes.audience = ?1
                     ORDER BY board_notes.day DESC, board_notes.id DESC LIMIT ?2"
                ))?;
                statement
                    .query_map(params![audience.map(Audience::as_str), limit], note)?
                    .collect::<Result<Vec<_>, _>>()
            })
            .map_err(|error: rusqlite::Error| BoardError::Unavailable(error.to_string()))
    }

    fn note(&self, id: BoardNoteId) -> Result<Option<BoardNote>, BoardError> {
        self.readers()
            .read(|c| {
                c.query_row(
                    &format!("SELECT {COLUMNS} WHERE board_notes.id = ?1"),
                    params![id.0],
                    note,
                )
                .optional()
            })
            .map_err(|error: rusqlite::Error| BoardError::Unavailable(error.to_string()))
    }

    fn change(&self, actor: StaffId, command: BoardCommand) -> Result<BoardOutput, BoardError> {
        let pending = self
            .submit_as(actor.0, WriteCommand::Board(command))
            .map_err(|error| match error {
                SubmitError::Busy => BoardError::Busy,
                SubmitError::Closed => BoardError::Unavailable(error.to_string()),
            })?;
        match pending.wait() {
            Ok(WriteOutput::Board(output)) => Ok(output),
            Ok(_) => Err(BoardError::Unavailable(
                "the writer answered with another result".to_owned(),
            )),
            Err(CommandFailed::Rejected(WriteRefusal::Board(refusal))) => {
                Err(BoardError::Refused(refusal))
            }
            Err(failure) => Err(BoardError::Unavailable(failure.to_string())),
        }
    }
}
