// SPDX-License-Identifier: GPL-3.0-or-later
//! The Library of Congress BIBFRAME ontology: every class and property, as published.
//!
//! The tables are generated from the pinned `reference/bibframe/bibframe.rdf`
//! (docs/specs/catalogo/bibframe-completo.md, items 1 to 4). Nothing here is Elzevir's own
//! vocabulary: names, labels and definitions are the ontology's.

use crate::generated::bibframe::{CLASSES, ONTOLOGY, PROPERTIES};

/// The namespace of every class and property.
pub const NAMESPACE: &str = "http://id.loc.gov/ontologies/bibframe/";

/// Which release of the ontology is pinned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ontology {
    /// `owl:versionInfo`, for example `3.0.1`.
    pub version: &'static str,
    /// `owl:versionIRI`.
    pub version_iri: &'static str,
    /// `dcterms:issued`.
    pub issued: &'static str,
    /// Where the pinned file was fetched from.
    pub source_url: &'static str,
    /// SHA-256 of the pinned file.
    pub source_sha256: &'static str,
}

/// One `owl:Class`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Class {
    /// Local name, for example `Work`.
    pub name: &'static str,
    /// `rdfs:label`.
    pub label: &'static str,
    /// `skos:definition`.
    pub definition: &'static str,
    /// `rdfs:subClassOf`: local names, or a full IRI for a class of another vocabulary.
    pub super_classes: &'static [&'static str],
}

/// What kind of property the ontology declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PropertyKind {
    /// `owl:ObjectProperty`: the value is a resource.
    Object,
    /// `owl:DatatypeProperty`: the value is a literal.
    Datatype,
    /// `owl:SymmetricProperty`: a relation that holds in both directions.
    Symmetric,
}

/// One property.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Property {
    /// Local name, for example `instanceOf`.
    pub name: &'static str,
    /// `rdfs:label`.
    pub label: &'static str,
    /// `skos:definition`.
    pub definition: &'static str,
    /// The kind declared.
    pub kind: PropertyKind,
    /// `rdfs:domain`, as local names. Empty for about half of the properties.
    pub domains: &'static [&'static str],
    /// `rdfs:range`: local names, or a full IRI (`rdfs:Literal`).
    pub ranges: &'static [&'static str],
    /// `rdfs:subPropertyOf`.
    pub super_properties: &'static [&'static str],
    /// `owl:inverseOf`.
    pub inverse_of: Option<&'static str>,
    /// The ontology's "Suggested use - With …" comment: the classes a property without a
    /// domain is used with.
    pub used_with: &'static [&'static str],
    /// The ontology's "Suggested value - …" comment.
    pub suggested_values: &'static [&'static str],
}

/// The four entities of the catalogue that carry description.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Entity {
    /// `bf:Hub`.
    Hub,
    /// `bf:Work`.
    Work,
    /// `bf:Instance`.
    Instance,
    /// `bf:Item`.
    Item,
}

impl Entity {
    /// Every entity, from the most abstract to the copy.
    pub const ALL: [Self; 4] = [Self::Hub, Self::Work, Self::Instance, Self::Item];

    /// The entity's class in the ontology.
    #[must_use]
    pub const fn class_name(self) -> &'static str {
        match self {
            Self::Hub => "Hub",
            Self::Work => "Work",
            Self::Instance => "Instance",
            Self::Item => "Item",
        }
    }
}

/// Where the properties that the ontology leaves without a domain and without a
/// "Suggested use" comment apply (bibframe-completo.md, item 4). An empty list means the
/// property only describes a nested resource (a title, a note, a provision activity).
///
/// Source: the Library of Congress MARC 21 to BIBFRAME conversion specifications.
const GENERIC_USE: &[(&str, &[Entity])] = &[
    ("adminMetadata", &Entity::ALL),
    ("agent", &[]),
    ("assigner", &[]),
    ("code", &[]),
    ("count", &[]),
    ("date", &[]),
    ("electronicLocator", &[Entity::Instance, Entity::Item]),
    ("enumerationAndChronology", &[Entity::Item]),
    ("extent", &[Entity::Instance]),
    ("firstIssue", &[Entity::Instance]),
    ("frequency", &[Entity::Instance]),
    ("geographicCoverage", &[Entity::Work]),
    ("identifiedBy", &Entity::ALL),
    ("immediateAcquisition", &[Entity::Item]),
    ("language", &[Entity::Hub, Entity::Work]),
    ("lastIssue", &[Entity::Instance]),
    ("note", &[Entity::Work, Entity::Instance, Entity::Item]),
    ("part", &[]),
    ("place", &[]),
    ("qualifier", &[]),
    ("relatedTo", &Entity::ALL),
    ("source", &[]),
    ("status", &[Entity::Instance, Entity::Item]),
    ("unit", &[]),
];

/// Properties a Hub carries although the ontology's "Suggested use" comments, written
/// before `bf:Hub` existed, do not name it.
///
/// Source: the Hub descriptions the Library of Congress publishes at
/// `id.loc.gov/resources/hubs`, which have a title and contributions.
const HUB_USE: &[&str] = &["contribution", "title"];

/// The pinned release.
#[must_use]
pub const fn ontology() -> Ontology {
    ONTOLOGY
}

/// Every class, sorted by name.
#[must_use]
pub fn classes() -> &'static [Class] {
    CLASSES
}

/// Every property, sorted by name.
#[must_use]
pub fn properties() -> &'static [Property] {
    PROPERTIES
}

/// The class called `name`.
#[must_use]
pub fn class(name: &str) -> Option<&'static Class> {
    let at = CLASSES
        .binary_search_by(|class| class.name.cmp(name))
        .ok()?;
    CLASSES.get(at)
}

/// The property called `name`.
#[must_use]
pub fn property(name: &str) -> Option<&'static Property> {
    let at = PROPERTIES
        .binary_search_by(|property| property.name.cmp(name))
        .ok()?;
    PROPERTIES.get(at)
}

/// Whether `name` is `ancestor` or one of its subclasses, at any depth.
#[must_use]
pub fn is_a(name: &str, ancestor: &str) -> bool {
    // The hierarchy is at most a few levels deep; the bound stops a cycle in a broken file.
    let mut level = vec![name];
    for _ in 0..CLASSES.len() {
        if level.contains(&ancestor) {
            return true;
        }
        level = level
            .iter()
            .filter_map(|name| class(name))
            .flat_map(|class| class.super_classes.iter().copied())
            .collect();
        if level.is_empty() {
            break;
        }
    }
    false
}

/// The direct and indirect subclasses of `ancestor`, sorted by name.
pub fn subclasses(ancestor: &str) -> impl Iterator<Item = &'static Class> {
    CLASSES
        .iter()
        .filter(move |class| class.name != ancestor && is_a(class.name, ancestor))
}

impl Property {
    /// Whether the property describes `entity` directly (bibframe-completo.md, item 4):
    /// its domain is the entity's class or a subclass of it; or, without a domain, the
    /// ontology suggests its use with that class (or [`HUB_USE`] does); or, without either, [`GENERIC_USE`] says so.
    #[must_use]
    pub fn applies_to(&self, entity: Entity) -> bool {
        let class = entity.class_name();
        if !self.domains.is_empty() {
            return self.domains.iter().any(|domain| is_a(domain, class));
        }
        if !self.used_with.is_empty() {
            return self.used_with.contains(&class)
                || (entity == Entity::Hub && HUB_USE.contains(&self.name));
        }
        GENERIC_USE
            .iter()
            .any(|(name, entities)| *name == self.name && entities.contains(&entity))
    }

    /// The property's IRI.
    #[must_use]
    pub fn iri(&self) -> String {
        format!("{NAMESPACE}{}", self.name)
    }
}

/// The properties that describe `entity` directly, sorted by name.
pub fn properties_of(entity: Entity) -> impl Iterator<Item = &'static Property> {
    PROPERTIES
        .iter()
        .filter(move |property| property.applies_to(entity))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names<'a>(classes: impl Iterator<Item = &'a Class>) -> Vec<&'a str> {
        classes.map(|class| class.name).collect()
    }

    #[test]
    fn the_pinned_release_is_3_0_1() {
        assert_eq!(ontology().version, "3.0.1");
        assert_eq!(ontology().source_sha256.len(), 64);
        assert_eq!(classes().len(), 224);
        assert_eq!(properties().len(), 231);
    }

    #[test]
    fn tables_are_sorted_and_every_reference_resolves() {
        assert!(classes().is_sorted_by(|a, b| a.name < b.name));
        assert!(properties().is_sorted_by(|a, b| a.name < b.name));
        let local = |name: &&str| !name.contains(':');
        for class in classes() {
            for parent in class.super_classes.iter().copied().filter(local) {
                assert!(super::class(parent).is_some(), "{}: {parent}", class.name);
            }
        }
        for property in properties() {
            let classes = property.domains.iter().chain(property.ranges);
            for name in classes.chain(property.used_with).copied().filter(local) {
                // The published file misspells this one range ("Qualifer") and declares
                // no such class; it is kept as published.
                if name == "MediumComponentQualifer" {
                    continue;
                }
                assert!(class(name).is_some(), "{}: {name}", property.name);
            }
            let related = property.super_properties.iter().chain(&property.inverse_of);
            for name in related {
                assert!(super::property(name).is_some(), "{}: {name}", property.name);
            }
        }
    }

    #[test]
    fn material_classes_are_the_ontology_s() {
        assert_eq!(
            names(subclasses("Work")),
            [
                "Arrangement",
                "Audio",
                "Cartography",
                "Collection",
                "Dataset",
                "Integrating",
                "Kit",
                "Manuscript",
                "MixedMaterial",
                "Monograph",
                "MovingImage",
                "Multimedia",
                "MusicAudio",
                "NonMusicAudio",
                "NotatedMovement",
                "NotatedMusic",
                "Object",
                "Serial",
                "Series",
                "StillImage",
                "Text"
            ]
        );
        assert_eq!(
            names(subclasses("Instance")),
            ["Archival", "Electronic", "Microform", "Print", "Tactile"]
        );
        assert_eq!(
            names(subclasses("Agent")),
            [
                "Family",
                "Jurisdiction",
                "Meeting",
                "Organization",
                "Person"
            ]
        );
        assert_eq!(subclasses("Identifier").count(), 52);
        assert!(is_a("ShelfMarkDdc", "Identifier"), "through ShelfMark");
        assert!(!is_a("Work", "Text"));
    }

    #[test]
    fn a_property_applies_by_domain_then_by_suggested_use() {
        let applies = |name: &str| {
            let property = property(name).unwrap();
            Entity::ALL.map(|entity| property.applies_to(entity))
        };
        // Hub, Work, Instance, Item.
        assert_eq!(applies("sublocation"), [false, false, false, true]);
        assert_eq!(applies("carrier"), [false, false, true, false]);
        assert_eq!(applies("content"), [false, true, false, false]);
        assert_eq!(applies("subject"), [false, true, true, true]);
        assert_eq!(applies("genreForm"), [false, true, true, true]);
        assert_eq!(applies("summary"), [false, true, true, false]);
        assert_eq!(applies("title"), [true, true, true, true]);
        assert_eq!(applies("usageAndAccessPolicy"), [false, true, true, true]);
        assert_eq!(applies("hasExpression"), [true, true, false, false]);
        assert_eq!(applies("identifiedBy"), [true, true, true, true]);
        assert_eq!(applies("role"), [false, false, false, false]);
    }

    #[test]
    fn the_generic_table_names_exactly_the_properties_the_ontology_leaves_open() {
        let open: Vec<_> = properties()
            .iter()
            .filter(|property| property.domains.is_empty() && property.used_with.is_empty())
            .map(|property| property.name)
            .collect();
        let listed: Vec<_> = GENERIC_USE.iter().map(|(name, _)| *name).collect();
        assert_eq!(open, listed);
    }

    #[test]
    fn every_entity_has_properties_and_iris_use_the_namespace() {
        for entity in Entity::ALL {
            assert!(class(entity.class_name()).is_some());
            assert!(properties_of(entity).count() > 5, "{entity:?}");
        }
        assert_eq!(
            property("heldBy").unwrap().iri(),
            "http://id.loc.gov/ontologies/bibframe/heldBy"
        );
    }
}
