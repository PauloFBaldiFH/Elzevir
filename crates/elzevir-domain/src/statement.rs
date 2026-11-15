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

/// What BIBFRAME says of one entity beyond its typed columns (items 8 and 10h): the
/// subclasses it belongs to and its statements.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Description {
    /// Local names of subclasses of the entity's class, such as `Text` and `Monograph`.
    pub classes: Vec<String>,
    /// Its statements.
    pub statements: Vec<Statement>,
}

/// Most classes one entity carries.
pub const MAX_CLASSES: usize = 16;

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
    /// The class is not a subclass of the entity's class (item 10h).
    #[error("`{class}` is not a kind of {entity:?}")]
    NotASubclass {
        /// The class given.
        class: String,
        /// The entity.
        entity: EntityKind,
    },
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
        if is_typed(subject.bibframe(), &statement.property)
            && !is_other_title(statement)
            && !is_typed_note(statement)
            && !is_other_provision(statement)
            && !is_language_part(statement)
            && !is_coded_publication(statement)
            && !is_described_classification(statement)
            && !is_coded_audience(statement)
            && !is_sourced_number(statement)
        {
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
    // The subject's own statements are level 0: only nested resources count.
    check_level(statements, 0, &mut count)
}

/// Whether `statement` is a title other than the preferred one: a `bf:VariantTitle` or one
/// of its subclasses (bibframe-completo.md, item 29, part 5a). The preferred title has its
/// own field; every other title of a Work or an Instance is a statement.
#[must_use]
pub fn is_other_title(statement: &Statement) -> bool {
    statement.property == "title"
        && matches!(&statement.value, Value::Resource(resource)
            if bibframe::is_a(&resource.class, "VariantTitle"))
}

/// Whether `statement` is a note with a `bf:noteType` (bibframe-completo.md, item 29,
/// part 5b). General, contents and local notes have their own field; a note of a type
/// from the `mnotetype` list is a statement.
#[must_use]
pub fn is_typed_note(statement: &Statement) -> bool {
    statement.property == "note"
        && matches!(&statement.value, Value::Resource(note)
            if note.class == "Note" && note.statements.iter().any(|s| s.property == "noteType"))
}

/// Whether `statement` is a provision activity other than the publication: production,
/// distribution or manufacture (bibframe-completo.md, item 29, part 5c). The publication
/// has its own fields.
#[must_use]
pub fn is_other_provision(statement: &Statement) -> bool {
    statement.property == "provisionActivity"
        && matches!(&statement.value, Value::Resource(activity)
            if matches!(activity.class.as_str(), "Production" | "Distribution" | "Manufacture"))
}

/// Whether `statement` is a classification described as a statement (bibframe-completo.md,
/// item 29, part 5f): every classification but the Work's main number, which has its own
/// field, and what the main one says beyond its number (edition, source, item number,
/// assigner).
#[must_use]
pub fn is_described_classification(statement: &Statement) -> bool {
    statement.property == "classification"
        && matches!(&statement.value, Value::Resource(classification)
            if bibframe::is_a(&classification.class, "Classification"))
}

/// Whether `statement` is a coded target audience, a concept of the MARC list
/// (bibframe-completo.md, item 29, part 5f), as `008/22` gives it. The audience as written
/// has its own field.
#[must_use]
pub fn is_coded_audience(statement: &Statement) -> bool {
    statement.property == "intendedAudience" && matches!(&statement.value, Value::Concept { .. })
}

/// Whether `statement` is a number that an agency named by its source gives, such as the
/// number of a national bibliographic agency (bibframe-completo.md, item 29, part 5f): a
/// `bf:Local` with a `bf:source`. The standard numbers have their own field.
#[must_use]
pub fn is_sourced_number(statement: &Statement) -> bool {
    statement.property == "identifiedBy"
        && matches!(&statement.value, Value::Resource(local)
            if local.class == "Local" && local.statements.iter().any(|s| s.property == "source"))
}

/// Whether `statement` is the coded publication (bibframe-completo.md, item 29, part 5e): a
/// `bf:Publication` that holds only a date, a status and countries, as `008` codes them. The
/// publication as transcribed has its own fields.
#[must_use]
pub fn is_coded_publication(statement: &Statement) -> bool {
    statement.property == "provisionActivity"
        && matches!(&statement.value, Value::Resource(publication)
        if publication.class == "Publication"
            && !publication.statements.is_empty()
            && publication.statements.iter().all(|s| match (&*s.property, &s.value) {
                ("date", Value::Literal { .. }) | ("status" | "place", Value::Concept { .. }) => {
                    true
                }
                ("place", Value::Resource(place)) => {
                    place.label.is_none() && place.statements.iter().any(|c| c.property == "code")
                }
                _ => false,
            }))
}

/// Whether `statement` is the language of a part of the content, such as the original or
/// the summary (bibframe-completo.md, item 29, part 5e). The languages of the content itself
/// have their own field.
#[must_use]
pub fn is_language_part(statement: &Statement) -> bool {
    statement.property == "language"
        && matches!(&statement.value, Value::Resource(language)
            if language.class == "Language" && language.statements.iter().any(|s| s.property == "part"))
}

/// Checks a whole description: its classes, then its statements (item 10h).
///
/// # Errors
///
/// The first class or statement that breaks a rule.
pub fn check_description(
    subject: EntityKind,
    description: &Description,
) -> Result<(), StatementRefusal> {
    if description.classes.len() > MAX_CLASSES {
        return Err(StatementRefusal::TooLarge);
    }
    let parent = subject.bibframe().class_name();
    for class in &description.classes {
        if bibframe::class(class).is_none() {
            return Err(StatementRefusal::UnknownClass(class.clone()));
        }
        if class == parent || !bibframe::is_a(class, parent) {
            return Err(StatementRefusal::NotASubclass {
                class: class.clone(),
                entity: subject,
            });
        }
    }
    check(subject, &description.statements)
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
    fn a_number_with_its_agency_is_a_statement_and_a_standard_one_is_not() {
        let national = [statement(
            "identifiedBy",
            resource(
                "Local",
                vec![statement(
                    "source",
                    resource("Source", vec![statement("code", literal("BR-RjBN"))]),
                )],
            ),
        )];
        assert_eq!(check(EntityKind::Instance, &national), Ok(()));
        let isbn = [statement("identifiedBy", resource("Isbn", vec![]))];
        assert_eq!(
            check(EntityKind::Instance, &isbn),
            Err(StatementRefusal::Typed("identifiedBy".to_owned()))
        );
    }

    #[test]
    fn a_coded_audience_is_a_statement_and_a_written_one_is_not() {
        let coded = [statement("intendedAudience", concept("maudience", "juv"))];
        assert_eq!(check(EntityKind::Work, &coded), Ok(()));
        let written = [statement("intendedAudience", literal("Infantil"))];
        assert_eq!(
            check(EntityKind::Work, &written),
            Err(StatementRefusal::Typed("intendedAudience".to_owned()))
        );
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
                &[statement("content", concept("contentTypes", "book"))]
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
        // Every other title is a statement.
        for class in [
            "VariantTitle",
            "ParallelTitle",
            "AbbreviatedTitle",
            "CollectiveTitle",
        ] {
            let title = resource(class, vec![statement("mainTitle", literal("Capitu"))]);
            assert_eq!(
                check(EntityKind::Work, &[statement("title", title)]),
                Ok(())
            );
        }
        // Production, distribution and manufacture are statements; the publication is not.
        assert_eq!(
            check(
                EntityKind::Instance,
                &[statement(
                    "provisionActivity",
                    resource("Manufacture", vec![])
                )]
            ),
            Ok(())
        );
        assert_eq!(
            check(
                EntityKind::Instance,
                &[statement(
                    "provisionActivity",
                    resource("Publication", vec![])
                )]
            ),
            Err(StatementRefusal::Typed("provisionActivity".to_owned()))
        );
        // A note with a type is a statement; a plain one has its own field.
        let typed = resource("Note", vec![statement("noteType", literal("bibliography"))]);
        assert_eq!(
            check(EntityKind::Instance, &[statement("note", typed)]),
            Ok(())
        );
        assert_eq!(
            check(
                EntityKind::Instance,
                &[statement("note", resource("Note", vec![]))]
            ),
            Err(StatementRefusal::Typed("note".to_owned()))
        );
        // The language of a part is a statement; the language of the content is not.
        let part = resource(
            "Language",
            vec![
                statement("code", literal("fre")),
                statement("part", literal("original")),
            ],
        );
        assert_eq!(
            check(EntityKind::Work, &[statement("language", part)]),
            Ok(())
        );
        assert_eq!(
            check(
                EntityKind::Work,
                &[statement("language", resource("Language", vec![]))]
            ),
            Err(StatementRefusal::Typed("language".to_owned()))
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
    fn the_coded_publication_is_a_statement() {
        // The coded publication is a statement; one with an agent is the transcribed one.
        let coded = resource("Publication", vec![statement("date", literal("1995"))]);
        assert_eq!(
            check(
                EntityKind::Instance,
                &[statement("provisionActivity", coded)]
            ),
            Ok(())
        );
        let transcribed = resource(
            "Publication",
            vec![statement("agent", resource("Agent", vec![]))],
        );
        assert_eq!(
            check(
                EntityKind::Instance,
                &[statement("provisionActivity", transcribed)]
            ),
            Err(StatementRefusal::Typed("provisionActivity".to_owned()))
        );
        // A classification is a statement; the main number keeps its field on the Work.
        let udc = resource(
            "ClassificationUdc",
            vec![statement("classificationPortion", literal("316.7"))],
        );
        assert_eq!(
            check(EntityKind::Work, &[statement("classification", udc)]),
            Ok(())
        );
        assert_eq!(
            check(
                EntityKind::Work,
                &[statement("classification", resource("Work", vec![]))]
            ),
            Err(StatementRefusal::Typed("classification".to_owned()))
        );
    }

    #[test]
    fn limits_are_enforced() {
        let mut deep = resource("Work", vec![]);
        for _ in 1..MAX_DEPTH {
            deep = resource("Work", vec![statement("hasPart", deep)]);
        }
        // MAX_DEPTH nested resources are allowed; one more is refused.
        assert_eq!(
            check(EntityKind::Work, &[statement("hasPart", deep.clone())]),
            Ok(())
        );
        deep = resource("Work", vec![statement("hasPart", deep)]);
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

    #[test]
    fn classes_must_be_subclasses_of_the_entity() {
        let described = |classes: &[&str]| Description {
            classes: classes.iter().map(|class| (*class).to_owned()).collect(),
            statements: Vec::new(),
        };
        assert_eq!(
            check_description(
                EntityKind::Work,
                &described(&["Text", "Monograph", "MusicAudio"])
            ),
            Ok(())
        );
        assert_eq!(
            check_description(EntityKind::Instance, &described(&["Electronic"])),
            Ok(())
        );
        assert_eq!(
            check_description(EntityKind::Work, &described(&["Print"])),
            Err(StatementRefusal::NotASubclass {
                class: "Print".to_owned(),
                entity: EntityKind::Work
            })
        );
        assert_eq!(
            check_description(EntityKind::Work, &described(&["Work"])),
            Err(StatementRefusal::NotASubclass {
                class: "Work".to_owned(),
                entity: EntityKind::Work
            })
        );
        assert_eq!(
            check_description(EntityKind::Work, &described(&["Livro"])),
            Err(StatementRefusal::UnknownClass("Livro".to_owned()))
        );
    }
}
