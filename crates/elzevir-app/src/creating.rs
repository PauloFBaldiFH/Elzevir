// SPDX-License-Identifier: GPL-3.0-or-later
//! Creating records from the staff screen (docs/specs/catalogo/criacao.md, items 5 to 12):
//! the screen sends the same changes as an edit, applied to an empty Work and Instance.

use crate::catalog::{InstanceChange, NewInstance, NewRecord, WorkChange};
use crate::editing::{FieldError, validate_instance_changes, validate_work_changes};
use elzevir_domain::statement::{Description, Statement};

/// Builds a new family (a Work and its first Instance) from the screen's changes. The
/// Instance takes the Work's title when it has none (item 9).
///
/// # Errors
///
/// Every field that cannot be stored as written, and the part a record cannot exist
/// without: the Work's title (item 8).
pub fn new_record(
    mut work: Vec<WorkChange>,
    mut instance: Vec<InstanceChange>,
) -> Result<NewRecord, Vec<FieldError>> {
    let mut errors = Vec::new();
    collect(&mut errors, validate_work_changes(&mut work));
    collect(&mut errors, validate_instance_changes(&mut instance));

    let mut work_title = None;
    let mut record_parts = RecordParts::default();
    for change in work {
        match change {
            WorkChange::Title(title) => work_title = Some(title),
            other => record_parts.apply(other),
        }
    }
    let (instance_title, new_instance) = fold_instance(instance);

    let work_title = work_title.filter(|title| !title.is_empty());
    if work_title.is_none() && !errors.iter().any(|error| error.field == "title") {
        errors.push(required("title", "edit-error-title-empty"));
    }
    let (Some(work_title), true) = (work_title, errors.is_empty()) else {
        return Err(errors);
    };
    let instance = NewInstance {
        title: instance_title.unwrap_or_else(|| work_title.clone()),
        ..new_instance
    };
    let mut work_description = Description {
        classes: record_parts.classes,
        statements: Vec::new(),
    };
    for (property, statements) in record_parts.statements {
        replace(&mut work_description, &property, statements);
    }
    Ok(NewRecord {
        work_title,
        work_description,
        hub: None,
        relations: record_parts.relations,
        classification: record_parts.classification,
        classification_scheme: record_parts.classification_scheme,
        summary: record_parts.summary,
        audience: record_parts.audience,
        languages: record_parts.languages,
        contributions: record_parts.contributions,
        subjects: record_parts.subjects,
        instance,
        items: crate::catalog::RecordItems::default(),
        source: None,
    })
}

/// Builds a new Instance of a stored Work from the screen's changes.
///
/// # Errors
///
/// Every field that cannot be stored as written, and a missing title (item 8).
pub fn new_instance(mut changes: Vec<InstanceChange>) -> Result<NewInstance, Vec<FieldError>> {
    let mut errors = Vec::new();
    collect(&mut errors, validate_instance_changes(&mut changes));
    let (title, instance) = fold_instance(changes);
    match title {
        Some(title) if errors.is_empty() => Ok(NewInstance { title, ..instance }),
        Some(_) => Err(errors),
        None => {
            if !errors.iter().any(|error| error.field == "title") {
                errors.push(required("title", "edit-error-title-empty"));
            }
            Err(errors)
        }
    }
}

/// The Work's parts other than its title, as a new record holds them.
#[derive(Default)]
struct RecordParts {
    classes: Vec<String>,
    classification: Option<String>,
    classification_scheme: Option<String>,
    summary: Option<String>,
    audience: Option<String>,
    languages: Vec<String>,
    contributions: Vec<crate::catalog::NewContribution>,
    subjects: Vec<crate::catalog::NewSubject>,
    relations: Vec<crate::catalog::NewWorkRelation>,
    statements: Vec<(String, Vec<Statement>)>,
}

impl RecordParts {
    fn apply(&mut self, change: WorkChange) {
        match change {
            WorkChange::Classes(classes) => {
                self.classes = classes.into_iter().map(|choice| choice.class).collect();
            }
            WorkChange::Classification { number, scheme } => {
                self.classification = number;
                self.classification_scheme = scheme;
            }
            WorkChange::Summary(text) => self.summary = text,
            WorkChange::Audience(text) => self.audience = text,
            WorkChange::Languages(codes) => self.languages = codes,
            WorkChange::Contributions(contributions) => self.contributions = contributions,
            WorkChange::Subjects(subjects) => self.subjects = subjects,
            WorkChange::Relations(relations) => self.relations = relations,
            WorkChange::Statements {
                property,
                statements,
            } => self.statements.push((property, statements)),
            // The title is read by the caller before the rest.
            WorkChange::Title(_) => {}
        }
    }
}

/// The Instance the changes describe, and its title if one was given and is not empty.
/// A later change of the same part wins, as in an edit.
fn fold_instance(changes: Vec<InstanceChange>) -> (Option<String>, NewInstance) {
    let mut title = None;
    let mut instance = NewInstance::default();
    for change in changes {
        match change {
            InstanceChange::Title(text) => title = Some(text),
            InstanceChange::Classes(classes) => {
                instance.description.classes =
                    classes.into_iter().map(|choice| choice.class).collect();
            }
            InstanceChange::Edition(text) => instance.edition = text,
            InstanceChange::Place(text) => instance.place = text,
            InstanceChange::Publisher(text) => instance.publisher = text,
            InstanceChange::Date(text) => instance.date = text,
            InstanceChange::Extent(text) => instance.extent = text,
            InstanceChange::Series(text) => instance.series = text,
            InstanceChange::Identifiers(identifiers) => instance.identifiers = identifiers,
            InstanceChange::Notes(notes) => instance.notes = notes,
            InstanceChange::OpacHidden(hidden) => instance.opac_hidden = hidden,
            InstanceChange::Statements {
                property,
                statements,
            } => replace(&mut instance.description, &property, statements),
        }
    }
    (title.filter(|title| !title.is_empty()), instance)
}

/// The statements of `property` in `description` become `statements`.
fn replace(description: &mut Description, property: &str, statements: Vec<Statement>) {
    description
        .statements
        .retain(|statement| statement.property != property);
    description.statements.extend(statements);
}

fn collect(errors: &mut Vec<FieldError>, result: Result<(), Vec<FieldError>>) {
    if let Err(found) = result {
        errors.extend(found);
    }
}

const fn required(field: &'static str, message_key: &'static str) -> FieldError {
    FieldError { field, message_key }
}

#[cfg(test)]
mod tests {
    use elzevir_domain::catalog::IdentifierKind;

    use super::*;
    use crate::catalog::{ClassChoice, NewIdentifier};

    fn classes(names: &[&str]) -> Vec<ClassChoice> {
        names
            .iter()
            .map(|name| ClassChoice {
                class: (*name).to_owned(),
            })
            .collect()
    }

    #[test]
    fn a_new_family_needs_a_title_and_classes_of_its_own_entity() {
        let errors = new_record(Vec::new(), Vec::new()).unwrap_err();
        assert_eq!(errors, [required("title", "edit-error-title-empty")]);
        // A blank title is reported once, by the edit's own validation.
        let errors = new_record(
            vec![
                WorkChange::Title("  ".to_owned()),
                WorkChange::Classes(classes(&["Text", "Print"])),
            ],
            vec![InstanceChange::Classes(classes(&["Text"]))],
        )
        .unwrap_err();
        assert_eq!(
            errors,
            [
                required("title", "edit-error-title-empty"),
                required("classes", "edit-error-class"),
                required("classes", "edit-error-class"),
            ]
        );
    }

    #[test]
    fn the_chosen_classes_are_the_description() {
        let record = new_record(
            vec![
                WorkChange::Title("Revista".to_owned()),
                WorkChange::Classes(classes(&[" Text ", "", "Serial", "Text"])),
            ],
            vec![InstanceChange::Classes(classes(&["Electronic"]))],
        )
        .unwrap();
        assert_eq!(record.work_description.classes, ["Text", "Serial"]);
        assert_eq!(record.instance.description.classes, ["Electronic"]);
    }

    #[test]
    fn the_instance_takes_the_work_title_when_it_has_none() {
        let record = new_record(
            vec![
                WorkChange::Title(" Dom Casmurro ".to_owned()),
                WorkChange::Classes(classes(&["Text", "Monograph"])),
                WorkChange::Languages(vec!["por".to_owned()]),
                WorkChange::Summary(Some(String::new())),
            ],
            vec![InstanceChange::Publisher(Some("Garnier".to_owned()))],
        )
        .unwrap();
        assert_eq!(record.work_title, "Dom Casmurro");
        assert_eq!(record.instance.title, "Dom Casmurro");
        assert_eq!(record.instance.publisher.as_deref(), Some("Garnier"));
        assert_eq!(record.languages, ["por"]);
        assert_eq!(record.summary, None);
        assert!(record.source.is_none() && record.items.is_empty());
        assert_eq!(record.work_description.classes, ["Text", "Monograph"]);
    }

    #[test]
    fn a_new_instance_needs_a_title_and_valid_identifiers() {
        assert_eq!(
            new_instance(vec![InstanceChange::Edition(Some("2. ed.".to_owned()))]).unwrap_err(),
            [required("title", "edit-error-title-empty")]
        );
        let isbn = |value: &str| NewIdentifier {
            kind: IdentifierKind::Isbn,
            value: value.to_owned(),
            qualifier: None,
        };
        assert_eq!(
            new_instance(vec![
                InstanceChange::Title("Dom Casmurro".to_owned()),
                InstanceChange::Identifiers(vec![isbn("9788535910668")]),
            ])
            .unwrap_err(),
            [required("identifiers", "edit-error-isbn")]
        );
        let instance = new_instance(vec![
            InstanceChange::Title("Dom Casmurro".to_owned()),
            InstanceChange::Identifiers(vec![isbn("978-85-359-1066-7")]),
        ])
        .unwrap();
        assert_eq!(instance.title, "Dom Casmurro");
        assert_eq!(instance.identifiers[0].value, "9788535910667");
    }
}
