// SPDX-License-Identifier: GPL-3.0-or-later
//! The form of a focused Work or Instance, built from its profile
//! (docs/specs/catalogo/edicao.md, items 25 to 32). Each field's `change` and each
//! column's `name` are the edit's own JSON names, so the script sends what the route reads.

use elzevir_app::catalog::NewRecord;
use elzevir_app::profiles::{InstanceField, WorkField};
use elzevir_data::bibframe;
use elzevir_domain::catalog::{AgentKind, IdentifierKind, NoteKind, SubjectKind, WorkRelationKind};
use elzevir_domain::statement::EntityKind;
use elzevir_web_templates::{
    ColumnKind, FormCell, FormColumn, FormField, FormInput, FormOption, I18n, RecordForm,
};

use crate::form_sections::{instance_sections, work_sections};
use crate::icons::class_icon;

/// The serde names of the vocabularies, with their labels. A unit test checks each name
/// against the type's own serde form.
const AGENTS: &[(AgentKind, &str, &str)] = &[
    (AgentKind::Person, "person", "staff-agent-person"),
    (
        AgentKind::Organization,
        "organization",
        "staff-agent-organization",
    ),
    (AgentKind::Meeting, "meeting", "staff-agent-meeting"),
    (AgentKind::Family, "family", "staff-agent-family"),
];

const SUBJECTS: &[(SubjectKind, &str, &str)] = &[
    (SubjectKind::Topic, "topic", "staff-subject-topic"),
    (SubjectKind::Place, "place", "staff-subject-place"),
    (SubjectKind::Person, "person", "staff-subject-person"),
    (
        SubjectKind::Organization,
        "organization",
        "staff-subject-organization",
    ),
    (SubjectKind::Meeting, "meeting", "staff-subject-meeting"),
    (SubjectKind::Family, "family", "staff-subject-family"),
    (SubjectKind::Title, "title", "staff-subject-title"),
    (
        SubjectKind::GenreForm,
        "genre_form",
        "staff-subject-genre-form",
    ),
    (SubjectKind::Temporal, "temporal", "staff-subject-temporal"),
];

const IDENTIFIERS: &[(IdentifierKind, &str, &str)] = &[
    (IdentifierKind::Isbn, "isbn", "staff-identifier-isbn"),
    (IdentifierKind::Issn, "issn", "staff-identifier-issn"),
    (IdentifierKind::Lccn, "lccn", "staff-identifier-lccn"),
    (IdentifierKind::Ean, "ean", "staff-identifier-ean"),
    (
        IdentifierKind::SystemControlNumber,
        "system_control_number",
        "staff-identifier-system-control-number",
    ),
    (IdentifierKind::Doi, "doi", "staff-identifier-doi"),
    (IdentifierKind::Ismn, "ismn", "staff-identifier-ismn"),
];

const NOTES: &[(NoteKind, &str, &str)] = &[
    (NoteKind::General, "general", "staff-fields-note-general"),
    (NoteKind::Contents, "contents", "staff-fields-note-contents"),
    (NoteKind::Local, "local", "staff-fields-note-local"),
];

/// The serde name of an agent kind.
pub(crate) fn agent_name(kind: AgentKind) -> &'static str {
    name_of(AGENTS, kind)
}

/// The serde name of a subject kind.
pub(crate) fn subject_name(kind: SubjectKind) -> &'static str {
    name_of(SUBJECTS, kind)
}

fn name_of<T: PartialEq + Copy>(
    table: &[(T, &'static str, &'static str)],
    value: T,
) -> &'static str {
    table
        .iter()
        .find(|(variant, _, _)| *variant == value)
        .map_or("", |(_, name, _)| name)
}

fn options<T>(t: &I18n, table: &[(T, &'static str, &'static str)]) -> Vec<FormOption> {
    table
        .iter()
        .map(|(_, value, key)| FormOption {
            value: (*value).to_owned(),
            label: t.get(key),
            icon: None,
        })
        .collect()
}

const fn column(
    name: &'static str,
    label_key: &'static str,
    kind: ColumnKind,
    optional: bool,
) -> FormColumn {
    FormColumn {
        name,
        label_key,
        kind,
        optional,
    }
}

fn cells(columns: &[FormColumn], values: Vec<String>) -> Vec<FormCell> {
    columns
        .iter()
        .cloned()
        .zip(values)
        .map(|(column, value)| FormCell { column, value })
        .collect()
}

fn rows(columns: &[FormColumn], stored: Vec<Vec<String>>, blank: Vec<String>) -> FormInput {
    FormInput::Rows {
        rows: stored.into_iter().map(|row| cells(columns, row)).collect(),
        blank: cells(columns, blank),
    }
}

fn text(value: Option<&str>) -> FormInput {
    FormInput::Text {
        value: value.unwrap_or_default().to_owned(),
        required: false,
    }
}

/// Whether a field holds a value.
pub(crate) fn is_filled(field: &FormField) -> bool {
    match &field.input {
        FormInput::Text { value, .. }
        | FormInput::LongText { value }
        | FormInput::Select { value, .. } => !value.is_empty(),
        FormInput::Rows { rows, .. } => !rows.is_empty(),
        FormInput::Values { values } => !values.is_empty(),
        FormInput::Group { cells } => cells.iter().any(|cell| !cell.value.is_empty()),
        FormInput::Check { checked } => *checked,
    }
}

fn field(
    change: &'static str,
    error: &'static str,
    label_key: &'static str,
    input: FormInput,
) -> FormField {
    FormField {
        change,
        error,
        label_key,
        input,
        property: "",
        ontology_label: "",
        inherited: false,
    }
}

pub(crate) fn work_field(t: &I18n, record: &NewRecord, part: WorkField) -> FormField {
    match part {
        WorkField::Title => field(
            "Title",
            "title",
            "staff-fields-preferred-title",
            FormInput::Text {
                value: record.work_title.clone(),
                required: true,
            },
        ),
        WorkField::Classes => classes_field(
            EntityKind::Work,
            &record.work_description.classes,
            "staff-fields-work-classes",
        ),
        WorkField::Contributions => contributions_field(t, record),
        WorkField::Classification => classification_field(record),
        WorkField::Languages => field(
            "Languages",
            "languages",
            "staff-fields-languages",
            FormInput::Values {
                values: record.languages.clone(),
            },
        ),
        WorkField::Subjects => subjects_field(t, record),
        WorkField::Relations => relations_field(t, record),
        WorkField::Summary => field(
            "Summary",
            "summary",
            "staff-fields-summary",
            FormInput::LongText {
                value: record.summary.clone().unwrap_or_default(),
            },
        ),
        WorkField::Audience => field(
            "Audience",
            "audience",
            "staff-fields-audience",
            text(record.audience.as_deref()),
        ),
    }
}

pub(crate) fn instance_field(
    t: &I18n,
    record: &NewRecord,
    new_identifier: IdentifierKind,
    part: InstanceField,
) -> FormField {
    let instance = &record.instance;
    match part {
        InstanceField::Title => field(
            "Title",
            "title",
            "staff-fields-title",
            FormInput::Text {
                value: instance.title.clone(),
                required: true,
            },
        ),
        InstanceField::Edition => field(
            "Edition",
            "edition",
            "staff-fields-edition",
            text(instance.edition.as_deref()),
        ),
        InstanceField::Place => field(
            "Place",
            "place",
            "staff-fields-place",
            text(instance.place.as_deref()),
        ),
        InstanceField::Publisher => field(
            "Publisher",
            "publisher",
            "staff-fields-publisher",
            text(instance.publisher.as_deref()),
        ),
        InstanceField::Date => field(
            "Date",
            "date",
            "staff-fields-date",
            text(instance.date.as_deref()),
        ),
        InstanceField::Extent => field(
            "Extent",
            "extent",
            "staff-fields-extent",
            text(instance.extent.as_deref()),
        ),
        InstanceField::Series => field(
            "Series",
            "series",
            "staff-fields-series",
            text(instance.series.as_deref()),
        ),
        InstanceField::Identifiers => identifiers_field(t, record, new_identifier),
        InstanceField::Notes => notes_field(t, record),
        InstanceField::Classes => classes_field(
            EntityKind::Instance,
            &instance.description.classes,
            "staff-fields-instance-classes",
        ),
        InstanceField::OpacHidden => field(
            "OpacHidden",
            "opac_hidden",
            "staff-fields-opac-hidden",
            FormInput::Check {
                checked: instance.opac_hidden,
            },
        ),
    }
}

/// The form of the Work of `record`, read at `version`.
pub(crate) fn work_form<'a>(
    t: &'a I18n,
    record: &NewRecord,
    work: i64,
    version: i64,
) -> RecordForm<'a> {
    RecordForm {
        t,
        endpoint: format!("/equipe/acervo/obra/{work}"),
        version,
        sections: work_sections(t, record),
        history: Some(format!("/equipe/acervo/obra/{work}/historico")),
        reach: None,
    }
}

/// The form of the Instance of `record`, read at `version`.
pub(crate) fn instance_form<'a>(
    t: &'a I18n,
    record: &NewRecord,
    instance: i64,
    version: i64,
) -> RecordForm<'a> {
    RecordForm {
        t,
        endpoint: format!("/equipe/acervo/instancia/{instance}"),
        version,
        sections: instance_sections(t, record),
        history: Some(format!("/equipe/acervo/instancia/{instance}/historico")),
        reach: None,
    }
}

fn contributions_field(t: &I18n, record: &NewRecord) -> FormField {
    let columns = [
        column("heading", "edit-column-name", ColumnKind::Text, false),
        column(
            "kind",
            "edit-column-kind",
            ColumnKind::Select(options(t, AGENTS)),
            false,
        ),
        column("role", "edit-column-role", ColumnKind::Text, true),
        column("primary", "staff-fields-primary", ColumnKind::Check, false),
    ];
    let stored = record
        .contributions
        .iter()
        .map(|contribution| {
            vec![
                contribution.heading.clone(),
                name_of(AGENTS, contribution.kind).to_owned(),
                contribution.role.clone().unwrap_or_default(),
                contribution.primary.to_string(),
            ]
        })
        .collect();
    let blank = vec![
        String::new(),
        "person".to_owned(),
        String::new(),
        "false".to_owned(),
    ];
    field(
        "Contributions",
        "contributions",
        "staff-fields-contributions",
        rows(&columns, stored, blank),
    )
}

fn classification_field(record: &NewRecord) -> FormField {
    let columns = [
        column("number", "edit-column-number", ColumnKind::Text, true),
        column("scheme", "edit-column-scheme", ColumnKind::Text, true),
    ];
    let values = vec![
        record.classification.clone().unwrap_or_default(),
        record.classification_scheme.clone().unwrap_or_default(),
    ];
    field(
        "Classification",
        "classification",
        "staff-fields-classification",
        FormInput::Group {
            cells: cells(&columns, values),
        },
    )
}

fn subjects_field(t: &I18n, record: &NewRecord) -> FormField {
    let columns = [
        column("heading", "edit-column-heading", ColumnKind::Text, false),
        column(
            "kind",
            "edit-column-kind",
            ColumnKind::Select(options(t, SUBJECTS)),
            false,
        ),
        column("thesaurus", "edit-column-thesaurus", ColumnKind::Text, true),
    ];
    let stored = record
        .subjects
        .iter()
        .map(|subject| {
            vec![
                subject.heading.clone(),
                name_of(SUBJECTS, subject.kind).to_owned(),
                subject.thesaurus.clone().unwrap_or_default(),
            ]
        })
        .collect();
    let blank = vec![String::new(), "topic".to_owned(), String::new()];
    field(
        "Subjects",
        "subjects",
        "staff-fields-subjects",
        rows(&columns, stored, blank),
    )
}

const RELATIONS: &[(WorkRelationKind, &str, &str)] = &[
    (
        WorkRelationKind::TranslationOf,
        "translation_of",
        "staff-relation-translation-of",
    ),
    (
        WorkRelationKind::AdaptationOf,
        "adaptation_of",
        "staff-relation-adaptation-of",
    ),
    (
        WorkRelationKind::PartOf,
        "part_of",
        "staff-relation-part-of",
    ),
    (
        WorkRelationKind::SupplementTo,
        "supplement_to",
        "staff-relation-supplement-to",
    ),
    (
        WorkRelationKind::ContinuationOf,
        "continuation_of",
        "staff-relation-continuation-of",
    ),
];

/// The Works this one relates to, each by its number (bibframe-completo.md, step 6d).
/// The subclasses of the entity's class, one select per row, with the ontology's labels
/// and the icon of the kind each stands for (bibframe-completo.md, item 14).
fn classes_field(entity: EntityKind, classes: &[String], label_key: &'static str) -> FormField {
    let parent = entity.bibframe().class_name();
    let mut options: Vec<FormOption> = bibframe::subclasses(parent)
        .map(|class| FormOption {
            value: class.name.to_owned(),
            label: class.label.to_owned(),
            icon: class_icon(class.name),
        })
        .collect();
    options.sort_by(|a, b| a.label.cmp(&b.label));
    // A new row starts with no class; an empty one is dropped when the form is sent.
    options.insert(
        0,
        FormOption {
            value: String::new(),
            label: String::new(),
            icon: None,
        },
    );
    let columns = [column(
        "class",
        "edit-column-class",
        ColumnKind::Select(options),
        false,
    )];
    let stored = classes.iter().map(|class| vec![class.clone()]).collect();
    field(
        "Classes",
        "classes",
        label_key,
        rows(&columns, stored, vec![String::new()]),
    )
}

fn relations_field(t: &I18n, record: &NewRecord) -> FormField {
    let columns = [
        column(
            "kind",
            "edit-column-kind",
            ColumnKind::Select(options(t, RELATIONS)),
            false,
        ),
        column("work", "edit-column-work", ColumnKind::Number, false),
    ];
    let stored = record
        .relations
        .iter()
        .map(|relation| {
            vec![
                name_of(RELATIONS, relation.kind).to_owned(),
                relation.work.0.to_string(),
            ]
        })
        .collect();
    let blank = vec!["translation_of".to_owned(), String::new()];
    field(
        "Relations",
        "relations",
        "staff-fields-relations",
        rows(&columns, stored, blank),
    )
}

fn identifiers_field(t: &I18n, record: &NewRecord, new_identifier: IdentifierKind) -> FormField {
    let columns = [
        column(
            "kind",
            "edit-column-kind",
            ColumnKind::Select(options(t, IDENTIFIERS)),
            false,
        ),
        column("value", "edit-column-value", ColumnKind::Text, false),
        column("qualifier", "edit-column-qualifier", ColumnKind::Text, true),
    ];
    let stored = record
        .instance
        .identifiers
        .iter()
        .map(|identifier| {
            vec![
                name_of(IDENTIFIERS, identifier.kind).to_owned(),
                identifier.value.clone(),
                identifier.qualifier.clone().unwrap_or_default(),
            ]
        })
        .collect();
    let blank = vec![
        name_of(IDENTIFIERS, new_identifier).to_owned(),
        String::new(),
        String::new(),
    ];
    field(
        "Identifiers",
        "identifiers",
        "staff-fields-identifiers",
        rows(&columns, stored, blank),
    )
}

fn notes_field(t: &I18n, record: &NewRecord) -> FormField {
    let columns = [
        column(
            "kind",
            "edit-column-kind",
            ColumnKind::Select(options(t, NOTES)),
            false,
        ),
        column("text", "edit-column-text", ColumnKind::Text, false),
    ];
    let stored = record
        .instance
        .notes
        .iter()
        .map(|note| vec![name_of(NOTES, note.kind).to_owned(), note.text.clone()])
        .collect();
    let blank = vec!["general".to_owned(), String::new()];
    field(
        "Notes",
        "notes",
        "edit-field-notes",
        rows(&columns, stored, blank),
    )
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;
    use serde::de::value::{Error, StrDeserializer};

    use super::*;

    fn parses<'de, T: Deserialize<'de> + PartialEq + std::fmt::Debug>(name: &'de str, expected: T) {
        let parsed = T::deserialize(StrDeserializer::<Error>::new(name));
        assert_eq!(parsed.ok(), Some(expected), "{name}");
    }

    /// Every message key the staff catalog's Rust code names exists in the English messages.
    #[test]
    fn every_key_named_in_code_exists() {
        let t = I18n::english().unwrap();
        let sources = [
            include_str!("record_form.rs"),
            include_str!("record_fields.rs"),
            include_str!("form_sections.rs"),
            include_str!("../../elzevir-app/src/editing.rs"),
            include_str!("../../elzevir-app/src/creating.rs"),
        ];
        let mut missing = Vec::new();
        for source in sources {
            for chunk in source.split('"').skip(1).step_by(2) {
                let is_key = ["staff-", "edit-", "content-", "carrier-"]
                    .iter()
                    .any(|prefix| chunk.starts_with(prefix))
                    && !chunk.contains(' ');
                if is_key && !t.has(chunk) {
                    missing.push(chunk.to_owned());
                }
            }
        }
        assert!(missing.is_empty(), "missing Fluent keys: {missing:?}");
    }

    #[test]
    fn every_name_is_the_serde_name_of_its_variant() {
        for (kind, name, _) in AGENTS {
            parses(name, *kind);
        }
        assert_eq!(AGENTS.len(), AgentKind::ALL.len());
        for (kind, name, _) in SUBJECTS {
            parses(name, *kind);
        }
        assert_eq!(SUBJECTS.len(), SubjectKind::ALL.len());
        for (kind, name, _) in IDENTIFIERS {
            parses(name, *kind);
        }
        assert_eq!(IDENTIFIERS.len(), IdentifierKind::ALL.len());
        for (kind, name, _) in RELATIONS {
            parses(name, *kind);
        }
        assert_eq!(RELATIONS.len(), WorkRelationKind::ALL.len());
        for (kind, name, _) in NOTES {
            parses(name, *kind);
        }
        assert_eq!(NOTES.len(), NoteKind::ALL.len());
    }
}
