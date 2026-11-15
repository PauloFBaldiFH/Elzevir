// SPDX-License-Identifier: GPL-3.0-or-later
//! BIBFRAME statements: the properties of Hubs, Works, Instances and Items that have no
//! column of their own (docs/specs/catalogo/bibframe-completo.md, items 10a to 10g).

use elzevir_data::bibframe::{self, Entity, PropertyKind};
use elzevir_data::vocabulary;
use serde::{Deserialize, Serialize};

/// Deepest nesting of resources (item 10d).
pub const MAX_DEPTH: usize = 4;
/// Most statements of one subject, nested ones included (item 10d).
pub const MAX_STATEMENTS: usize = 2_000;
/// Longest literal, in characters (item 10d).
pub const MAX_LITERAL: usize = 10_000;
/// Longest IRI, in characters (item 10d).
pub const MAX_IRI: usize = 2_000;

/// The properties stored in typed tables, per entity: the statements command refuses
/// them so nothing is stored twice (item 10a).
pub const TYPED: &[(Entity, &str)] = &[
    (Entity::Hub, "title"),
    (Entity::Hub, "hasExpression"),
    (Entity::Work, "title"),
    (Entity::Work, "expressionOf"),
    (Entity::Work, "hasInstance"),
    (Entity::Work, "contribution"),
    (Entity::Work, "subject"),
    (Entity::Work, "language"),
    (Entity::Work, "classification"),
    (Entity::Work, "summary"),
    (Entity::Work, "intendedAudience"),
    (Entity::Work, "translationOf"),
    (Entity::Work, "derivativeOf"),
    (Entity::Work, "partOf"),
    (Entity::Work, "supplementTo"),
    (Entity::Work, "continues"),
    (Entity::Instance, "instanceOf"),
    (Entity::Instance, "hasItem"),
    (Entity::Instance, "title"),
    (Entity::Instance, "editionStatement"),
    (Entity::Instance, "provisionActivity"),
    (Entity::Instance, "extent"),
    (Entity::Instance, "seriesStatement"),
    (Entity::Instance, "identifiedBy"),
    (Entity::Instance, "note"),
    (Entity::Item, "itemOf"),
    (Entity::Item, "identifiedBy"),
    (Entity::Item, "shelfMark"),
    (Entity::Item, "heldBy"),
    (Entity::Item, "sublocation"),
    (Entity::Item, "status"),
    (Entity::Item, "usageAndAccessPolicy"),
    (Entity::Item, "immediateAcquisition"),
];

/// Whether `property` of `entity` lives in a typed table.
#[must_use]
pub fn is_typed(entity: Entity, property: &str) -> bool {
    TYPED
        .iter()
        .any(|(typed, name)| *typed == entity && *name == property)
}

/// Which of the four described entities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum EntityKind {
    /// A Hub.
    Hub,
    /// A Work.
    Work,
    /// An Instance.
    Instance,
    /// An Item.
    Item,
}

impl EntityKind {
    /// Every kind, from the most abstract to the copy.
    pub const ALL: [Self; 4] = [Self::Hub, Self::Work, Self::Instance, Self::Item];

    /// The entity in the ontology.
    #[must_use]
    pub const fn bibframe(self) -> Entity {
        match self {
            Self::Hub => Entity::Hub,
            Self::Work => Entity::Work,
            Self::Instance => Entity::Instance,
            Self::Item => Entity::Item,
        }
    }
}

/// A Hub, Work, Instance or Item of the catalogue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EntityRef {
    /// Which entity.
    pub entity: EntityKind,
    /// Its row id.
    pub id: i64,
}

/// One property and its value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Statement {
    /// The property's local name in the ontology, such as `genreForm`.
    pub property: String,
    /// Its value.
    pub value: Value,
}

/// The value of a statement (item 10b).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Value {
    /// Text, for a datatype property.
    Literal {
        /// The text.
        text: String,
        /// Its language tag, such as `por`.
        language: Option<String>,
    },
    /// A concept of a Library of Congress vocabulary.
    Concept {
        /// The vocabulary, such as `contentTypes`.
        scheme: String,
        /// The concept's code, such as `txt`.
        code: String,
    },
    /// Another entity of the catalogue.
    Entity(EntityRef),
    /// An outside resource, such as an `id.loc.gov` authority.
    Iri(String),
    /// A resource described in place.
    Resource(Resource),
}

/// A resource described in place: its class, an optional label and its own statements.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resource {
    /// The class's local name, such as `Publication`.
    pub class: String,
    /// `rdfs:label`, when the resource is named rather than structured.
    pub label: Option<String>,
    /// Its statements.
    pub statements: Vec<Statement>,
}

/// Why a list of statements cannot be stored.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
pub enum StatementRefusal {
    /// The ontology has no such property.
    #[error("`{0}` is not a BIBFRAME property")]
    UnknownProperty(String),
    /// The property does not describe this entity.
    #[error("`{property}` does not describe a {entity:?}")]
    NotApplicable {
        /// The property.
        property: String,
        /// The entity.
        entity: EntityKind,
    },
    /// The property has its own field; it is not a statement.
    #[error("`{0}` is edited in its own field")]
    Typed(String),
    /// A literal for an object property, or a resource for a datatype property.
    #[error("`{0}` does not take this kind of value")]
    WrongKind(String),
    /// The concept is not in a vocabulary of the property.
    #[error("`{scheme}/{code}` is not a value of `{property}`")]
    UnknownConcept {
        /// The property.
        property: String,
        /// The vocabulary given.
        scheme: String,
        /// The code given.
        code: String,
    },
    /// The ontology has no such class.
    #[error("`{0}` is not a BIBFRAME class")]
    UnknownClass(String),
    /// A text, a label or an IRI is empty or too long.
    #[error("a value of `{0}` is empty or too long")]
    BadText(String),
    /// Too many statements or too deep (item 10d).
    #[error("too many statements or nested too deep")]
    TooLarge,
}

/// Checks the statements of `subject` before they are stored (item 10c).
///
/// # Errors
///
/// The first statement that breaks a rule.
pub fn check(subject: EntityKind, statements: &[Statement]) -> Result<(), StatementRefusal> {
    let mut count = 0;
    for statement in statements {
        if is_typed(subject.bibframe(), &statement.property) {
            return Err(StatementRefusal::Typed(statement.property.clone()));
        }
        let property = known(&statement.property)?;
        if !property.applies_to(subject.bibframe()) {
            return Err(StatementRefusal::NotApplicable {
                property: statement.property.clone(),
                entity: subject,
            });
        }
    }
    check_level(statements, 1, &mut count)
}

fn known(name: &str) -> Result<&'static bibframe::Property, StatementRefusal> {
    bibframe::property(name).ok_or_else(|| StatementRefusal::UnknownProperty(name.to_owned()))
}

fn check_level(
    statements: &[Statement],
    depth: usize,
    count: &mut usize,
) -> Result<(), StatementRefusal> {
    if depth > MAX_DEPTH {
        return Err(StatementRefusal::TooLarge);
    }
    for statement in statements {
        *count += 1;
        if *count > MAX_STATEMENTS {
            return Err(StatementRefusal::TooLarge);
        }
        let property = known(&statement.property)?;
        let name = || statement.property.clone();
        let literal = property.kind == PropertyKind::Datatype;
        match &statement.value {
            Value::Literal { text, language } => {
                if !literal {
                    return Err(StatementRefusal::WrongKind(name()));
                }
                text_fits(text, MAX_LITERAL, &statement.property)?;
                if let Some(language) = language {
                    text_fits(language, 64, &statement.property)?;
                }
            }
            _ if literal => return Err(StatementRefusal::WrongKind(name())),
            Value::Concept { scheme, code } => {
                let found = vocabulary::schemes_of(&statement.property)
                    .find(|known| known.name == scheme)
                    .and_then(|known| known.concept(code));
                if found.is_none() {
                    return Err(StatementRefusal::UnknownConcept {
                        property: name(),
                        scheme: scheme.clone(),
                        code: code.clone(),
                    });
                }
            }
            Value::Entity(_) => {}
            Value::Iri(iri) => {
                text_fits(iri, MAX_IRI, &statement.property)?;
                if !(iri.starts_with("http://") || iri.starts_with("https://")) {
                    return Err(StatementRefusal::BadText(name()));
                }
            }
            Value::Resource(resource) => {
                if bibframe::class(&resource.class).is_none() {
                    return Err(StatementRefusal::UnknownClass(resource.class.clone()));
                }
                if let Some(label) = &resource.label {
                    text_fits(label, MAX_LITERAL, &statement.property)?;
                }
                check_level(&resource.statements, depth + 1, count)?;
            }
        }
    }
    Ok(())
}

fn text_fits(text: &str, most: usize, property: &str) -> Result<(), StatementRefusal> {
    if text.trim().is_empty() || text.chars().count() > most {
        return Err(StatementRefusal::BadText(property.to_owned()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn statement(property: &str, value: Value) -> Statement {
        Statement {
            property: property.to_owned(),
            value,
        }
    }

    fn concept(scheme: &str, code: &str) -> Value {
        Value::Concept {
            scheme: scheme.to_owned(),
            code: code.to_owned(),
        }
    }

    fn literal(text: &str) -> Value {
        Value::Literal {
            text: text.to_owned(),
            language: None,
        }
    }

    fn resource(class: &str, statements: Vec<Statement>) -> Value {
        Value::Resource(Resource {
            class: class.to_owned(),
            label: None,
            statements,
        })
    }

    #[test]
    fn official_values_are_accepted() {
        let work = [
            statement("content", concept("contentTypes", "txt")),
            statement("originDate", literal("1899")),
            statement(
                "genreForm",
                Value::Iri("http://id.loc.gov/authorities/genreForms/gf2014026339".to_owned()),
            ),
        ];
        assert_eq!(check(EntityKind::Work, &work), Ok(()));
        let instance = [
            statement("carrier", concept("carriers", "nc")),
            statement("media", concept("mediaTypes", "n")),
            statement("copyrightDate", resource("Publication", vec![])),
        ];
        assert_eq!(
            check(EntityKind::Instance, &instance),
            Err(StatementRefusal::WrongKind("copyrightDate".to_owned())),
            "a datatype property takes a literal"
        );
        let publication = resource(
            "Publication",
            vec![
                statement("place", concept("countries", "bl")),
                statement("date", literal("2020")),
            ],
        );
        assert_eq!(
            check(EntityKind::Instance, &[statement("hasSeries", publication)]),
            Ok(())
        );
    }

    #[test]
    fn invented_codes_and_wrong_lists_are_refused() {
        assert!(matches!(
            check(
                EntityKind::Work,
                &[statement("content", concept("contentTypes", "BK"))]
            ),
            Err(StatementRefusal::UnknownConcept { .. })
        ));
        assert!(matches!(
            check(
                EntityKind::Instance,
                &[statement("carrier", concept("contentTypes", "txt"))]
            ),
            Err(StatementRefusal::UnknownConcept { .. })
        ));
        assert_eq!(
            check(EntityKind::Work, &[statement("content", literal("text"))]),
            Err(StatementRefusal::WrongKind("content".to_owned()))
        );
    }

    #[test]
    fn properties_of_other_entities_and_typed_ones_are_refused() {
        assert_eq!(
            check(
                EntityKind::Work,
                &[statement("carrier", concept("carriers", "nc"))]
            ),
            Err(StatementRefusal::NotApplicable {
                property: "carrier".to_owned(),
                entity: EntityKind::Work
            })
        );
        assert_eq!(
            check(
                EntityKind::Instance,
                &[statement("title", resource("Title", vec![]))]
            ),
            Err(StatementRefusal::Typed("title".to_owned()))
        );
        assert_eq!(
            check(EntityKind::Item, &[statement("colour", literal("x"))]),
            Err(StatementRefusal::UnknownProperty("colour".to_owned()))
        );
        assert_eq!(
            check(
                EntityKind::Work,
                &[statement("hasPart", resource("Book", vec![]))]
            ),
            Err(StatementRefusal::UnknownClass("Book".to_owned()))
        );
    }

    #[test]
    fn limits_are_enforced() {
        let mut deep = resource("Work", vec![]);
        for _ in 0..MAX_DEPTH {
            deep = resource("Work", vec![statement("hasPart", deep)]);
        }
        assert_eq!(
            check(EntityKind::Work, &[statement("hasPart", deep)]),
            Err(StatementRefusal::TooLarge)
        );
        let many = vec![statement("originDate", literal("1")); MAX_STATEMENTS + 1];
        assert_eq!(
            check(EntityKind::Work, &many),
            Err(StatementRefusal::TooLarge)
        );
        let long = "x".repeat(MAX_LITERAL + 1);
        assert_eq!(
            check(EntityKind::Work, &[statement("originDate", literal(&long))]),
            Err(StatementRefusal::BadText("originDate".to_owned()))
        );
        assert_eq!(
            check(
                EntityKind::Work,
                &[statement("hasPart", Value::Iri("urn:x".to_owned()))]
            ),
            Err(StatementRefusal::BadText("hasPart".to_owned()))
        );
    }

    #[test]
    fn every_typed_property_describes_its_entity() {
        for (entity, name) in TYPED {
            let property = bibframe::property(name).unwrap();
            assert!(property.applies_to(*entity), "{name} on {entity:?}");
        }
    }
}
