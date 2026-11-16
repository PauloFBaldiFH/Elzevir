// SPDX-License-Identifier: GPL-3.0-or-later
//! Helpers over descriptions (docs/specs/catalogo/bibframe-completo.md): the description a
//! new family starts with, and the texts that search and display read from statements.

use elzevir_domain::statement::{Description, Statement, Value};

/// The Work and Instance a new family starts with, as most do: a printed book, that is a
/// monographic text Work with `txt` content and a `bf:Print` Instance. The cataloguer
/// changes what differs (bibframe-completo.md, item 14a).
#[must_use]
pub fn printed_book() -> (Description, Description) {
    (
        Description {
            classes: vec!["Text".to_owned(), "Monograph".to_owned()],
            statements: vec![concept("content", "contentTypes", "txt")],
        },
        Description {
            classes: vec!["Print".to_owned()],
            statements: Vec::new(),
        },
    )
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

/// The texts of the typed notes of `descriptions`, in order, for search and display
/// (bibframe-completo.md, item 29, part 5b).
#[must_use]
pub fn typed_notes(descriptions: &[&Description]) -> Vec<String> {
    descriptions
        .iter()
        .flat_map(|description| &description.statements)
        .filter(|statement| elzevir_domain::statement::is_typed_note(statement))
        .filter_map(|statement| match &statement.value {
            Value::Resource(note) if !is_physical(note) => note.label.clone(),
            _ => None,
        })
        .collect()
}

/// The labels of the Work's genres and forms (`bf:genreForm`), in order, for display
/// (bibframe-completo.md, item 17b). A genre named only by an `id.loc.gov` IRI has no
/// label here yet and is left out: the LCGFT labels come with the authorities.
#[must_use]
pub fn genres(work: &Description) -> Vec<String> {
    work.statements
        .iter()
        .filter(|statement| statement.property == "genreForm")
        .filter_map(|statement| match &statement.value {
            Value::Literal { text, .. } => Some(text.clone()),
            Value::Resource(genre) => genre.label.clone(),
            Value::Concept { scheme, code } => elzevir_data::vocabulary::scheme(scheme)
                .and_then(|scheme| scheme.concept(code))
                .map(|concept| concept.label.to_owned()),
            Value::Entity(_) | Value::Iri(_) => None,
        })
        .collect()
}

/// Whether `note` is part of the physical description (physical details or accompanying
/// material), which is shown with the extent, not with the notes.
fn is_physical(note: &elzevir_domain::statement::Resource) -> bool {
    note.statements.iter().any(|s| {
        s.property == "noteType"
            && matches!(&s.value, Value::Literal { text, .. }
                if text == "physical details" || text == "accompanying material")
    })
}

/// The physical description of an Instance as ISBD writes it: the extent, then ` : ` and
/// the physical details, ` ; ` and the dimensions, ` + ` and the accompanying material
/// (bibframe-completo.md, item 29, part 5e).
#[must_use]
pub fn physical_description(extent: Option<&str>, instance: &Description) -> Option<String> {
    let mut text = extent.unwrap_or_default().to_owned();
    let mut add = |separator: &str, part: &str| {
        if !text.is_empty() {
            text.push_str(separator);
        }
        text.push_str(part);
    };
    for part in notes_of(instance, "physical details") {
        add(" : ", part);
    }
    for statement in &instance.statements {
        if let Value::Literal { text: part, .. } = &statement.value
            && statement.property == "dimensions"
        {
            add(" ; ", part);
        }
    }
    for part in notes_of(instance, "accompanying material") {
        add(" + ", part);
    }
    (!text.is_empty()).then_some(text)
}

/// The texts of the notes of `instance` whose type is `kind`.
fn notes_of<'a>(instance: &'a Description, kind: &'a str) -> impl Iterator<Item = &'a str> {
    instance
        .statements
        .iter()
        .filter_map(move |s| match &s.value {
            Value::Resource(note)
                if s.property == "note"
                    && note.statements.iter().any(|t| {
                        t.property == "noteType"
                            && matches!(&t.value, Value::Literal { text, .. } if text == kind)
                    }) =>
            {
                note.label.as_deref()
            }
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use elzevir_domain::statement::{EntityKind, check_description};

    #[test]
    fn the_physical_description_reads_as_isbd() {
        let note = |kind: &str, text: &str| Statement {
            property: "note".to_owned(),
            value: Value::Resource(elzevir_domain::statement::Resource {
                class: "Note".to_owned(),
                label: Some(text.to_owned()),
                statements: vec![Statement {
                    property: "noteType".to_owned(),
                    value: Value::Literal {
                        text: kind.to_owned(),
                        language: None,
                    },
                }],
            }),
        };
        let instance = Description {
            classes: Vec::new(),
            statements: vec![
                note("accompanying material", "1 CD-ROM"),
                Statement {
                    property: "dimensions".to_owned(),
                    value: Value::Literal {
                        text: "19 cm".to_owned(),
                        language: None,
                    },
                },
                note("physical details", "il."),
                note("bibliography", "Inclui bibliografia."),
            ],
        };
        assert_eq!(
            physical_description(Some("195 p."), &instance).as_deref(),
            Some("195 p. : il. ; 19 cm + 1 CD-ROM")
        );
        assert_eq!(typed_notes(&[&instance]), ["Inclui bibliografia."]);
        assert_eq!(physical_description(None, &Description::default()), None);
    }

    #[test]
    fn the_genres_are_their_labels_and_an_unlabelled_iri_is_left_out() {
        let genre = |value: Value| Statement {
            property: "genreForm".to_owned(),
            value,
        };
        let work = Description {
            classes: Vec::new(),
            statements: vec![
                genre(Value::Resource(elzevir_domain::statement::Resource {
                    class: "GenreForm".to_owned(),
                    label: Some("Romances".to_owned()),
                    statements: Vec::new(),
                })),
                genre(Value::Iri(
                    "http://id.loc.gov/authorities/genreForms/gf2014026339".to_owned(),
                )),
                genre(Value::Literal {
                    text: "Contos".to_owned(),
                    language: None,
                }),
            ],
        };
        assert_eq!(genres(&work), ["Romances", "Contos"]);
    }

    #[test]
    fn a_new_family_starts_as_a_valid_printed_book() {
        let (work, instance) = printed_book();
        check_description(EntityKind::Work, &work).unwrap();
        check_description(EntityKind::Instance, &instance).unwrap();
    }
}
