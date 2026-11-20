// SPDX-License-Identifier: GPL-3.0-or-later
//! The notice board of the staff home and the OPAC home (docs/specs/interface/mural.md).

use jiff::civil::Date;
use serde::{Deserialize, Serialize};

use crate::staff::StaffId;

/// Most notes the board keeps (spec, item 2).
pub const MAX_NOTES: usize = 50;

/// Most characters of a note's text (spec, item 1).
pub const MAX_TEXT_CHARS: usize = 1400;

/// Most public notes the OPAC home shows (spec, item 6).
pub const OPAC_NOTES: usize = 6;

/// `board_notes.id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BoardNoteId(pub i64);

/// Who sees a note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Audience {
    /// The staff home only.
    Staff,
    /// The staff home and the OPAC home.
    Public,
}

impl Audience {
    /// The value stored in `board_notes.audience`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Staff => "staff",
            Self::Public => "public",
        }
    }

    /// The audience stored as `name`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "staff" => Some(Self::Staff),
            "public" => Some(Self::Public),
            _ => None,
        }
    }
}

/// The kind of a note, as the draft labels it (spec, item 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NoteKind {
    /// For the team.
    Staff,
    /// From the institution.
    Institutional,
    /// A personal reminder.
    Personal,
    /// Needs attention now.
    Urgent,
}

impl NoteKind {
    /// Every kind, in the order the form offers them.
    pub const ALL: [Self; 4] = [
        Self::Staff,
        Self::Institutional,
        Self::Personal,
        Self::Urgent,
    ];

    /// The value stored in `board_notes.kind` and sent by the form.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Staff => "staff",
            Self::Institutional => "institutional",
            Self::Personal => "personal",
            Self::Urgent => "urgent",
        }
    }

    /// The kind stored as `name`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == name)
    }
}

/// The text of a note: trimmed, not empty, at most [`MAX_TEXT_CHARS`] characters, with
/// line breaks as `\n`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteText(String);

/// Why a text cannot be a note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NoteTextError {
    /// Nothing but spaces.
    #[error("the note is empty")]
    Empty,
    /// Longer than [`MAX_TEXT_CHARS`].
    #[error("the note is longer than {MAX_TEXT_CHARS} characters")]
    TooLong,
}

impl NoteText {
    /// Checks `typed` as a note's text.
    ///
    /// # Errors
    /// [`NoteTextError`] when it is empty or too long.
    pub fn new(typed: &str) -> Result<Self, NoteTextError> {
        let text = typed.replace("\r\n", "\n");
        let text = text.trim();
        if text.is_empty() {
            return Err(NoteTextError::Empty);
        }
        if text.chars().count() > MAX_TEXT_CHARS {
            return Err(NoteTextError::TooLong);
        }
        Ok(Self(text.to_owned()))
    }

    /// The text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What a note says and who sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteContent {
    /// Its kind.
    pub kind: NoteKind,
    /// Who sees it.
    pub audience: Audience,
    /// Its text.
    pub text: NoteText,
    /// Who wrote it.
    pub author: StaffId,
    /// The library's day of the change.
    pub on: Date,
}

/// A change to the board.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BoardCommand {
    /// A new note.
    Add(NoteContent),
    /// A note rewritten whole.
    Update {
        /// The note.
        id: BoardNoteId,
        /// Who saw it until now; checked against the stored note (spec, item 3).
        was: Audience,
        /// The version the person edited.
        row_version: i64,
        /// What it says now.
        content: NoteContent,
    },
    /// A note taken down.
    Remove {
        /// The note.
        id: BoardNoteId,
        /// Who saw it; checked against the stored note.
        was: Audience,
    },
}

impl BoardCommand {
    /// Whether the command touches what the public sees, which needs administration.
    #[must_use]
    pub const fn is_public(&self) -> bool {
        match self {
            Self::Add(content) => matches!(content.audience, Audience::Public),
            Self::Update { was, content, .. } => {
                matches!(was, Audience::Public) || matches!(content.audience, Audience::Public)
            }
            Self::Remove { was, .. } => matches!(was, Audience::Public),
        }
    }
}

/// What a successful board command returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardOutput {
    /// The note added.
    Added(BoardNoteId),
    /// The note changed or removed.
    Done,
}

/// Why the board refused a change.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BoardRefusal {
    /// No such note, or it has another audience than the command says.
    #[error("no such note")]
    NotFound,
    /// Someone changed the note since it was read.
    #[error("the note changed meanwhile")]
    Stale,
    /// The board already keeps [`MAX_NOTES`] notes.
    #[error("the board keeps at most {MAX_NOTES} notes")]
    Full,
}

/// A note as the homes show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardNote {
    /// The note.
    pub id: BoardNoteId,
    /// Its kind.
    pub kind: NoteKind,
    /// Who sees it.
    pub audience: Audience,
    /// Its text.
    pub text: String,
    /// The login of who wrote it.
    pub author: String,
    /// The day of the last change.
    pub on: Date,
    /// Its version, for an edit.
    pub row_version: i64,
}

/// The board failed or refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BoardError {
    /// The change was refused.
    #[error(transparent)]
    Refused(#[from] BoardRefusal),
    /// The writer queue is full; try again.
    #[error("the writer is busy")]
    Busy,
    /// Storage failed.
    #[error("notice board unavailable: {0}")]
    Unavailable(String),
}

/// The notice board.
pub trait NoticeBoard: Send + Sync {
    /// The notes `audience` sees, the newest first; `None` is every note.
    ///
    /// # Errors
    /// [`BoardError::Unavailable`] when storage fails.
    fn notes(&self, audience: Option<Audience>, limit: usize)
    -> Result<Vec<BoardNote>, BoardError>;

    /// One note.
    ///
    /// # Errors
    /// [`BoardError::Unavailable`] when storage fails.
    fn note(&self, id: BoardNoteId) -> Result<Option<BoardNote>, BoardError>;

    /// Applies `command` as `actor`, waiting for the writer.
    ///
    /// # Errors
    /// [`BoardError`] when it is refused or storage fails.
    fn change(&self, actor: StaffId, command: BoardCommand) -> Result<BoardOutput, BoardError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_note_text_is_trimmed_and_bounded() {
        assert_eq!(
            NoteText::new("  Reunião\r\nàs 14h  ").unwrap().as_str(),
            "Reunião\nàs 14h"
        );
        assert_eq!(NoteText::new(" \n "), Err(NoteTextError::Empty));
        assert!(NoteText::new(&"é".repeat(MAX_TEXT_CHARS)).is_ok());
        assert_eq!(
            NoteText::new(&"é".repeat(MAX_TEXT_CHARS + 1)),
            Err(NoteTextError::TooLong)
        );
    }

    #[test]
    fn touching_a_public_note_is_public() {
        let content = |audience| NoteContent {
            kind: NoteKind::Staff,
            audience,
            text: NoteText::new("x").unwrap(),
            author: StaffId(1),
            on: jiff::civil::date(2026, 11, 20),
        };
        assert!(!BoardCommand::Add(content(Audience::Staff)).is_public());
        assert!(BoardCommand::Add(content(Audience::Public)).is_public());
        let update = |was, now| BoardCommand::Update {
            id: BoardNoteId(1),
            was,
            row_version: 1,
            content: content(now),
        };
        assert!(update(Audience::Public, Audience::Staff).is_public());
        assert!(update(Audience::Staff, Audience::Public).is_public());
        assert!(!update(Audience::Staff, Audience::Staff).is_public());
        assert!(
            BoardCommand::Remove {
                id: BoardNoteId(1),
                was: Audience::Public
            }
            .is_public()
        );
    }

    #[test]
    fn kinds_round_trip() {
        for kind in NoteKind::ALL {
            assert_eq!(NoteKind::from_name(kind.as_str()), Some(kind));
        }
        assert_eq!(Audience::from_name("public"), Some(Audience::Public));
        assert_eq!(NoteKind::from_name("other"), None);
    }
}
