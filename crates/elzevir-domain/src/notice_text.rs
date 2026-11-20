// SPDX-License-Identifier: GPL-3.0-or-later
//! The texts of the notices a library writes in place of the built-in ones, with
//! placeholders filled in when the notice goes out (docs/specs/circulacao/avisos.md,
//! items 34 to 37).

use serde::{Deserialize, Serialize};

use crate::notice::NoticeLevel;

/// Most characters of a subject.
pub const MAX_SUBJECT: usize = 200;

/// Most characters of a body.
pub const MAX_BODY: usize = 8_000;

/// Which notice a text is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum NoticeTextKind {
    /// An overdue notice of this level (Koha: `ODUE`, `ODUE2`, `ODUE3`).
    Overdue(NoticeLevel),
    /// A hold waiting to be picked up (Koha: `HOLD`).
    Hold,
}

impl NoticeTextKind {
    /// Every kind, in the order the screen shows them.
    pub const ALL: [Self; 4] = [
        Self::Overdue(NoticeLevel::First),
        Self::Overdue(NoticeLevel::Second),
        Self::Overdue(NoticeLevel::Third),
        Self::Hold,
    ];

    /// The name stored and sent by forms.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Overdue(NoticeLevel::First) => "OVERDUE1",
            Self::Overdue(NoticeLevel::Second) => "OVERDUE2",
            Self::Overdue(NoticeLevel::Third) => "OVERDUE3",
            Self::Hold => "HOLD",
        }
    }

    /// The kind with this name, if any.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == name)
    }

    /// The placeholders a text of this kind may use.
    #[must_use]
    pub const fn placeholders(self) -> &'static [&'static str] {
        match self {
            Self::Overdue(_) => &["name", "library", "items"],
            Self::Hold => &["name", "library", "items", "date"],
        }
    }
}

/// A subject and a body, with placeholders such as `{name}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoticeText {
    /// The subject, one line.
    pub subject: String,
    /// The body.
    pub body: String,
}

/// Why a text is refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
pub enum NoticeTextError {
    /// The subject or the body is empty.
    #[error("the subject and the body must not be empty")]
    Empty,
    /// The subject is longer than [`MAX_SUBJECT`] or the body than [`MAX_BODY`].
    #[error("the text is too long")]
    TooLong,
    /// The subject has more than one line.
    #[error("the subject is one line")]
    SubjectLines,
    /// A brace that does not hold a placeholder of the kind.
    #[error("unknown placeholder `{0}`")]
    UnknownPlaceholder(String),
}

/// The text with its ends trimmed, if every placeholder is one of the kind's.
///
/// # Errors
///
/// The first [`NoticeTextError`] found.
pub fn checked_text(
    kind: NoticeTextKind,
    text: &NoticeText,
) -> Result<NoticeText, NoticeTextError> {
    let subject = text.subject.trim().to_owned();
    let body = text.body.trim().replace("\r\n", "\n");
    if subject.is_empty() || body.is_empty() {
        return Err(NoticeTextError::Empty);
    }
    if subject.chars().count() > MAX_SUBJECT || body.chars().count() > MAX_BODY {
        return Err(NoticeTextError::TooLong);
    }
    if subject.contains('\n') {
        return Err(NoticeTextError::SubjectLines);
    }
    for part in [&subject, &body] {
        if let Some(unknown) = pieces(part).find_map(|piece| match piece {
            Piece::Placeholder(name) if kind.placeholders().contains(&name) => None,
            Piece::Placeholder(name) => Some(format!("{{{name}}}")),
            Piece::Stray(rest) => Some(rest.chars().take(20).collect()),
            Piece::Text(_) => None,
        }) {
            return Err(NoticeTextError::UnknownPlaceholder(unknown));
        }
    }
    Ok(NoticeText { subject, body })
}

/// `text` with each `{name}` replaced by its value; anything else stays as written.
#[must_use]
pub fn fill(text: &str, values: &[(&str, &str)]) -> String {
    pieces(text)
        .map(|piece| match piece {
            Piece::Placeholder(name) => values
                .iter()
                .find(|(key, _)| *key == name)
                .map_or_else(|| format!("{{{name}}}"), |(_, value)| (*value).to_owned()),
            Piece::Text(text) | Piece::Stray(text) => text.to_owned(),
        })
        .collect()
}

/// A part of a text.
enum Piece<'a> {
    Text(&'a str),
    /// `{name}`, name in lowercase letters.
    Placeholder(&'a str),
    /// A brace that opens no placeholder, and what follows it.
    Stray(&'a str),
}

fn pieces(text: &str) -> impl Iterator<Item = Piece<'_>> {
    let mut rest = text;
    std::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        let Some(open) = rest.find(['{', '}']) else {
            let piece = Piece::Text(rest);
            rest = "";
            return Some(piece);
        };
        if open > 0 {
            let (text, after) = rest.split_at(open);
            rest = after;
            return Some(Piece::Text(text));
        }
        let name = rest
            .strip_prefix('{')
            .and_then(|after| after.split_once('}'))
            .map(|(name, _)| name)
            .filter(|name| !name.is_empty() && name.bytes().all(|b| b.is_ascii_lowercase()));
        if let Some(name) = name {
            rest = rest.get(name.len() + 2..).unwrap_or_default();
            Some(Piece::Placeholder(name))
        } else {
            let piece = Piece::Stray(rest);
            rest = "";
            Some(piece)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(subject: &str, body: &str) -> NoticeText {
        NoticeText {
            subject: subject.to_owned(),
            body: body.to_owned(),
        }
    }

    #[test]
    fn placeholders_are_filled_and_the_rest_stays() {
        assert_eq!(
            fill(
                "Dear {name},\n{items}\n{library}",
                &[
                    ("name", "Ana"),
                    ("items", "- A (1)"),
                    ("library", "Central")
                ]
            ),
            "Dear Ana,\n- A (1)\nCentral"
        );
        assert_eq!(fill("{date} ok", &[]), "{date} ok");
    }

    #[test]
    fn a_text_uses_only_the_placeholders_of_its_kind() {
        let overdue = NoticeTextKind::Overdue(NoticeLevel::First);
        assert_eq!(
            checked_text(
                overdue,
                &text(" Late at {library} ", "Dear {name},\r\n{items}")
            ),
            Ok(text("Late at {library}", "Dear {name},\n{items}"))
        );
        assert_eq!(
            checked_text(overdue, &text("Late", "until {date}")),
            Err(NoticeTextError::UnknownPlaceholder("{date}".to_owned()))
        );
        assert!(checked_text(NoticeTextKind::Hold, &text("Waiting", "until {date}")).is_ok());
        assert_eq!(
            checked_text(overdue, &text("Late", "Dear {Name}")),
            Err(NoticeTextError::UnknownPlaceholder("{Name}".to_owned()))
        );
        assert_eq!(
            checked_text(overdue, &text("Late", "a } b")),
            Err(NoticeTextError::UnknownPlaceholder("} b".to_owned()))
        );
        assert_eq!(
            checked_text(overdue, &text("  ", "body")),
            Err(NoticeTextError::Empty)
        );
        assert_eq!(
            checked_text(overdue, &text("a\nb", "body")),
            Err(NoticeTextError::SubjectLines)
        );
        assert_eq!(
            checked_text(overdue, &text("a", &"x".repeat(MAX_BODY + 1))),
            Err(NoticeTextError::TooLong)
        );
    }

    #[test]
    fn kinds_have_stable_names() {
        for kind in NoticeTextKind::ALL {
            assert_eq!(NoticeTextKind::from_name(kind.as_str()), Some(kind));
        }
        assert_eq!(NoticeTextKind::from_name("ODUE"), None);
    }
}
