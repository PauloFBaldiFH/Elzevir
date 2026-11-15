// SPDX-License-Identifier: GPL-3.0-or-later
//! The classes and the content, media and carrier that stand for a category of material
//! (docs/specs/catalogo/bibframe-completo.md, item 31): what a record that only has a
//! `material_type` becomes, until the forms choose classes and lists directly.

use elzevir_domain::catalog::MaterialType;
use elzevir_domain::statement::{Description, Statement, Value};

/// The Work's classes and `bf:content` for `material`, after the Library of Congress
/// conversion of Leader/06 and /07 (`marc2bibframe2`, `conf/codeMaps.xml`, `contentTypes`).
#[must_use]
pub fn work_of_material(material: MaterialType) -> Description {
    let (classes, content): (&[&str], Option<&str>) = match material {
        MaterialType::Book => (&["Text", "Monograph"], Some("txt")),
        MaterialType::Periodical => (&["Text", "Serial"], Some("txt")),
        MaterialType::Map => (&["Cartography"], Some("cri")),
        MaterialType::Score => (&["NotatedMusic"], Some("ntm")),
        // Leader/06 `i` and `j` tell spoken word from music; the category does not.
        MaterialType::SoundRecording => (&["Audio"], None),
        MaterialType::Video => (&["MovingImage"], Some("tdi")),
        MaterialType::Digital => (&["Multimedia"], Some("cop")),
        MaterialType::VisualMaterial => (&["StillImage"], Some("sti")),
        MaterialType::Mixed => (&["MixedMaterial"], None),
    };
    Description {
        classes: classes.iter().map(|class| (*class).to_owned()).collect(),
        statements: content
            .map(|code| concept("content", "contentTypes", code))
            .into_iter()
            .collect(),
    }
}

/// The Instance's classes for its own category: only an electronic edition says one.
#[must_use]
pub fn instance_of_material(material: Option<MaterialType>) -> Description {
    Description {
        classes: match material {
            Some(MaterialType::Digital) => vec!["Electronic".to_owned()],
            _ => Vec::new(),
        },
        statements: Vec::new(),
    }
}

/// A statement whose value is a Library of Congress concept.
#[must_use]
pub fn concept(property: &str, scheme: &str, code: &str) -> Statement {
    Statement {
        property: property.to_owned(),
        value: Value::Concept {
            scheme: scheme.to_owned(),
            code: code.to_owned(),
        },
    }
}

/// The other titles of `descriptions`, in order, each as its main title and subtitle joined
/// as on the title page, for search and display (bibframe-completo.md, item 29, part 5a).
#[must_use]
pub fn other_titles(descriptions: &[&Description]) -> Vec<String> {
    descriptions
        .iter()
        .flat_map(|description| &description.statements)
        .filter(|statement| elzevir_domain::statement::is_other_title(statement))
        .filter_map(|statement| match &statement.value {
            Value::Resource(title) => {
                let part = |property: &str| {
                    title.statements.iter().find_map(|s| match &s.value {
                        Value::Literal { text, .. } if s.property == property => {
                            Some(text.as_str())
                        }
                        _ => None,
                    })
                };
                let main = part("mainTitle")?;
                Some(match part("subtitle") {
                    Some(subtitle) => format!("{main} : {subtitle}"),
                    None => main.to_owned(),
                })
            }
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use elzevir_domain::statement::{EntityKind, check_description};

    #[test]
    fn every_category_becomes_a_valid_description() {
        for &material in MaterialType::ALL {
            let work = work_of_material(material);
            assert!(!work.classes.is_empty(), "{material:?}");
            check_description(EntityKind::Work, &work).unwrap();
            check_description(EntityKind::Instance, &instance_of_material(Some(material))).unwrap();
        }
        assert_eq!(
            work_of_material(MaterialType::Book),
            Description {
                classes: vec!["Text".to_owned(), "Monograph".to_owned()],
                statements: vec![concept("content", "contentTypes", "txt")],
            }
        );
    }
}
