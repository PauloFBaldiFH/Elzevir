// SPDX-License-Identifier: GPL-3.0-or-later
//! Additional patron fields the library defines, and the values patrons hold
//! (docs/specs/circulacao/patron-attributes.md).

use elzevir_domain::patron::{PatronCategoryId, PatronFieldId, PatronFieldOptionId};
use serde::{Deserialize, Serialize};

use crate::patrons::{MAX_NAME, PatronRefusal};

/// Most options a list field has (spec, item 2).
pub const MAX_OPTIONS: usize = 200;
/// Longest text value (Koha: `borrower_attributes.attribute`, `varchar(255)`).
pub const MAX_TEXT_VALUE: usize = 255;
/// Most values a patron holds for one repeatable field (spec, item 8).
pub const MAX_VALUES_PER_FIELD: usize = 20;

/// What a field holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldKind {
    /// Free text.
    Text,
    /// A date.
    Date,
    /// One of the options, in the library's order.
    List(Vec<FieldOption>),
}

impl FieldKind {
    /// Whether two kinds hold the same sort of value, whatever their options.
    #[must_use]
    pub const fn same_sort(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (Self::Text, Self::Text) | (Self::Date, Self::Date) | (Self::List(_), Self::List(_))
        )
    }
}

/// One option of a list field: an existing one keeps its id, so a new label renames it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldOption {
    /// The option, `None` for a new one.
    pub id: Option<PatronFieldOptionId>,
    /// Its label.
    pub label: String,
}

/// Everything the library sets on a field (spec, item 1).
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is a separate setting of the field (Koha keeps them as separate columns too)"
)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatronFieldSpec {
    /// Its name, unique among the fields.
    pub name: String,
    /// What it holds.
    pub kind: FieldKind,
    /// Whether a patron can hold more than one value.
    pub repeatable: bool,
    /// Whether two patrons can share a value.
    pub unique: bool,
    /// Whether the staff form refuses a patron without a value.
    pub required: bool,
    /// The category it applies to; `None` for every patron.
    pub category: Option<PatronCategoryId>,
    /// Whether the patron sees it on their OPAC account.
    pub opac_shown: bool,
    /// Whether the staff patron search finds patrons by it.
    pub searchable: bool,
}

/// One change to the additional fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldChange {
    /// A new field.
    Create(PatronFieldSpec),
    /// Replaces every setting of a field; options left out are removed.
    Update {
        /// The field.
        field: PatronFieldId,
        /// Its settings, as they should be.
        spec: PatronFieldSpec,
    },
    /// Deletes a field no patron holds a value for.
    Delete(PatronFieldId),
}

/// A field as the registry lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatronFieldLine {
    /// The field.
    pub id: PatronFieldId,
    /// Its settings, with the options as stored.
    pub spec: PatronFieldSpec,
    /// How many patrons hold a value for it.
    pub patrons: u64,
}

/// The field as stored: name and labels trimmed.
///
/// # Errors
///
/// [`PatronRefusal::BadFieldName`] for an empty or long name;
/// [`PatronRefusal::BadFieldOptions`] for a list with no option, too many, or an empty,
/// long or repeated label.
pub fn checked_field(spec: PatronFieldSpec) -> Result<PatronFieldSpec, PatronRefusal> {
    let name = trimmed(&spec.name, MAX_NAME).ok_or(PatronRefusal::BadFieldName)?;
    let kind = match spec.kind {
        FieldKind::List(options) => {
            if options.is_empty() || options.len() > MAX_OPTIONS {
                return Err(PatronRefusal::BadFieldOptions);
            }
            let mut checked: Vec<FieldOption> = Vec::with_capacity(options.len());
            for option in options {
                let label =
                    trimmed(&option.label, MAX_NAME).ok_or(PatronRefusal::BadFieldOptions)?;
                if label.chars().any(char::is_control)
                    || checked.iter().any(|seen| seen.label == label)
                {
                    return Err(PatronRefusal::BadFieldOptions);
                }
                checked.push(FieldOption {
                    id: option.id,
                    label,
                });
            }
            FieldKind::List(checked)
        }
        other => other,
    };
    Ok(PatronFieldSpec { name, kind, ..spec })
}

/// `text` trimmed, when it is not empty and fits in `max` characters.
fn trimmed(text: &str, max: usize) -> Option<String> {
    let text = text.trim();
    (!text.is_empty() && text.chars().count() <= max).then(|| text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(kind: FieldKind) -> PatronFieldSpec {
        PatronFieldSpec {
            name: " School ".into(),
            kind,
            repeatable: false,
            unique: false,
            required: false,
            category: None,
            opac_shown: false,
            searchable: true,
        }
    }

    fn option(label: &str) -> FieldOption {
        FieldOption {
            id: None,
            label: label.into(),
        }
    }

    #[test]
    fn names_and_labels_are_trimmed() {
        let checked = checked_field(spec(FieldKind::List(vec![option(" Morning ")]))).unwrap();
        assert_eq!(checked.name, "School");
        assert_eq!(checked.kind, FieldKind::List(vec![option("Morning")]));
    }

    #[test]
    fn a_name_is_required_and_bounded() {
        let mut empty = spec(FieldKind::Text);
        empty.name = "  ".into();
        assert_eq!(checked_field(empty), Err(PatronRefusal::BadFieldName));
        let mut long = spec(FieldKind::Text);
        long.name = "x".repeat(MAX_NAME + 1);
        assert_eq!(checked_field(long), Err(PatronRefusal::BadFieldName));
    }

    #[test]
    fn a_list_needs_distinct_labels_within_the_limit() {
        let refused = Err(PatronRefusal::BadFieldOptions);
        assert_eq!(checked_field(spec(FieldKind::List(vec![]))), refused);
        assert_eq!(
            checked_field(spec(FieldKind::List(vec![option("A"), option(" A")]))),
            refused
        );
        assert_eq!(
            checked_field(spec(FieldKind::List(vec![option(" ")]))),
            refused
        );
        let many = (0..=MAX_OPTIONS).map(|n| option(&n.to_string())).collect();
        assert_eq!(checked_field(spec(FieldKind::List(many))), refused);
    }

    #[test]
    fn kinds_compare_by_sort_not_options() {
        assert!(FieldKind::List(vec![]).same_sort(&FieldKind::List(vec![option("A")])));
        assert!(!FieldKind::Text.same_sort(&FieldKind::Date));
    }
}
