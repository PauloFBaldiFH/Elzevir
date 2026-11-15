// SPDX-License-Identifier: GPL-3.0-or-later
//! The Library of Congress controlled vocabularies that BIBFRAME values come from
//! (docs/specs/catalogo/bibframe-completo.md, items 11 to 13).
//!
//! The concepts are generated from the pinned bulk downloads of `id.loc.gov`; codes,
//! IRIs and English labels are the Library of Congress's. Which property takes which
//! vocabulary is the one table written by hand here, [`PROPERTY_SCHEMES`].

use crate::generated::vocabularies::{SCHEMES, SOURCES};

/// One downloaded file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VocabularySource {
    /// Path in the repository.
    pub file: &'static str,
    /// Where it was downloaded from.
    pub url: &'static str,
    /// SHA-256 of the file.
    pub sha256: &'static str,
}

/// One vocabulary (`skos:ConceptScheme`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scheme {
    /// Name in its IRI, for example `contentTypes`.
    pub name: &'static str,
    /// The scheme's IRI.
    pub iri: &'static str,
    /// Its concepts, sorted by code.
    pub concepts: &'static [Concept],
}

/// One concept of a vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Concept {
    /// The last segment of its IRI, for example `txt`.
    pub code: &'static str,
    /// `skos:prefLabel` in English.
    pub label: &'static str,
    /// The Library of Congress deprecated it: it is read, never offered.
    pub deprecated: bool,
}

impl Scheme {
    /// The concept with `code`.
    #[must_use]
    pub fn concept(&self, code: &str) -> Option<&'static Concept> {
        let at = self
            .concepts
            .binary_search_by(|concept| concept.code.cmp(code))
            .ok()?;
        self.concepts.get(at)
    }

    /// The concepts a cataloguer can choose: all but the deprecated ones.
    pub fn offered(&self) -> impl Iterator<Item = &'static Concept> {
        self.concepts.iter().filter(|concept| !concept.deprecated)
    }

    /// The IRI of the concept with `code`.
    #[must_use]
    pub fn concept_iri(&self, code: &str) -> String {
        format!("{}/{code}", self.iri)
    }
}

/// Which vocabularies give the values of a property (item 11). Most properties take one;
/// the characteristics of sound, video, projection and digital files, the materials and
/// the notations are classes with subclasses, one vocabulary each.
///
/// Source: the Library of Congress MARC 21 to BIBFRAME conversion (`marc2bibframe2`),
/// which writes values from these vocabularies for these properties; each concept's
/// `historyNote` names the MARC position it comes from.
const PROPERTY_SCHEMES: &[(&str, &[&str])] = &[
    ("appliedMaterial", &["mmaterial"]),
    ("aspectRatio", &["maspect"]),
    ("baseMaterial", &["mmaterial"]),
    ("bookFormat", &["bookformat"]),
    ("carrier", &["carriers"]),
    ("colorContent", &["mcolor"]),
    ("content", &["contentTypes"]),
    ("descriptionConventions", &["descriptionConventions"]),
    (
        "digitalCharacteristic",
        &["mencformat", "mfiletype", "mregencoding"],
    ),
    ("ensembleSize", &["ensemblesize"]),
    ("fontSize", &["mfont"]),
    ("frequency", &["frequencies"]),
    ("generation", &["mgeneration"]),
    // Only the government and serial publication types; topical genres come from LCGFT,
    // which the library chooses term by term (item 12).
    ("genreForm", &["mgovtpubtype", "mserialpubtype"]),
    ("illustrativeContent", &["millus"]),
    ("intendedAudience", &["maudience"]),
    ("issuance", &["issuance"]),
    ("language", &["languages"]),
    ("layout", &["mlayout"]),
    ("media", &["mediaTypes"]),
    ("mediumComponentQualifier", &["medcompqual"]),
    ("musicFormat", &["mmusicformat"]),
    ("notation", &["mmusnotation", "mscript", "mtactile"]),
    // `bf:noteType` and `bf:variantType` are literals: the label of the concept is the value.
    ("noteType", &["mnotetype"]),
    ("originPlace", &["countries"]),
    ("place", &["countries"]),
    ("polarity", &["mpolarity"]),
    ("productionMethod", &["mproduction"]),
    ("projection", &["mprojection"]),
    ("projectionCharacteristic", &["mpresformat"]),
    ("reductionRatio", &["mreductionratio"]),
    ("relationship", &["relationship"]),
    ("relief", &["mrelief"]),
    ("role", &["relators"]),
    (
        "soundCharacteristic",
        &[
            "mcapturestorage",
            "mgroove",
            "mplayback",
            "mplayspeed",
            "mrecmedium",
            "mrectype",
            "mspecplayback",
            "mtapeconfig",
        ],
    ),
    ("soundContent", &["msoundcontent"]),
    ("status", &["mstatus"]),
    ("supplementaryContent", &["msupplcont"]),
    ("variantType", &["vartitletype"]),
    ("videoCharacteristic", &["mbroadstd", "mvidformat"]),
];

/// Downloaded vocabularies no BIBFRAME property takes, and why. Every scheme is either
/// here or in [`PROPERTY_SCHEMES`], so a new vocabulary in a download is a decision.
const UNUSED_SCHEMES: &[(&str, &str)] = &[
    (
        "marcauthen",
        "authentication codes of the BFLC extension, not BIBFRAME",
    ),
    (
        "menclvl",
        "encoding level of the BFLC extension, not BIBFRAME",
    ),
    (
        "mscale",
        "kinds of scale; bf:scale takes the scale statement itself",
    ),
    (
        "mtechnique",
        "moving image technique; BIBFRAME has no property for it",
    ),
    (
        "rbmsrel",
        "rare books relationship designators, an RBMS vocabulary",
    ),
    (
        "resourceComponents",
        "parts of a resource for the BFLC extension",
    ),
];

/// The downloaded vocabularies no BIBFRAME property takes, each with the reason.
#[must_use]
pub fn set_aside() -> &'static [(&'static str, &'static str)] {
    UNUSED_SCHEMES
}

/// The downloaded files.
#[must_use]
pub fn sources() -> &'static [VocabularySource] {
    SOURCES
}

/// Every vocabulary, sorted by name.
#[must_use]
pub fn schemes() -> &'static [&'static Scheme] {
    SCHEMES
}

/// The vocabulary called `name`.
#[must_use]
pub fn scheme(name: &str) -> Option<&'static Scheme> {
    let at = SCHEMES
        .binary_search_by(|scheme| scheme.name.cmp(name))
        .ok()?;
    SCHEMES.get(at).copied()
}

/// The vocabularies the values of `property` come from, empty for a property that takes
/// free text or another resource.
pub fn schemes_of(property: &str) -> impl Iterator<Item = &'static Scheme> {
    PROPERTY_SCHEMES
        .iter()
        .filter(move |(name, _)| *name == property)
        .flat_map(|(_, schemes)| schemes.iter())
        .filter_map(|name| scheme(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bibframe::{Entity, property};

    #[test]
    fn the_vocabularies_are_the_pinned_downloads() {
        assert_eq!(sources().len(), 3);
        assert_eq!(schemes().len(), 57);
        assert_eq!(
            schemes().iter().map(|s| s.concepts.len()).sum::<usize>(),
            2205
        );
        assert!(schemes().is_sorted_by(|a, b| a.name < b.name));
        for scheme in schemes() {
            assert!(!scheme.concepts.is_empty(), "{}", scheme.name);
            assert!(
                scheme.concepts.is_sorted_by(|a, b| a.code < b.code),
                "{}",
                scheme.name
            );
        }
    }

    #[test]
    fn concepts_are_found_by_code_with_their_iri() {
        let content = scheme("contentTypes").unwrap();
        assert_eq!(content.concept("txt").unwrap().label, "text");
        assert_eq!(
            content.concept_iri("txt"),
            "http://id.loc.gov/vocabulary/contentTypes/txt"
        );
        assert_eq!(
            scheme("languages").unwrap().concept("por").unwrap().label,
            "Portuguese"
        );
        assert_eq!(
            scheme("countries").unwrap().concept("bl").unwrap().label,
            "Brazil"
        );
        assert_eq!(
            scheme("relators").unwrap().concept("aut").unwrap().label,
            "author"
        );
        assert!(scheme("contentTypes").unwrap().concept("BK").is_none());
    }

    #[test]
    fn deprecated_concepts_are_read_but_not_offered() {
        let relators = scheme("relators").unwrap();
        let afterword = relators.concept("aft").unwrap();
        assert!(afterword.deprecated);
        assert!(relators.offered().all(|concept| concept.code != "aft"));
        assert_eq!(relators.offered().count(), relators.concepts.len() - 3);
    }

    #[test]
    fn every_scheme_is_used_by_a_property_or_set_aside() {
        for (name, schemes) in PROPERTY_SCHEMES {
            assert!(
                property(name).is_some(),
                "{name} is not a BIBFRAME property"
            );
            for scheme_name in *schemes {
                assert!(
                    scheme(scheme_name).is_some(),
                    "{name}: no scheme {scheme_name}"
                );
            }
        }
        for scheme in schemes() {
            let used = PROPERTY_SCHEMES
                .iter()
                .any(|(_, schemes)| schemes.contains(&scheme.name));
            let unused = UNUSED_SCHEMES.iter().any(|(name, _)| *name == scheme.name);
            assert!(
                used != unused,
                "{}: used {used}, set aside {unused}",
                scheme.name
            );
        }
    }

    /// The categories the directive names (item 14) all have their official list.
    #[test]
    fn the_material_type_and_item_lists_come_from_the_library_of_congress() {
        let codes = |property: &str| -> Vec<&str> {
            schemes_of(property)
                .flat_map(Scheme::offered)
                .map(|concept| concept.code)
                .collect()
        };
        assert!(codes("content").contains(&"txt") && codes("content").contains(&"tct"));
        assert!(codes("content").contains(&"spw"));
        assert!(codes("media").contains(&"n") && codes("media").contains(&"c"));
        assert!(codes("carrier").contains(&"nc") && codes("carrier").contains(&"cr"));
        assert!(codes("carrier").contains(&"sd") && codes("carrier").contains(&"vd"));
        assert!(codes("intendedAudience").contains(&"juv"));
        assert!(!codes("status").is_empty());
        for (name, _) in PROPERTY_SCHEMES {
            let property = property(name).unwrap();
            let described = Entity::ALL
                .iter()
                .any(|entity| property.applies_to(*entity));
            let nested = !property.domains.is_empty()
                || matches!(*name, "place" | "noteType" | "variantType");
            assert!(described || nested, "{name} describes nothing");
        }
    }
}
