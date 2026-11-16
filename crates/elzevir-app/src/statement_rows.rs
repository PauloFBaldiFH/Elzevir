// SPDX-License-Identifier: GPL-3.0-or-later
//! One BIBFRAME property of an entity as rows of the cataloguing form
//! (docs/specs/catalogo/bibframe-completo.md, items 24 to 26).
//!
//! Every property gets the same few cells: a literal is a text and its language; any
//! other value is a concept of the property's vocabulary, an IRI, or a resource named by
//! its label. Each row also carries the stored value it was drawn from, so a row left as
//! it was keeps what the cells do not show (a resource's own statements, an entity of
//! the catalogue).

use elzevir_data::bibframe::{self, Property, PropertyKind};
use elzevir_data::vocabulary;
use elzevir_domain::statement::{self, EntityKind, Resource, Statement, Value};
use serde::{Deserialize, Serialize};

use crate::editing::FieldError;

/// The message of a property whose rows cannot be stored.
const REFUSED: &str = "edit-error-statement";

/// The class a new resource takes where the property names none the form can use.
const NEW_CLASSES: &[(&str, &str)] = &[
    // Only a variant title can be a statement: the preferred one has its own field.
    ("title", "VariantTitle"),
    ("subject", "Topic"),
    ("originPlace", "Place"),
];

/// One row as the form shows and sends it; an empty cell is `None`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StatementRow {
    /// A literal's text, or a resource's label.
    pub text: Option<String>,
    /// A literal's language tag.
    pub language: Option<String>,
    /// A concept, as `scheme/code`.
    pub concept: Option<String>,
    /// An outside resource.
    pub iri: Option<String>,
    /// The stored value the row was drawn from, as JSON.
    pub kept: Option<String>,
}

/// Every row of one property, as one change of an edit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatementRows {
    /// The property's local name.
    pub property: String,
    /// Its rows, in order.
    pub rows: Vec<StatementRow>,
}

/// Whether the values of `property` are literals rather than resources.
#[must_use]
pub fn is_literal(property: &Property) -> bool {
    property.kind == PropertyKind::Datatype
}

/// Whether `property` relates two resources: `bf:relatedTo` or one of its
/// sub-properties, at any depth.
#[must_use]
pub fn is_relation(property: &Property) -> bool {
    let mut level = vec![property.name];
    // The hierarchy is a few levels deep; the bound stops a cycle in a broken file.
    for _ in 0..bibframe::properties().len() {
        if level
            .iter()
            .any(|name| matches!(*name, "relatedTo" | "relation"))
        {
            return true;
        }
        level = level
            .iter()
            .filter_map(|name| bibframe::property(name))
            .flat_map(|property| property.super_properties.iter().copied())
            .collect();
        if level.is_empty() {
            break;
        }
    }
    false
}

/// The row that shows `value`, carrying it.
#[must_use]
pub fn row_of(value: &Value) -> StatementRow {
    let mut row = cells_of(value);
    // A value serializes; an error would only drop what the row keeps.
    row.kept = serde_json::to_string(value).ok();
    row
}

fn cells_of(value: &Value) -> StatementRow {
    let mut row = StatementRow::default();
    match value {
        Value::Literal { text, language } => {
            row.text = Some(text.clone());
            row.language.clone_from(language);
        }
        Value::Concept { scheme, code } => row.concept = Some(format!("{scheme}/{code}")),
        Value::Iri(iri) => row.iri = Some(iri.clone()),
        Value::Resource(resource) => row.text.clone_from(&resource.label),
        Value::Entity(entity) => {
            row.text = Some(format!(
                "{} {}",
                entity.entity.bibframe().class_name(),
                entity.id
            ));
        }
    }
    row
}

/// The statements of `rows` for an entity of `kind`, checked as the store checks them.
///
/// # Errors
///
/// The property's field, when a row cannot be read or the statements are refused.
pub fn statements_of(
    kind: EntityKind,
    rows: &StatementRows,
) -> Result<(&'static str, Vec<Statement>), FieldError> {
    let property = bibframe::property(&rows.property)
        .filter(|property| property.applies_to(kind.bibframe()))
        .ok_or(FieldError {
            field: "statements",
            message_key: REFUSED,
        })?;
    let refused = FieldError {
        field: property.name,
        message_key: REFUSED,
    };
    let mut statements = Vec::new();
    for row in &rows.rows {
        if let Some(value) = value_of(kind, property, row).map_err(|()| refused)? {
            statements.push(Statement {
                property: property.name.to_owned(),
                value,
            });
        }
    }
    statement::check(kind, &statements).map_err(|_| refused)?;
    Ok((property.name, statements))
}

/// The value of one row: `None` for an empty row; an error for one that cannot be read.
fn value_of(
    kind: EntityKind,
    property: &Property,
    row: &StatementRow,
) -> Result<Option<Value>, ()> {
    let kept: Option<Value> = match row.kept.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(json) => Some(serde_json::from_str(json).map_err(drop)?),
    };
    let cells = trimmed(row);
    if let Some(kept) = kept
        .as_ref()
        .filter(|kept| trimmed(&cells_of(kept)) == cells)
    {
        return Ok(Some(kept.clone()));
    }
    if is_literal(property) {
        return Ok(cells.text.map(|text| Value::Literal {
            text,
            language: cells.language,
        }));
    }
    if let Some(concept) = cells.concept {
        let (scheme, code) = concept.split_once('/').ok_or(())?;
        return Ok(Some(Value::Concept {
            scheme: scheme.to_owned(),
            code: code.to_owned(),
        }));
    }
    if let Some(iri) = cells.iri {
        return Ok(Some(Value::Iri(iri)));
    }
    let Some(label) = cells.text else {
        return Ok(None);
    };
    // A label changed on a stored resource keeps its class and its own statements.
    let resource = match kept {
        Some(Value::Resource(resource)) => Resource {
            label: Some(label),
            ..resource
        },
        _ => Resource {
            class: new_class(kind, property).ok_or(())?.to_owned(),
            label: Some(label),
            statements: Vec::new(),
        },
    };
    Ok(Some(Value::Resource(resource)))
}

/// The cells of a row, trimmed, an empty one as `None`; without what it keeps.
fn trimmed(row: &StatementRow) -> StatementRow {
    let cell = |value: &Option<String>| {
        value
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    };
    StatementRow {
        text: cell(&row.text),
        language: cell(&row.language),
        concept: cell(&row.concept),
        iri: cell(&row.iri),
        kept: None,
    }
}

/// The class of a new resource of `property` named only by its label.
fn new_class(kind: EntityKind, property: &Property) -> Option<&'static str> {
    if let Some((_, class)) = NEW_CLASSES.iter().find(|(name, _)| *name == property.name) {
        return Some(class);
    }
    if let Some(range) = property
        .ranges
        .iter()
        .find_map(|range| bibframe::class(range))
    {
        return Some(range.name);
    }
    let mut capital = property.name.to_owned();
    if let Some(first) = capital.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    if let Some(class) = bibframe::class(&capital) {
        return Some(class.name);
    }
    // A related resource named only by its label is of the entity's own class.
    is_relation(property).then(|| kind.bibframe().class_name())
}

/// The concepts a row of `property` can choose, as `(scheme/code, label)`.
pub fn concept_choices(property: &str) -> impl Iterator<Item = (String, &'static str)> {
    vocabulary::schemes_of(property).flat_map(|scheme| {
        scheme
            .offered()
            .map(move |concept| (format!("{}/{}", scheme.name, concept.code), concept.label))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(property: &str, rows: Vec<StatementRow>) -> StatementRows {
        StatementRows {
            property: property.to_owned(),
            rows,
        }
    }

    fn text(value: &str) -> StatementRow {
        StatementRow {
            text: Some(value.to_owned()),
            ..StatementRow::default()
        }
    }

    #[test]
    fn rows_become_literals_concepts_iris_and_resources() {
        let (property, statements) = statements_of(
            EntityKind::Work,
            &rows("awards", vec![text(" Jabuti 1995 "), text("  ")]),
        )
        .unwrap();
        assert_eq!(property, "awards");
        assert_eq!(
            statements[..],
            [Statement {
                property: "awards".to_owned(),
                value: Value::Literal {
                    text: "Jabuti 1995".to_owned(),
                    language: None,
                },
            }]
        );
        let (_, statements) = statements_of(
            EntityKind::Work,
            &rows(
                "content",
                vec![StatementRow {
                    concept: Some("contentTypes/txt".to_owned()),
                    ..StatementRow::default()
                }],
            ),
        )
        .unwrap();
        assert!(matches!(&statements[0].value, Value::Concept { code, .. } if code == "txt"));
        let (_, statements) =
            statements_of(EntityKind::Work, &rows("genreForm", vec![text("Romances")])).unwrap();
        assert!(matches!(&statements[0].value,
            Value::Resource(resource) if resource.class == "GenreForm"));
        let (_, statements) =
            statements_of(EntityKind::Work, &rows("otherEdition", vec![text("Outra")])).unwrap();
        assert!(matches!(&statements[0].value,
            Value::Resource(resource) if resource.class == "Work"));
    }

    #[test]
    fn a_row_left_as_it_was_keeps_what_its_cells_do_not_show() {
        let stored = Value::Resource(Resource {
            class: "Dissertation".to_owned(),
            label: Some("Tese".to_owned()),
            statements: vec![Statement {
                property: "degree".to_owned(),
                value: Value::Literal {
                    text: "Doutorado".to_owned(),
                    language: None,
                },
            }],
        });
        let row = row_of(&stored);
        assert_eq!(row.text.as_deref(), Some("Tese"));
        let (_, statements) =
            statements_of(EntityKind::Work, &rows("dissertation", vec![row.clone()])).unwrap();
        assert_eq!(statements[0].value, stored);
        // A new label keeps the class and the statements under it.
        let renamed = StatementRow {
            text: Some("Tese de doutorado".to_owned()),
            ..row
        };
        let (_, statements) =
            statements_of(EntityKind::Work, &rows("dissertation", vec![renamed])).unwrap();
        let Value::Resource(resource) = &statements[0].value else {
            panic!("a resource")
        };
        assert_eq!(resource.statements.len(), 1);
        assert_eq!(resource.label.as_deref(), Some("Tese de doutorado"));
    }

    #[test]
    fn rows_the_store_would_refuse_are_refused_at_their_field() {
        let refused = |property: &str, row: StatementRow| {
            statements_of(EntityKind::Work, &rows(property, vec![row])).unwrap_err()
        };
        assert_eq!(
            refused(
                "content",
                StatementRow {
                    concept: Some("contentTypes/xyz".to_owned()),
                    ..StatementRow::default()
                }
            ),
            FieldError {
                field: "content",
                message_key: REFUSED
            }
        );
        // The preferred title has its own field; a title here is a variant one.
        assert!(statements_of(EntityKind::Work, &rows("title", vec![text("Outro")])).is_ok());
        // Not a property of a Work.
        assert_eq!(refused("dimensions", text("21 cm")).field, "statements");
        assert_eq!(
            refused(
                "content",
                StatementRow {
                    kept: Some("{".to_owned()),
                    ..StatementRow::default()
                }
            )
            .field,
            "content"
        );
    }

    #[test]
    fn relations_are_the_sub_properties_of_related_to() {
        for name in [
            "relatedTo",
            "absorbed",
            "seriesOf",
            "translationOf",
            "relation",
        ] {
            assert!(is_relation(bibframe::property(name).unwrap()), "{name}");
        }
        for name in ["subject", "awards", "genreForm"] {
            assert!(!is_relation(bibframe::property(name).unwrap()), "{name}");
        }
    }
}
