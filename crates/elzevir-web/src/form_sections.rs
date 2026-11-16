// SPDX-License-Identifier: GPL-3.0-or-later
//! The form of a Work or an Instance as every property the ontology gives it, always, in
//! fixed sections (docs/specs/catalogo/bibframe-completo.md, items 24 to 26): no field is
//! hidden behind "expand" and none depends on the category of material.
//!
//! A property with a typed part of the model has that part's fields; every other one, and
//! a typed one that also takes statements (a variant title, a described classification),
//! has a field of rows that edits its statements.

use elzevir_app::catalog::NewRecord;
use elzevir_app::profiles::{InstanceField, WorkField, profile_for};
use elzevir_app::statement_rows::{StatementRow, concept_choices, is_literal, is_relation, row_of};
use elzevir_data::bibframe::{Entity, Property, properties_of};
use elzevir_domain::statement::{Statement, Value, is_typed};
use elzevir_web_templates::{
    ColumnKind, FormCell, FormColumn, FormField, FormInput, FormOption, FormSection, I18n,
};

use crate::record_form::{instance_field, work_field};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    Title,
    Content,
    Contributions,
    Subjects,
    Classification,
    Responsibility,
    Edition,
    Provision,
    PhysicalDescription,
    MediaAndCarrier,
    Identifiers,
    Series,
    Relations,
    Notes,
    ElectronicLocator,
    Location,
    AdminMetadata,
}

/// How the form of one entity is laid out.
struct Layout<F: 'static> {
    entity: Entity,
    /// The sections, in order, with the message key of each heading.
    sections: &'static [(Section, &'static str)],
    /// The typed parts, by the property they hold.
    typed: &'static [(&'static str, &'static [F])],
    /// Typed properties that also take statements of another shape.
    with_statements: &'static [&'static str],
    /// What is edited elsewhere: the structure of the catalogue (the entities above and
    /// below) and the typed parts of an Item, which the item grid edits.
    elsewhere: &'static [&'static str],
    /// Typed properties that have no field yet. The list only shrinks.
    pending: &'static [&'static str],
    section_of: fn(&Property) -> Section,
}

const WORK: Layout<WorkField> = Layout {
    entity: Entity::Work,
    sections: &[
        (Section::Title, "edit-section-title"),
        (Section::Content, "edit-section-content"),
        (Section::Contributions, "edit-section-contributions"),
        (Section::Subjects, "edit-section-subjects"),
        (Section::Classification, "edit-section-classification"),
        (Section::Relations, "edit-section-relations"),
        (Section::Notes, "edit-section-notes"),
        (Section::AdminMetadata, "edit-section-admin-metadata"),
    ],
    typed: &[
        ("title", &[WorkField::Title]),
        ("contribution", &[WorkField::Contributions]),
        ("classification", &[WorkField::Classification]),
        ("language", &[WorkField::Languages]),
        ("subject", &[WorkField::Subjects]),
        ("summary", &[WorkField::Summary]),
        ("intendedAudience", &[WorkField::Audience]),
        // One field holds the five typed relations, each row with its kind.
        ("translationOf", &[WorkField::Relations]),
        ("derivativeOf", &[]),
        ("partOf", &[]),
        ("supplementTo", &[]),
        ("continues", &[]),
    ],
    // Variant titles, the languages of parts, described classifications, coded audiences.
    with_statements: &["title", "language", "classification", "intendedAudience"],
    elsewhere: &["expressionOf", "hasExpression", "hasInstance"],
    pending: &[],
    section_of: work_section,
};

const INSTANCE: Layout<InstanceField> = Layout {
    entity: Entity::Instance,
    sections: &[
        (Section::Title, "edit-section-title"),
        (Section::Responsibility, "edit-section-responsibility"),
        (Section::Edition, "edit-section-edition"),
        (Section::Provision, "edit-section-provision"),
        (
            Section::PhysicalDescription,
            "edit-section-physical-description",
        ),
        (Section::MediaAndCarrier, "edit-section-media-and-carrier"),
        (Section::Content, "edit-section-content"),
        (Section::Identifiers, "edit-section-identifiers"),
        (Section::Series, "edit-section-series"),
        (Section::Relations, "edit-section-relations"),
        (Section::Notes, "edit-section-notes"),
        (
            Section::ElectronicLocator,
            "edit-section-electronic-locator",
        ),
        (Section::AdminMetadata, "edit-section-admin-metadata"),
    ],
    typed: &[
        ("title", &[InstanceField::Title]),
        ("editionStatement", &[InstanceField::Edition]),
        (
            "provisionActivity",
            &[
                InstanceField::Place,
                InstanceField::Publisher,
                InstanceField::Date,
            ],
        ),
        ("extent", &[InstanceField::Extent]),
        ("seriesStatement", &[InstanceField::Series]),
        ("identifiedBy", &[InstanceField::Identifiers]),
        ("note", &[InstanceField::Notes]),
    ],
    // Variant titles, production, distribution, manufacture and the coded publication,
    // numbers of a named agency, notes of a type.
    with_statements: &["title", "provisionActivity", "identifiedBy", "note"],
    elsewhere: &["instanceOf", "hasItem"],
    pending: &[],
    section_of: instance_section,
};

const HUB: Layout<WorkField> = Layout {
    entity: Entity::Hub,
    sections: &[
        (Section::Title, "edit-section-title"),
        (Section::Contributions, "edit-section-contributions"),
        (Section::Content, "edit-section-content"),
        (Section::Identifiers, "edit-section-identifiers"),
        (Section::Relations, "edit-section-relations"),
        (Section::AdminMetadata, "edit-section-admin-metadata"),
    ],
    typed: &[],
    // The main title is the Hub's own column; other titles are variant ones.
    with_statements: &["title"],
    elsewhere: &["hasExpression"],
    pending: &[],
    section_of: hub_section,
};

const ITEM: Layout<WorkField> = Layout {
    entity: Entity::Item,
    sections: &[
        (Section::Title, "edit-section-title"),
        (Section::Content, "edit-section-content"),
        (
            Section::PhysicalDescription,
            "edit-section-physical-description",
        ),
        (Section::Location, "edit-section-location"),
        (Section::Relations, "edit-section-relations"),
        (Section::Notes, "edit-section-notes"),
        (Section::AdminMetadata, "edit-section-admin-metadata"),
    ],
    typed: &[],
    // A number given by a named agency, beside the barcode of the grid.
    with_statements: &["identifiedBy"],
    elsewhere: &[
        "itemOf",
        "shelfMark",
        "heldBy",
        "sublocation",
        "status",
        "usageAndAccessPolicy",
        "immediateAcquisition",
    ],
    pending: &[],
    section_of: item_section,
};

fn hub_section(property: &Property) -> Section {
    match property.name {
        "title" => Section::Title,
        "contribution" => Section::Contributions,
        "identifiedBy" => Section::Identifiers,
        "adminMetadata" => Section::AdminMetadata,
        _ if is_relation(property) => Section::Relations,
        _ => Section::Content,
    }
}

fn item_section(property: &Property) -> Section {
    match property.name {
        "title" => Section::Title,
        "contribution" | "subject" | "genreForm" | "classification" | "identifiedBy" => {
            Section::Content
        }
        "physicalLocation" | "electronicLocator" | "enumerationAndChronology" => Section::Location,
        "adminMetadata" => Section::AdminMetadata,
        _ if is_note(property) => Section::Notes,
        _ if is_relation(property) => Section::Relations,
        _ => Section::PhysicalDescription,
    }
}

/// Every field of a Hub or an Item described by `statements`: neither has a typed part
/// here (the Hub's title is its card's, the Item's typed parts are the grid's).
pub(crate) fn described_sections(entity: Entity, statements: &[Statement]) -> Vec<FormSection> {
    let layout = if entity == Entity::Hub { &HUB } else { &ITEM };
    lay_out(layout, None, statements, |_| None)
}

fn work_section(property: &Property) -> Section {
    match property.name {
        "title" => Section::Title,
        "contribution" => Section::Contributions,
        "subject" | "genreForm" | "geographicCoverage" | "temporalCoverage" => Section::Subjects,
        "classification" => Section::Classification,
        "adminMetadata" => Section::AdminMetadata,
        _ if is_note(property) => Section::Notes,
        _ if is_relation(property) => Section::Relations,
        _ => Section::Content,
    }
}

fn instance_section(property: &Property) -> Section {
    match property.name {
        "title" => Section::Title,
        "responsibilityStatement" | "contribution" => Section::Responsibility,
        "editionStatement" | "editionEnumeration" => Section::Edition,
        "provisionActivity"
        | "provisionActivityStatement"
        | "publicationStatement"
        | "productionStatement"
        | "distributionStatement"
        | "manufactureStatement"
        | "copyrightDate"
        | "phonogramDate"
        | "copyrightRegistration" => Section::Provision,
        "media" | "carrier" | "issuance" => Section::MediaAndCarrier,
        "identifiedBy" => Section::Identifiers,
        "seriesStatement"
        | "seriesEnumeration"
        | "subseriesStatement"
        | "subseriesEnumeration"
        | "hasSeries"
        | "hasSubseries"
        | "seriesOf"
        | "subseriesOf" => Section::Series,
        "subject"
        | "genreForm"
        | "classification"
        | "intendedAudience"
        | "illustrativeContent"
        | "supplementaryContent"
        | "natureOfContent"
        | "contentAccessibility"
        | "colorContent"
        | "soundContent"
        | "notation"
        | "musicFormat"
        | "cartographicAttributes"
        | "scale"
        | "aspectRatio"
        | "duration"
        | "dissertation"
        | "frequency"
        | "pubFrequency"
        | "firstIssue"
        | "lastIssue"
        | "status" => Section::Content,
        "electronicLocator" => Section::ElectronicLocator,
        "adminMetadata" => Section::AdminMetadata,
        _ if is_note(property) => Section::Notes,
        _ if is_relation(property) => Section::Relations,
        _ => Section::PhysicalDescription,
    }
}

fn is_note(property: &Property) -> bool {
    matches!(
        property.name,
        "note"
            | "summary"
            | "tableOfContents"
            | "review"
            | "awards"
            | "credits"
            | "preferredCitation"
            | "custodialHistory"
            | "historyOfWork"
            | "acquisitionSource"
            | "acquisitionTerms"
    )
}

/// Every field of the Work of `record`, by section.
pub(crate) fn work_sections(t: &I18n, record: &NewRecord) -> Vec<FormSection> {
    // The category is the Work's classes, not a property: it opens the content.
    let category = (
        Section::Content,
        work_field(t, record, WorkField::MaterialType),
    );
    lay_out(
        &WORK,
        Some(category),
        &record.work_description.statements,
        |part| Some(work_field(t, record, part)),
    )
}

/// Every field of the Instance of `record`, by section.
pub(crate) fn instance_sections(t: &I18n, record: &NewRecord) -> Vec<FormSection> {
    let material = record
        .instance
        .material_type
        .unwrap_or(record.material_type);
    // A new identifier starts as the category's usual one (an ISBN, an ISSN).
    let identifier = profile_for(material).identifier;
    let category = (
        Section::MediaAndCarrier,
        instance_field(t, record, identifier, InstanceField::MaterialType),
    );
    lay_out(
        &INSTANCE,
        Some(category),
        &record.instance.description.statements,
        |part| Some(instance_field(t, record, identifier, part)),
    )
}

fn lay_out<F: Copy>(
    layout: &Layout<F>,
    category: Option<(Section, FormField)>,
    statements: &[Statement],
    typed_field: impl Fn(F) -> Option<FormField>,
) -> Vec<FormSection> {
    let mut sections: Vec<(Section, FormSection)> = layout
        .sections
        .iter()
        .map(|(section, key)| {
            (
                *section,
                FormSection {
                    heading_key: Some(key),
                    fields: Vec::new(),
                },
            )
        })
        .collect();
    let mut push = |section: Section, field: FormField| {
        if let Some((_, form)) = sections.iter_mut().find(|(s, _)| *s == section) {
            form.fields.push(field);
        }
    };
    if let Some((section, field)) = category {
        push(section, field);
    }
    for property in properties_of(layout.entity) {
        if layout.elsewhere.contains(&property.name) || layout.pending.contains(&property.name) {
            continue;
        }
        let section = (layout.section_of)(property);
        if let Some((_, parts)) = layout.typed.iter().find(|(name, _)| *name == property.name) {
            for field in parts.iter().filter_map(|part| typed_field(*part)) {
                push(section, field);
            }
        }
        if !is_typed(layout.entity, property.name)
            || layout.with_statements.contains(&property.name)
        {
            push(section, statement_field(property, statements));
        }
    }
    sections.into_iter().map(|(_, form)| form).collect()
}

/// The rows of the statements of `property` among `statements`.
pub(crate) fn statement_field(property: &'static Property, statements: &[Statement]) -> FormField {
    let stored: Vec<&Value> = statements
        .iter()
        .filter(|statement| statement.property == property.name)
        .map(|statement| &statement.value)
        .collect();
    let columns = columns_of(property, &stored);
    let rows = stored
        .iter()
        .map(|value| cells(&columns, &row_of(value)))
        .collect();
    FormField {
        change: "Statements",
        error: property.name,
        label_key: "",
        input: FormInput::Rows {
            rows,
            blank: cells(&columns, &StatementRow::default()),
        },
        property: property.name,
        ontology_label: property.label,
        inherited: false,
    }
}

const fn column(name: &'static str, label_key: &'static str, kind: ColumnKind) -> FormColumn {
    FormColumn {
        name,
        label_key,
        kind,
        optional: true,
    }
}

/// A literal is a text and its language; any other value a concept of the property's
/// vocabularies (when it has one), a label or an IRI.
fn columns_of(property: &Property, stored: &[&Value]) -> Vec<FormColumn> {
    let kept = column("kept", "edit-column-kept", ColumnKind::Hidden);
    if is_literal(property) {
        return vec![
            column("text", "edit-column-text", ColumnKind::Text),
            column("language", "edit-column-language", ColumnKind::Text),
            kept,
        ];
    }
    let mut columns = Vec::new();
    let mut options: Vec<FormOption> = concept_choices(property.name)
        .map(|(value, label)| FormOption {
            value,
            label: label.to_owned(),
        })
        .collect();
    if !options.is_empty() {
        // A stored concept the Library of Congress has since deprecated stays choosable.
        for value in stored {
            if let Value::Concept { scheme, code } = value {
                let value = format!("{scheme}/{code}");
                if !options.iter().any(|option| option.value == value) {
                    options.push(FormOption {
                        label: value.clone(),
                        value,
                    });
                }
            }
        }
        options.insert(
            0,
            FormOption {
                value: String::new(),
                label: String::new(),
            },
        );
        columns.push(column(
            "concept",
            "edit-column-concept",
            ColumnKind::Select(options),
        ));
    }
    columns.push(column("text", "edit-column-label", ColumnKind::Text));
    columns.push(column("iri", "edit-column-iri", ColumnKind::Text));
    columns.push(kept);
    columns
}

fn cells(columns: &[FormColumn], row: &StatementRow) -> Vec<FormCell> {
    columns
        .iter()
        .map(|column| {
            let value = match column.name {
                "text" => &row.text,
                "language" => &row.language,
                "concept" => &row.concept,
                "iri" => &row.iri,
                _ => &row.kept,
            };
            FormCell {
                column: column.clone(),
                value: value.clone().unwrap_or_default(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use elzevir_domain::catalog::MaterialType;

    use super::*;
    use crate::new_record::blank;

    fn uncovered<F>(layout: &Layout<F>, sections: &[FormSection]) -> Vec<&'static str> {
        let fields: Vec<&FormField> = sections.iter().flat_map(|s| &s.fields).collect();
        properties_of(layout.entity)
            .map(|property| property.name)
            .filter(|name| {
                let typed = layout.typed.iter().any(|(typed, _)| typed == name);
                let rows = fields.iter().any(|field| field.property == *name);
                let left = layout.elsewhere.contains(name) || layout.pending.contains(name);
                !(typed || rows || left)
            })
            .collect()
    }

    /// Form coverage (bibframe-completo.md, step 6): every property of a Work and of an
    /// Instance has a field, except the structure of the catalogue and what is pending.
    #[test]
    fn every_property_of_a_work_and_an_instance_has_a_field() {
        let t = I18n::english().unwrap();
        let record = blank(MaterialType::Book);
        for (sections, missing, count) in [
            (
                work_sections(&t, &record),
                uncovered(&WORK, &work_sections(&t, &record)),
                WORK.sections.len(),
            ),
            (
                instance_sections(&t, &record),
                uncovered(&INSTANCE, &instance_sections(&t, &record)),
                INSTANCE.sections.len(),
            ),
        ] {
            assert!(
                missing.is_empty(),
                "properties without a field: {missing:?}"
            );
            assert_eq!(sections.len(), count);
            assert!(sections.iter().all(|section| !section.fields.is_empty()));
            let fields = || sections.iter().flat_map(|s| &s.fields);
            assert!(fields().any(|field| field.change == "MaterialType"));
        }
        assert!(WORK.pending.is_empty() && INSTANCE.pending.is_empty());
        for (layout, entity) in [(&HUB, Entity::Hub), (&ITEM, Entity::Item)] {
            let sections = described_sections(entity, &[]);
            assert!(uncovered(layout, &sections).is_empty(), "{entity:?}");
            assert!(sections.iter().all(|section| !section.fields.is_empty()));
            assert!(layout.pending.is_empty());
        }
    }

    #[test]
    fn stored_statements_are_rows_that_keep_their_value() {
        let property = elzevir_data::bibframe::property("content").unwrap();
        let statements = [Statement {
            property: "content".to_owned(),
            value: Value::Concept {
                scheme: "contentTypes".to_owned(),
                code: "txt".to_owned(),
            },
        }];
        let field = statement_field(property, &statements);
        let FormInput::Rows { rows, blank } = field.input else {
            panic!("rows")
        };
        let names: Vec<_> = blank.iter().map(|cell| cell.column.name).collect();
        assert_eq!(names, ["concept", "text", "iri", "kept"]);
        assert_eq!(rows[0][0].value, "contentTypes/txt");
        assert!(rows[0][3].value.contains("\"txt\""));
        assert_eq!(field.ontology_label, property.label);
    }
}
