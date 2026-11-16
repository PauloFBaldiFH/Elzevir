// SPDX-License-Identifier: GPL-3.0-or-later
//! Editing Works and Instances from the staff screen (docs/specs/catalogo/edicao.md,
//! items 16 to 24, and docs/specs/catalogo/criacao.md): content validation before the command, and the port the store
//! implements.

use elzevir_domain::catalog::{IdentifierKind, isbn10, isbn13};
use elzevir_domain::statement::{Description, EntityKind, check_description};

use crate::catalog::{
    CatalogOutput, CatalogRefusal, ClassChoice, InstanceChange, InstanceUpdate, NewIdentifier,
    NewInstanceOfWork, NewRecord, PropertyEdit, WorkChange, WorkUpdate,
};
use crate::items::ItemsUpdate;
use crate::registries::RegistryChange;
use crate::staff::StaffId;

/// One edit from the screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    /// Parts of a Work.
    Work(WorkUpdate),
    /// Parts of an Instance.
    Instance(InstanceUpdate),
    /// A new family: a Work and its first Instance (docs/specs/catalogo/criacao.md).
    NewRecord(Box<NewRecord>),
    /// A new Instance of a stored Work (docs/specs/catalogo/criacao.md).
    NewInstance(Box<NewInstanceOfWork>),
    /// A batch of an Instance's Items (docs/specs/catalogo/exemplares.md).
    Items(Box<ItemsUpdate>),
    /// A change to the registries (docs/specs/catalogo/bibframe-completo.md, item 23d).
    Registry(RegistryChange),
    /// The statements of some properties of a Hub or an Item (bibframe-completo.md,
    /// items 24 to 26).
    Statements(Box<PropertyEdit>),
}

/// Why an edit was not stored.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EditFailed {
    /// The catalog refused it (stale version, unknown record).
    #[error(transparent)]
    Refused(#[from] CatalogRefusal),
    /// The writer queue is full; try again shortly.
    #[error("the catalog is busy")]
    Busy,
    /// Storage failed or the writer stopped.
    #[error("the catalog is unavailable: {0}")]
    Unavailable(String),
}

/// Port: applies an edit on behalf of a staff account. The store implements it with the
/// writer queue; the call blocks until the writer answers.
pub trait CatalogEditor {
    /// Applies `edit` as `actor`, who is recorded in the audit log.
    ///
    /// # Errors
    ///
    /// [`EditFailed`] when the edit is refused or cannot be stored.
    fn apply(&self, actor: StaffId, edit: Edit) -> Result<CatalogOutput, EditFailed>;
}

/// A field the validation refused, and the message key that says why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldError {
    /// The part of the record, as named in the edit (`title`, `identifiers`).
    pub field: &'static str,
    /// Fluent key of the message.
    pub message_key: &'static str,
}

/// Validates and normalizes the changes of a Work edit (item 19). Every error is
/// reported at once; nothing is stored when there is one.
///
/// # Errors
///
/// The fields that cannot be stored as written.
pub fn validate_work_changes(changes: &mut [WorkChange]) -> Result<(), Vec<FieldError>> {
    let mut errors = Vec::new();
    for change in changes.iter_mut() {
        match change {
            WorkChange::Title(title) => {
                trim(title);
                if title.is_empty() {
                    errors.push(error("title", "edit-error-title-empty"));
                }
            }
            WorkChange::Classes(classes) => {
                if !normalize_classes(EntityKind::Work, classes) {
                    errors.push(error("classes", "edit-error-class"));
                }
            }
            WorkChange::Classification { number, scheme } => {
                optional(number);
                optional(scheme);
            }
            WorkChange::Summary(text) | WorkChange::Audience(text) => optional(text),
            WorkChange::Languages(codes) => {
                for code in codes.iter_mut() {
                    trim(code);
                }
                codes.retain(|code| !code.is_empty());
                if !codes.iter().all(|code| is_language_code(code)) {
                    errors.push(error("languages", "edit-error-language-code"));
                }
            }
            WorkChange::Contributions(contributions) => {
                for contribution in contributions.iter_mut() {
                    trim(&mut contribution.heading);
                    optional(&mut contribution.role);
                }
                contributions.retain(|contribution| !contribution.heading.is_empty());
            }
            WorkChange::Subjects(subjects) => {
                for subject in subjects.iter_mut() {
                    trim(&mut subject.heading);
                    optional(&mut subject.thesaurus);
                }
                subjects.retain(|subject| !subject.heading.is_empty());
            }
            // The form's rows were read and checked by `statement_rows`; the store checks
            // the whole description again.
            WorkChange::Relations(_) | WorkChange::Statements { .. } => {}
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Validates and normalizes the changes of an Instance edit (item 19).
///
/// # Errors
///
/// The fields that cannot be stored as written.
pub fn validate_instance_changes(changes: &mut [InstanceChange]) -> Result<(), Vec<FieldError>> {
    let mut errors = Vec::new();
    for change in changes.iter_mut() {
        match change {
            InstanceChange::Title(title) => {
                trim(title);
                if title.is_empty() {
                    errors.push(error("title", "edit-error-title-empty"));
                }
            }
            InstanceChange::Classes(classes) => {
                if !normalize_classes(EntityKind::Instance, classes) {
                    errors.push(error("classes", "edit-error-class"));
                }
            }
            InstanceChange::Edition(text)
            | InstanceChange::Place(text)
            | InstanceChange::Publisher(text)
            | InstanceChange::Date(text)
            | InstanceChange::Extent(text)
            | InstanceChange::Series(text) => optional(text),
            InstanceChange::Identifiers(identifiers) => {
                for identifier in identifiers.iter_mut() {
                    trim(&mut identifier.value);
                    optional(&mut identifier.qualifier);
                }
                identifiers.retain(|identifier| !identifier.value.is_empty());
                for identifier in identifiers.iter_mut() {
                    if let Err(message_key) = normalize_identifier(identifier) {
                        errors.push(error("identifiers", message_key));
                    }
                }
            }
            InstanceChange::Notes(notes) => {
                for note in notes.iter_mut() {
                    trim(&mut note.text);
                }
                notes.retain(|note| !note.text.is_empty());
            }
            // Read and checked by `statement_rows`; the store checks them again.
            InstanceChange::Statements { .. } | InstanceChange::OpacHidden(_) => {}
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Trims the chosen classes and drops the empty rows and repeats; whether every one left
/// is a subclass of the entity's class, as the store will check again.
fn normalize_classes(entity: EntityKind, classes: &mut Vec<ClassChoice>) -> bool {
    for choice in classes.iter_mut() {
        trim(&mut choice.class);
    }
    let mut seen = Vec::new();
    classes.retain(|choice| {
        let keep = !choice.class.is_empty() && !seen.contains(&choice.class);
        if keep {
            seen.push(choice.class.clone());
        }
        keep
    });
    check_description(
        entity,
        &Description {
            classes: seen,
            statements: Vec::new(),
        },
    )
    .is_ok()
}

const fn error(field: &'static str, message_key: &'static str) -> FieldError {
    FieldError { field, message_key }
}

fn trim(text: &mut String) {
    let trimmed = text.trim();
    if trimmed.len() != text.len() {
        *text = trimmed.to_owned();
    }
}

/// Trims an optional text; an empty one clears the field.
fn optional(text: &mut Option<String>) {
    if let Some(value) = text {
        trim(value);
        if value.is_empty() {
            *text = None;
        }
    }
}

fn is_language_code(code: &str) -> bool {
    code.len() == 3 && code.bytes().all(|byte| byte.is_ascii_lowercase())
}

/// ISBN and ISSN are stored without separators (ISBN) or as `NNNN-NNNC` (ISSN), with a
/// right check digit; other identifiers are stored as written.
fn normalize_identifier(identifier: &mut NewIdentifier) -> Result<(), &'static str> {
    match identifier.kind {
        IdentifierKind::Isbn => {
            let compact = compact(&identifier.value);
            if !is_isbn(&compact) {
                return Err("edit-error-isbn");
            }
            identifier.value = compact;
        }
        IdentifierKind::Issn => {
            let compact = compact(&identifier.value);
            if !is_issn(&compact) {
                return Err("edit-error-issn");
            }
            let (left, right) = compact.split_at(4);
            identifier.value = format!("{left}-{right}");
        }
        _ => {}
    }
    Ok(())
}

/// The identifier without hyphens and spaces, with a lowercase check `x` made uppercase.
fn compact(value: &str) -> String {
    value
        .chars()
        .filter(|character| *character != '-' && !character.is_whitespace())
        .map(|character| character.to_ascii_uppercase())
        .collect()
}

/// The value of each character of an ISBN-10 or ISSN, `X` counting 10 in the last place.
fn digit_values(value: &str) -> Option<Vec<u32>> {
    let last = value.len().checked_sub(1)?;
    value
        .chars()
        .enumerate()
        .map(|(index, character)| match character {
            'X' if index == last => Some(10),
            _ => character.to_digit(10),
        })
        .collect()
}

/// The rule of [`isbn13`], on a value already without separators: digits only, `X` only
/// as the last character of an ISBN-10.
fn is_isbn(value: &str) -> bool {
    let last = value.len().saturating_sub(1);
    value
        .chars()
        .enumerate()
        .all(|(index, character)| character.is_ascii_digit() || (character == 'X' && index == last))
        && isbn13(value).is_some()
}

/// What the ISBN field says when the cataloguer leaves it (docs/specs/catalogo/edicao.md,
/// item 38).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IsbnCheck {
    /// Nothing typed: nothing to say.
    Empty,
    /// Not an ISBN: wrong length, characters or check digit.
    Invalid,
    /// An ISBN, written as the field keeps it: an ISBN-10 becomes its ISBN-13.
    Valid {
        /// The ISBN-13 the field holds.
        isbn13: String,
        /// The ISBN-10 that was typed, when one was.
        typed_isbn10: Option<String>,
        /// The ISBN-10 of a `978` ISBN-13 that was typed.
        isbn10: Option<String>,
    },
}

/// Checks the ISBN typed in a field, by the same rule the edit command uses.
#[must_use]
pub fn check_isbn(typed: &str) -> IsbnCheck {
    let compact = compact(typed);
    if compact.is_empty() {
        return IsbnCheck::Empty;
    }
    if !is_isbn(&compact) {
        return IsbnCheck::Invalid;
    }
    let Some(thirteen) = isbn13(&compact) else {
        return IsbnCheck::Invalid;
    };
    if compact.len() == 10 {
        IsbnCheck::Valid {
            isbn13: thirteen,
            typed_isbn10: Some(compact),
            isbn10: None,
        }
    } else {
        IsbnCheck::Valid {
            isbn10: isbn10(&thirteen),
            isbn13: thirteen,
            typed_isbn10: None,
        }
    }
}

fn is_issn(value: &str) -> bool {
    value.len() == 8
        && digit_values(value).is_some_and(|digits| {
            // Weights 8 down to 1; the sum is a multiple of 11.
            digits
                .iter()
                .zip((1..=8).rev())
                .map(|(digit, weight)| digit * weight)
                .sum::<u32>()
                % 11
                == 0
        })
}

#[cfg(test)]
mod tests {
    use elzevir_domain::catalog::NoteKind;

    use super::*;
    use crate::catalog::NewNote;

    fn identifier(kind: IdentifierKind, value: &str) -> NewIdentifier {
        NewIdentifier {
            kind,
            value: value.to_owned(),
            qualifier: Some("  ".to_owned()),
        }
    }

    #[test]
    fn the_isbn_field_is_checked_and_an_isbn10_becomes_an_isbn13() {
        assert_eq!(check_isbn("  "), IsbnCheck::Empty);
        assert_eq!(check_isbn("85-60342-01-3"), IsbnCheck::Invalid);
        assert_eq!(check_isbn("9778560342013"), IsbnCheck::Invalid);
        assert_eq!(check_isbn("85603X2016"), IsbnCheck::Invalid);
        assert_eq!(
            check_isbn("85-60342-01-x"),
            IsbnCheck::Valid {
                isbn13: "9788560342013".to_owned(),
                typed_isbn10: Some("856034201X".to_owned()),
                isbn10: None,
            }
        );
        assert_eq!(
            check_isbn("978-85-60342-01-3"),
            IsbnCheck::Valid {
                isbn13: "9788560342013".to_owned(),
                typed_isbn10: None,
                isbn10: Some("856034201X".to_owned()),
            }
        );
        assert_eq!(
            check_isbn("979 10 90636 07 1"),
            IsbnCheck::Valid {
                isbn13: "9791090636071".to_owned(),
                typed_isbn10: None,
                isbn10: None,
            }
        );
    }

    #[test]
    fn isbn_and_issn_check_digits() {
        assert!(is_isbn("8571390584"));
        assert!(is_isbn("080442957X"));
        assert!(!is_isbn("0804429571"));
        assert!(is_isbn("9788535910667"));
        assert!(!is_isbn("9788535910668"));
        assert!(!is_isbn("857139058"));
        assert!(!is_isbn("978853591066X"));
        assert!(is_issn("03785955"));
        assert!(is_issn("2434561X"));
        assert!(!is_issn("03785956"));
    }

    #[test]
    fn identifiers_are_normalized_or_refused() {
        let mut changes = [InstanceChange::Identifiers(vec![
            identifier(IdentifierKind::Isbn, " 978-85-359-1066-7 "),
            identifier(IdentifierKind::Issn, "2434 561x"),
            identifier(IdentifierKind::Lccn, " 2001012345 "),
            identifier(IdentifierKind::Isbn, ""),
        ])];
        validate_instance_changes(&mut changes).unwrap();
        let [InstanceChange::Identifiers(identifiers)] = &changes else {
            panic!("the change keeps its kind")
        };
        let values: Vec<_> = identifiers.iter().map(|id| id.value.as_str()).collect();
        assert_eq!(values, ["9788535910667", "2434-561X", "2001012345"]);
        assert!(identifiers.iter().all(|id| id.qualifier.is_none()));

        let mut wrong = [InstanceChange::Identifiers(vec![
            identifier(IdentifierKind::Isbn, "9788535910668"),
            identifier(IdentifierKind::Issn, "03785956"),
        ])];
        assert_eq!(
            validate_instance_changes(&mut wrong).unwrap_err(),
            [
                error("identifiers", "edit-error-isbn"),
                error("identifiers", "edit-error-issn"),
            ]
        );
    }

    #[test]
    fn titles_cannot_be_empty_and_empty_text_clears_a_field() {
        let mut work = [
            WorkChange::Title("   ".to_owned()),
            WorkChange::Summary(Some("  ".to_owned())),
            WorkChange::Languages(vec![" por ".to_owned(), "PT".to_owned()]),
        ];
        assert_eq!(
            validate_work_changes(&mut work).unwrap_err(),
            [
                error("title", "edit-error-title-empty"),
                error("languages", "edit-error-language-code"),
            ]
        );
        assert_eq!(work[1], WorkChange::Summary(None));

        let mut instance = [
            InstanceChange::Title(" Dom Casmurro ".to_owned()),
            InstanceChange::Edition(Some(String::new())),
            InstanceChange::Notes(vec![NewNote {
                kind: NoteKind::General,
                text: " ".to_owned(),
            }]),
        ];
        validate_instance_changes(&mut instance).unwrap();
        assert_eq!(
            instance[0],
            InstanceChange::Title("Dom Casmurro".to_owned())
        );
        assert_eq!(instance[1], InstanceChange::Edition(None));
        assert_eq!(instance[2], InstanceChange::Notes(Vec::new()));
    }
}
