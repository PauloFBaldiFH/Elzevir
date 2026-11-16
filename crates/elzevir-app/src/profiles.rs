// SPDX-License-Identifier: GPL-3.0-or-later
//! The typed parts of a Work and an Instance that the form edits with fields of their own
//! (docs/specs/catalogo/edicao.md, items 25 to 27). Each names a part of the model, never
//! a MARC tag; there is no profile per category (bibframe-completo.md, items 24 and 25).

/// A typed part of a Work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkField {
    /// Preferred title.
    Title,
    /// The subclasses of the entity (bibframe-completo.md, item 14).
    Classes,
    /// Contributors with their roles.
    Contributions,
    /// Classification number and scheme.
    Classification,
    /// Language codes.
    Languages,
    /// Subjects.
    Subjects,
    /// Summary.
    Summary,
    /// Target audience.
    Audience,
    /// Relations to other Works: translation, adaptation, part, supplement, continuation.
    Relations,
}

/// A typed part of an Instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceField {
    /// Title as on the publication.
    Title,
    /// Edition statement.
    Edition,
    /// Place of publication.
    Place,
    /// Publisher.
    Publisher,
    /// Date of publication.
    Date,
    /// Physical description.
    Extent,
    /// Series statement.
    Series,
    /// Identifiers.
    Identifiers,
    /// Notes.
    Notes,
    /// The subclasses of the entity (bibframe-completo.md, item 14).
    Classes,
}
