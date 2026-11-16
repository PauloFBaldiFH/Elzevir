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
use elzevir_domain::statement::{self, EntityKind, EntityRef, Resource, Statement, Value};
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
    /// A resource's own statements, one per line, as [`details_of`] writes them.
    pub details: Option<String>,
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
        Value::Resource(resource) => {
            row.text.clone_from(&resource.label);
            row.details = Some(details_of(&resource.statements)).filter(|d| !d.is_empty());
        }
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
    if cells.text.is_none() && cells.details.is_none() {
        return Ok(None);
    }
    // A stored resource keeps its class, and its own statements while their lines stay.
    let (class, statements) = match kept {
        Some(Value::Resource(resource))
            if trimmed(&cells_of(&Value::Resource(resource.clone()))).details == cells.details =>
        {
            (resource.class, resource.statements)
        }
        Some(Value::Resource(resource)) => (
            resource.class,
            parse_details(cells.details.as_deref().unwrap_or_default())?,
        ),
        _ => (
            new_class(kind, property).ok_or(())?.to_owned(),
            parse_details(cells.details.as_deref().unwrap_or_default())?,
        ),
    };
    Ok(Some(Value::Resource(Resource {
        class,
        label: cells.text,
        statements,
    })))
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
    // The lines of the details without trailing blanks or empty lines; the indentation
    // that nests them stays.
    let details = row.details.as_deref().map(|text| {
        text.lines()
            .map(str::trim_end)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join("\n")
    });
    StatementRow {
        text: cell(&row.text),
        language: cell(&row.language),
        concept: cell(&row.concept),
        iri: cell(&row.iri),
        details: details.filter(|text| !text.is_empty()),
        kept: None,
    }
}

/// The statements of a resource as lines of text (bibframe-completo.md, item 26): one
/// `property: value` a line, the statements of a nested resource two spaces further in.
/// A concept is `=scheme/code`, an IRI `<…>`, an entity of the catalogue `#Class id`, a
/// resource its label and `{Class}`, a literal its text and, with a language, ` @tag`.
#[must_use]
pub fn details_of(statements: &[Statement]) -> String {
    let mut lines = Vec::new();
    write_details(statements, 0, &mut lines);
    lines.join("\n")
}

fn write_details(statements: &[Statement], depth: usize, lines: &mut Vec<String>) {
    for statement in statements {
        let indent = "  ".repeat(depth);
        let value = match &statement.value {
            Value::Literal { text, language } => match language {
                Some(language) => format!("{text} @{language}"),
                None => text.clone(),
            },
            Value::Concept { scheme, code } => format!("={scheme}/{code}"),
            Value::Iri(iri) => format!("<{iri}>"),
            Value::Entity(entity) => {
                format!("#{} {}", entity.entity.bibframe().class_name(), entity.id)
            }
            Value::Resource(resource) => match &resource.label {
                Some(label) => format!("{label} {{{}}}", resource.class),
                None => format!("{{{}}}", resource.class),
            },
        };
        lines.push(format!("{indent}{}: {value}", statement.property));
        if let Value::Resource(resource) = &statement.value {
            write_details(&resource.statements, depth + 1, lines);
        }
    }
}

/// The statements written by [`details_of`]; an error for a line it cannot read.
fn parse_details(text: &str) -> Result<Vec<Statement>, ()> {
    // The statements read so far at each open level, the outermost first.
    let mut levels: Vec<Vec<Statement>> = vec![Vec::new()];
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let spaces = line.len() - line.trim_start_matches(' ').len();
        let depth = spaces / 2;
        if spaces % 2 != 0 || depth >= levels.len() {
            return Err(());
        }
        // Closing the deeper levels hands their statements to the resource above them.
        while levels.len() > depth + 1 {
            let children = levels.pop().ok_or(())?;
            attach(levels.last_mut().ok_or(())?, children)?;
        }
        let statement = parse_line(line.trim())?;
        let opens = matches!(statement.value, Value::Resource(_));
        levels.last_mut().ok_or(())?.push(statement);
        if opens {
            levels.push(Vec::new());
        }
    }
    while levels.len() > 1 {
        let children = levels.pop().ok_or(())?;
        attach(levels.last_mut().ok_or(())?, children)?;
    }
    levels.pop().ok_or(())
}

/// Gives `children` to the last statement of `level`, which must be a resource.
fn attach(level: &mut [Statement], children: Vec<Statement>) -> Result<(), ()> {
    if children.is_empty() {
        return Ok(());
    }
    match level.last_mut().map(|statement| &mut statement.value) {
        Some(Value::Resource(resource)) => {
            resource.statements = children;
            Ok(())
        }
        _ => Err(()),
    }
}

fn parse_line(line: &str) -> Result<Statement, ()> {
    let (name, value) = line.split_once(':').ok_or(())?;
    let property = bibframe::property(name.trim()).ok_or(())?;
    let value = value.trim();
    let value = if let Some(concept) = value.strip_prefix('=') {
        let (scheme, code) = concept.split_once('/').ok_or(())?;
        Value::Concept {
            scheme: scheme.to_owned(),
            code: code.to_owned(),
        }
    } else if let Some(iri) = value.strip_prefix('<').and_then(|v| v.strip_suffix('>')) {
        Value::Iri(iri.to_owned())
    } else if let Some(entity) = value.strip_prefix('#') {
        let (class, id) = entity.split_once(' ').ok_or(())?;
        let entity = EntityKind::ALL
            .into_iter()
            .find(|kind| kind.bibframe().class_name() == class)
            .ok_or(())?;
        Value::Entity(EntityRef {
            entity,
            id: id.trim().parse().map_err(drop)?,
        })
    } else if is_literal(property) {
        let (text, language) = match value.rsplit_once(" @") {
            Some((text, tag))
                if (2..=8).contains(&tag.len())
                    && tag.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') =>
            {
                (text, Some(tag.to_owned()))
            }
            _ => (value, None),
        };
        Value::Literal {
            text: text.to_owned(),
            language,
        }
    } else {
        let (label, class) = match value.strip_suffix('}').and_then(|v| v.rsplit_once('{')) {
            Some((label, class)) => (label.trim(), class.trim().to_owned()),
            None => (value, nested_class(property).ok_or(())?.to_owned()),
        };
        Value::Resource(Resource {
            class,
            label: Some(label.to_owned()).filter(|label| !label.is_empty()),
            statements: Vec::new(),
        })
    };
    Ok(Statement {
        property: property.name.to_owned(),
        value,
    })
}

/// The class of a nested resource written without one: as for a new resource, without
/// the entity a relation would take.
fn nested_class(property: &Property) -> Option<&'static str> {
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
    bibframe::class(&capital).map(|class| class.name)
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
    fn the_statements_of_a_resource_are_lines_that_come_back_the_same() {
        let place = Statement {
            property: "place".to_owned(),
            value: Value::Resource(Resource {
                class: "Place".to_owned(),
                label: Some("Rio de Janeiro".to_owned()),
                statements: vec![Statement {
                    property: "code".to_owned(),
                    value: Value::Literal {
                        text: "bl".to_owned(),
                        language: None,
                    },
                }],
            }),
        };
        let statements = vec![
            place,
            Statement {
                property: "agent".to_owned(),
                value: Value::Resource(Resource {
                    class: "Organization".to_owned(),
                    label: Some("Garnier".to_owned()),
                    statements: Vec::new(),
                }),
            },
            Statement {
                property: "date".to_owned(),
                value: Value::Literal {
                    text: "1899".to_owned(),
                    language: Some("por".to_owned()),
                },
            },
            Statement {
                property: "status".to_owned(),
                value: Value::Concept {
                    scheme: "mstatus".to_owned(),
                    code: "current".to_owned(),
                },
            },
        ];
        let text = details_of(&statements);
        assert_eq!(
            text,
            "place: Rio de Janeiro {Place}\n  code: bl\nagent: Garnier {Organization}\ndate: 1899 @por\nstatus: =mstatus/current"
        );
        assert_eq!(parse_details(&text).unwrap(), statements);
        // A line without a class takes the one the property gives.
        let typed = parse_details("agent: Garnier\nplace: Paris").unwrap();
        assert!(matches!(&typed[1].value, Value::Resource(r) if r.class == "Place"));
        assert!(parse_details("nonsense line").is_err());
        assert!(parse_details("    date: 1899").is_err());
    }

    #[test]
    fn a_nested_group_is_edited_by_its_lines() {
        let publication = Value::Resource(Resource {
            class: "Production".to_owned(),
            label: None,
            statements: parse_details("place: Rio de Janeiro {Place}\ndate: 1899").unwrap(),
        });
        let mut row = row_of(&publication);
        assert_eq!(
            row.details.as_deref(),
            Some("place: Rio de Janeiro {Place}\ndate: 1899")
        );
        row.details = Some("place: Rio de Janeiro {Place}\ndate: 1900\n".to_owned());
        let (_, statements) =
            statements_of(EntityKind::Instance, &rows("provisionActivity", vec![row])).unwrap();
        let Value::Resource(resource) = &statements[0].value else {
            panic!("a resource")
        };
        assert_eq!(resource.class, "Production");
        assert_eq!(
            details_of(&resource.statements),
            "place: Rio de Janeiro {Place}\ndate: 1900"
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
