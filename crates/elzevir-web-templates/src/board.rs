// SPDX-License-Identifier: GPL-3.0-or-later
//! The notice board of the homes and its form (docs/specs/interface/mural.md).

use askama::Template;

use crate::I18n;

/// The draft's note colours, in turn (spec, item 5).
const COLORS: [&str; 4] = ["note-yellow", "note-blue", "note-pink", "note-green"];

/// The colour of the note at `position` on the board.
#[must_use]
pub fn note_color(position: usize) -> &'static str {
    COLORS
        .get(position % COLORS.len())
        .copied()
        .unwrap_or("note-yellow")
}

/// One note as a home shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardCard {
    /// Where it is edited; `None` on the OPAC.
    pub edit: Option<String>,
    /// Fluent key of its kind.
    pub kind_key: &'static str,
    /// Its text, line breaks kept by the style sheet.
    pub text: String,
    /// Who wrote it; `None` on the OPAC (spec, item 6).
    pub author: Option<String>,
    /// The day of the last change, as shown.
    pub day: String,
    /// Its colour class.
    pub color: &'static str,
    /// Whether the OPAC shows it too.
    pub public: bool,
}

/// A kind of note the form offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KindChoice {
    /// The value sent.
    pub value: &'static str,
    /// Fluent key of its name.
    pub label_key: &'static str,
    /// Whether it is chosen.
    pub selected: bool,
}

/// The form of a new or an existing note.
#[derive(Template)]
#[template(path = "staff_board_note.html")]
pub struct StaffBoardNote<'a> {
    /// Interface texts.
    pub t: &'a I18n,
    /// Anti-forgery token of the session.
    pub csrf: &'a str,
    /// Where the form posts.
    pub action: String,
    /// Where the remove button posts; `None` for a new note.
    pub remove: Option<String>,
    /// The version edited; 0 for a new note.
    pub row_version: i64,
    /// The text, as stored or typed.
    pub text: String,
    /// The kinds, one chosen.
    pub kinds: Vec<KindChoice>,
    /// Whether the note is public.
    pub public: bool,
    /// Whether this operator may publish on the OPAC.
    pub may_publish: bool,
    /// Why the last change was refused, as shown.
    pub refused: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_take_turns() {
        assert_eq!(note_color(0), "note-yellow");
        assert_eq!(note_color(3), "note-green");
        assert_eq!(note_color(4), "note-yellow");
    }
}
