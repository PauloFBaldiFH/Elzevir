// SPDX-License-Identifier: GPL-3.0-or-later
//! Records back from the resources of a BIBFRAME document (bibframe-completo.md, item
//! 28d): the inverse of [`record_graph`](super::record_graph). A value of a property that
//! has a column goes to the column when it has the shape the export gives the column, and
//! is a statement otherwise, as the statements check allows.

use std::collections::BTreeMap;

use elzevir_data::vocabulary;
use elzevir_domain::catalog::{
    AgentKind, AuthorityLinkKind, IdentifierKind, InstanceId, ItemId, NamePartKind, NoteKind,
    SubjectKind, WorkId, WorkRelationKind,
};
use elzevir_domain::statement::{Description, EntityKind, Resource, Statement, Value};

use super::{BF, HeldItem, IriBase, MADSRDF, Node, Object, RDF, RDFS};
use crate::catalog::{
    AuthorityLink, ItemStatus, NamePart, NewContribution, NewIdentifier, NewInstance, NewNote,
    NewRecord, NewSubject, NewWorkRelation,
};

/// A record read back: its Work and Instance, as stored, and its Items.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadRecord {
    /// The Work.
    pub work: WorkId,
    /// The Instance.
    pub instance: InstanceId,
    /// The model; without Items (they are `items`) and without a source record.
    pub record: NewRecord,
    /// The Items, in document order.
    pub items: Vec<HeldItem>,
}

/// The records of `nodes`, one per Instance of the catalogue (named under `base`) whose
/// Work is in the document, in document order.
#[must_use]
pub fn records_from(nodes: &[Node], base: &IriBase) -> Vec<ReadRecord> {
    let named: BTreeMap<&str, &Node> = nodes
        .iter()
        .filter_map(|node| Some((node.iri.as_deref()?, node)))
        .collect();
    let reader = Reader { base, named };
    nodes
        .iter()
        .filter(|node| has_type(node, "Instance"))
        .filter_map(|instance| reader.record(instance, nodes))
        .collect()
}

struct Reader<'a> {
    base: &'a IriBase,
    named: BTreeMap<&'a str, &'a Node>,
}

impl Reader<'_> {
    fn record(&self, instance: &Node, nodes: &[Node]) -> Option<ReadRecord> {
        let instance_iri = instance.iri.as_deref()?;
        let instance_id = self.id_of(instance_iri, EntityKind::Instance)?;
        let work_iri = bf_values(instance, "instanceOf").find_map(iri_of)?;
        let work_id = self.id_of(work_iri, EntityKind::Work)?;
        let work = self.named.get(work_iri)?;
        let mut record = self.work(work);
        record.instance = self.instance(instance);
        let items = nodes
            .iter()
            .filter(|node| {
                has_type(node, "Item")
                    && bf_values(node, "itemOf").any(|of| iri_of(of) == Some(instance_iri))
            })
            .filter_map(|node| self.item(node))
            .collect();
        Some(ReadRecord {
            work: WorkId(work_id),
            instance: InstanceId(instance_id),
            record,
            items,
        })
    }

    fn id_of(&self, iri: &str, kind: EntityKind) -> Option<i64> {
        self.base
            .entity_of(iri)
            .filter(|entity| entity.entity == kind)
            .map(|entity| entity.id)
    }

    /// The node `object` is, looking a named one up in the document.
    fn resolve<'n>(&'n self, object: &'n Object) -> Option<&'n Node> {
        match object {
            Object::Node(node) => Some(node),
            Object::Iri(iri) => self.named.get(iri.as_str()).copied(),
            Object::Literal { .. } | Object::List(_) => None,
        }
    }

    fn work(&self, node: &Node) -> NewRecord {
        let mut record = NewRecord {
            work_title: String::new(),
            work_description: Description {
                classes: classes(node, "Work"),
                statements: Vec::new(),
            },
            hub: None,
            relations: Vec::new(),
            classification: None,
            classification_scheme: None,
            summary: None,
            audience: None,
            languages: Vec::new(),
            contributions: Vec::new(),
            subjects: Vec::new(),
            instance: NewInstance::default(),
            items: Vec::new(),
            source: None,
        };
        let mut titled = false;
        for (name, object) in bf_properties(node) {
            match name {
                "hasInstance" => {}
                "title" if !titled && main_title(object).is_some() => {
                    titled = true;
                    main_title(object)
                        .unwrap_or_default()
                        .clone_into(&mut record.work_title);
                }
                "expressionOf" if record.hub.is_none() => {
                    record.hub = iri_of(object)
                        .and_then(|iri| self.id_of(iri, EntityKind::Hub))
                        .map(elzevir_domain::catalog::HubId);
                }
                "contribution" => {
                    if let Some(contribution) = self.contribution(object) {
                        record.contributions.push(contribution);
                    }
                }
                "subject" => {
                    if let Some(subject) = self.resolve(object).and_then(Self::subject) {
                        record.subjects.push(subject);
                    }
                }
                "language" if language_code(object).is_some() => {
                    record
                        .languages
                        .extend(language_code(object).map(str::to_owned));
                }
                "classification" if record.classification.is_none() => {
                    match column_classification(object) {
                        Some((number, scheme)) => {
                            record.classification = Some(number);
                            record.classification_scheme = scheme;
                        }
                        None => self.statement(&mut record.work_description, name, object),
                    }
                }
                "summary" if record.summary.is_none() && labelled(object, "Summary").is_some() => {
                    record.summary = labelled(object, "Summary");
                }
                "intendedAudience"
                    if record.audience.is_none()
                        && labelled(object, "IntendedAudience").is_some() =>
                {
                    record.audience = labelled(object, "IntendedAudience");
                }
                _ => match relation_kind(name)
                    .zip(iri_of(object).and_then(|iri| self.id_of(iri, EntityKind::Work)))
                {
                    Some((kind, id)) => record.relations.push(NewWorkRelation {
                        kind,
                        work: WorkId(id),
                    }),
                    None => self.statement(&mut record.work_description, name, object),
                },
            }
        }
        record
    }

    fn instance(&self, node: &Node) -> NewInstance {
        let mut instance = NewInstance {
            description: Description {
                classes: classes(node, "Instance"),
                statements: Vec::new(),
            },
            ..NewInstance::default()
        };
        let mut titled = false;
        let mut published = false;
        for (name, object) in bf_properties(node) {
            match name {
                "instanceOf" | "hasItem" => {}
                "title" if !titled && main_title(object).is_some() => {
                    titled = true;
                    main_title(object)
                        .unwrap_or_default()
                        .clone_into(&mut instance.title);
                }
                "editionStatement" if instance.edition.is_none() && plain(object).is_some() => {
                    instance.edition = plain(object).map(str::to_owned);
                }
                "seriesStatement" if instance.series.is_none() && plain(object).is_some() => {
                    instance.series = plain(object).map(str::to_owned);
                }
                "extent" if instance.extent.is_none() && labelled(object, "Extent").is_some() => {
                    instance.extent = labelled(object, "Extent");
                }
                "provisionActivity" if !published && publication(object).is_some() => {
                    published = true;
                    if let Some((place, publisher, date)) = publication(object) {
                        instance.place = place;
                        instance.publisher = publisher;
                        instance.date = date;
                    }
                }
                "identifiedBy" if identifier(object).is_some() => {
                    instance.identifiers.extend(identifier(object));
                }
                "tableOfContents" if labelled(object, "TableOfContents").is_some() => {
                    instance.notes.push(NewNote {
                        kind: NoteKind::Contents,
                        text: labelled(object, "TableOfContents").unwrap_or_default(),
                    });
                }
                "note" if column_note(object).is_some() => {
                    instance.notes.extend(column_note(object));
                }
                _ => self.statement(&mut instance.description, name, object),
            }
        }
        instance
    }

    fn statement(&self, description: &mut Description, property: &str, object: &Object) {
        if let Some(value) = self.value(object) {
            description.statements.push(Statement {
                property: property.to_owned(),
                value,
            });
        }
    }

    /// The stored value an object stands for.
    fn value(&self, object: &Object) -> Option<Value> {
        Some(match object {
            Object::Literal { text, language } => Value::Literal {
                text: text.clone(),
                language: language.clone(),
            },
            Object::Iri(iri) => self
                .base
                .entity_of(iri)
                .map(Value::Entity)
                .or_else(|| concept_of(iri))
                .unwrap_or_else(|| Value::Iri(iri.clone())),
            Object::Node(node) => {
                let mut resource = Resource {
                    class: classes(node, "").into_iter().next().unwrap_or_default(),
                    label: label(node),
                    statements: Vec::new(),
                };
                for (name, inner) in bf_properties(node) {
                    resource
                        .statements
                        .extend(self.value(inner).map(|value| Statement {
                            property: name.to_owned(),
                            value,
                        }));
                }
                Value::Resource(resource)
            }
            Object::List(_) => return None,
        })
    }

    fn contribution(&self, object: &Object) -> Option<NewContribution> {
        let node = self.resolve(object)?;
        let primary = has_type(node, "PrimaryContribution");
        let agent = bf_values(node, "agent").find_map(|agent| self.resolve(agent))?;
        let kind = agent_kind(agent)?;
        let heading = label(agent)?;
        let mut roles = bf_values(node, "role");
        let role = roles.next().and_then(|role| match role {
            Object::Iri(iri) => iri
                .strip_prefix("http://id.loc.gov/vocabulary/relators/")
                .map(str::to_owned),
            other => labelled(other, "Role"),
        });
        let role_term = roles.next().and_then(|role| labelled(role, "Role"));
        Some(NewContribution {
            kind,
            name_parts: name_parts(kind, &heading, mads_list(agent, "elementList")),
            heading,
            role,
            role_term,
            primary,
            identifiers: links(agent),
        })
    }

    fn subject(node: &Node) -> Option<NewSubject> {
        let kind = subject_kind(node)?;
        let heading = label(node)?;
        let thesaurus = bf_values(node, "source")
            .find_map(|source| match source {
                Object::Node(source) => bf_values(source, "code").find_map(plain),
                _ => None,
            })
            .map(str::to_owned);
        let components = mads_list(node, "componentList");
        let (subdivisions, name, elements) = match components {
            Some(components) => {
                let kinds: Vec<SubjectKind> = components
                    .iter()
                    .skip(1)
                    .map(|component| component_kind(component).unwrap_or(SubjectKind::Topic))
                    .collect();
                let first = components.first();
                let name = first
                    .and_then(|first| mads_literal(first, "authoritativeLabel"))
                    .unwrap_or_default()
                    .to_owned();
                let elements = first.and_then(|first| mads_list(first, "elementList"));
                let all_topics = kinds.iter().all(|kind| *kind == SubjectKind::Topic);
                (if all_topics { Vec::new() } else { kinds }, name, elements)
            }
            None => (Vec::new(), heading.clone(), mads_list(node, "elementList")),
        };
        let name_parts = kind
            .agent()
            .map(|agent| name_parts(agent, &name, elements))
            .unwrap_or_default();
        Some(NewSubject {
            kind,
            heading,
            thesaurus,
            subdivisions,
            name_parts,
            identifiers: links(node),
        })
    }

    fn item(&self, node: &Node) -> Option<HeldItem> {
        let id = self.id_of(node.iri.as_deref()?, EntityKind::Item)?;
        let unit_iri = bf_values(node, "heldBy").find_map(iri_of)?;
        let branch = self.base.organization_of(unit_iri)?;
        let branch_name = self.named.get(unit_iri).and_then(|unit| label(unit))?;
        let mut item = HeldItem {
            id: ItemId(id),
            barcode: None,
            inventory_number: None,
            branch,
            branch_name,
            policy: String::new(),
            call_number: None,
            sublocation: None,
            collection: None,
            copy_number: None,
            acquired_on: None,
            status: ItemStatus::default(),
            description: Description {
                classes: classes(node, "Item"),
                statements: Vec::new(),
            },
        };
        let mut sublocations = 0;
        for (name, object) in bf_properties(node) {
            match name {
                "itemOf" | "heldBy" => {}
                "identifiedBy" if item.barcode.is_none() && valued(object, "Barcode").is_some() => {
                    item.barcode = valued(object, "Barcode");
                }
                "identifiedBy"
                    if item.inventory_number.is_none() && valued(object, "Local").is_some() =>
                {
                    item.inventory_number = valued(object, "Local");
                }
                "shelfMark" if labelled(object, "ShelfMark").is_some() => {
                    item.call_number = labelled(object, "ShelfMark");
                }
                "sublocation" if sublocations < 2 && labelled(object, "Sublocation").is_some() => {
                    let name = labelled(object, "Sublocation");
                    if sublocations == 0 {
                        item.sublocation = name;
                    } else {
                        item.collection = name;
                    }
                    sublocations += 1;
                }
                "enumerationAndChronology" if labelled(object, "Enumeration").is_some() => {
                    item.copy_number = labelled(object, "Enumeration");
                }
                "immediateAcquisition" => {
                    item.acquired_on = match object {
                        Object::Node(acquisition) => bf_values(acquisition, "date")
                            .find_map(plain)
                            .map(str::to_owned),
                        _ => None,
                    };
                }
                "usageAndAccessPolicy"
                    if item.policy.is_empty() && labelled(object, "UsePolicy").is_some() =>
                {
                    item.policy = labelled(object, "UsePolicy").unwrap_or_default();
                }
                "usageAndAccessPolicy" if coded(object, "AccessPolicy").is_some() => {
                    if let Some(("restricted", number)) = coded(object, "AccessPolicy") {
                        item.status.restricted = number;
                    }
                }
                "status" if coded(object, "Status").is_some() => {
                    if let Some((label, number)) = coded(object, "Status") {
                        match label {
                            "not for loan" => item.status.not_for_loan = number,
                            "damaged" => item.status.damaged = number,
                            "lost" => item.status.lost = number,
                            "withdrawn" => item.status.withdrawn = number,
                            _ => {}
                        }
                    }
                }
                _ => self.statement(&mut item.description, name, object),
            }
        }
        Some(item)
    }
}

/// The BIBFRAME properties of `node` by local name, in order.
fn bf_properties(node: &Node) -> impl Iterator<Item = (&str, &Object)> {
    node.properties
        .iter()
        .filter_map(|(predicate, object)| Some((predicate.strip_prefix(BF)?, object)))
}

fn bf_values<'a>(node: &'a Node, name: &'a str) -> impl Iterator<Item = &'a Object> {
    bf_properties(node)
        .filter(move |(property, _)| *property == name)
        .map(|(_, object)| object)
}

fn has_type(node: &Node, class: &str) -> bool {
    node.types
        .iter()
        .any(|iri| iri.strip_prefix(BF) == Some(class))
}

/// The BIBFRAME classes of `node` but `entity`, in order.
fn classes(node: &Node, entity: &str) -> Vec<String> {
    node.types
        .iter()
        .filter_map(|iri| iri.strip_prefix(BF))
        .filter(|class| *class != entity)
        .map(str::to_owned)
        .collect()
}

fn label(node: &Node) -> Option<String> {
    let rdfs_label = format!("{RDFS}label");
    node.properties
        .iter()
        .find(|(predicate, _)| *predicate == rdfs_label)
        .and_then(|(_, object)| plain(object))
        .map(str::to_owned)
}

fn iri_of(object: &Object) -> Option<&str> {
    match object {
        Object::Iri(iri) => Some(iri),
        _ => None,
    }
}

/// A literal without a language tag.
fn plain(object: &Object) -> Option<&str> {
    match object {
        Object::Literal {
            text,
            language: None,
        } => Some(text),
        _ => None,
    }
}

/// The label of a blank node of `class` that holds only its label.
fn labelled(object: &Object, class: &str) -> Option<String> {
    let Object::Node(node) = object else {
        return None;
    };
    (node.iri.is_none() && is_only(node, class) && node.properties.len() == 1)
        .then(|| label(node))
        .flatten()
}

/// The `rdf:value` of a blank node of `class` that holds only its value.
fn valued(object: &Object, class: &str) -> Option<String> {
    let Object::Node(node) = object else {
        return None;
    };
    let value = format!("{RDF}value");
    match node.properties.as_slice() {
        [(predicate, text)] if *predicate == value && is_only(node, class) => {
            plain(text).map(str::to_owned)
        }
        _ => None,
    }
}

/// The label and `bf:code` number of a blank node of `class`, as the export writes a status.
fn coded<'a>(object: &'a Object, class: &str) -> Option<(&'a str, i64)> {
    let Object::Node(node) = object else {
        return None;
    };
    let rdfs_label = format!("{RDFS}label");
    match node.properties.as_slice() {
        [(first, text), (second, code)]
            if is_only(node, class) && *first == rdfs_label && *second == format!("{BF}code") =>
        {
            Some((plain(text)?, plain(code)?.parse().ok()?))
        }
        _ => None,
    }
}

fn is_only(node: &Node, class: &str) -> bool {
    matches!(node.types.as_slice(), [only] if only.strip_prefix(BF) == Some(class))
}

/// The main title of a `bf:Title` that holds only it.
fn main_title(object: &Object) -> Option<&str> {
    let Object::Node(node) = object else {
        return None;
    };
    match node.properties.as_slice() {
        [(predicate, title)]
            if is_only(node, "Title") && *predicate == format!("{BF}mainTitle") =>
        {
            plain(title)
        }
        _ => None,
    }
}

/// The code of a language of the `languages` list.
fn language_code(object: &Object) -> Option<&str> {
    iri_of(object)?.strip_prefix("http://id.loc.gov/vocabulary/languages/")
}

/// The number and scheme of the classification the export writes from the columns.
fn column_classification(object: &Object) -> Option<(String, Option<String>)> {
    let Object::Node(node) = object else {
        return None;
    };
    let scheme = match node.types.as_slice() {
        [class] => match class.strip_prefix(BF)? {
            "ClassificationDdc" => Some("ddc"),
            "ClassificationLcc" => Some("lcc"),
            "ClassificationUdc" => Some("udc"),
            "ClassificationNlm" => Some("nlm"),
            "Classification" => None,
            _ => return None,
        },
        _ => return None,
    };
    let mut number = None;
    let mut source = None;
    for (name, value) in bf_properties(node) {
        match (name, value) {
            ("classificationPortion", value) if number.is_none() => number = plain(value),
            ("source", Object::Node(found)) if scheme.is_none() && source.is_none() => {
                match found.properties.as_slice() {
                    [(code, value)] if is_only(found, "Source") && *code == format!("{BF}code") => {
                        source = plain(value);
                    }
                    _ => return None,
                }
            }
            _ => return None,
        }
    }
    if node.properties.len() != usize::from(number.is_some()) + usize::from(source.is_some()) {
        return None;
    }
    Some((number?.to_owned(), scheme.or(source).map(str::to_owned)))
}

type Publication = (Option<String>, Option<String>, Option<String>);

/// The place, publisher and date of the publication the export writes from the columns.
fn publication(object: &Object) -> Option<Publication> {
    let Object::Node(node) = object else {
        return None;
    };
    if node.iri.is_some() || !is_only(node, "Publication") || node.properties.is_empty() {
        return None;
    }
    let (mut place, mut publisher, mut date) = (None, None, None);
    for (predicate, value) in &node.properties {
        match predicate.strip_prefix(BF)? {
            "place" if place.is_none() => place = Some(labelled(value, "Place")?),
            "agent" if publisher.is_none() => publisher = Some(labelled(value, "Agent")?),
            "date" if date.is_none() => date = Some(plain(value)?.to_owned()),
            _ => return None,
        }
    }
    Some((place, publisher, date))
}

/// An identifier the export writes from the column of identifiers.
fn identifier(object: &Object) -> Option<NewIdentifier> {
    let Object::Node(node) = object else {
        return None;
    };
    let [class] = node.types.as_slice() else {
        return None;
    };
    let kind = match class.strip_prefix(BF)? {
        "Isbn" => IdentifierKind::Isbn,
        "Issn" => IdentifierKind::Issn,
        "Lccn" => IdentifierKind::Lccn,
        "Ean" => IdentifierKind::Ean,
        "Local" => IdentifierKind::SystemControlNumber,
        "Doi" => IdentifierKind::Doi,
        "Ismn" => IdentifierKind::Ismn,
        _ => return None,
    };
    let value_predicate = format!("{RDF}value");
    let mut value = None;
    let mut qualifier = None;
    for (predicate, object) in &node.properties {
        if *predicate == value_predicate && value.is_none() {
            value = Some(plain(object)?.to_owned());
        } else if predicate.strip_prefix(BF) == Some("qualifier") && qualifier.is_none() {
            qualifier = Some(plain(object)?.to_owned());
        } else {
            return None;
        }
    }
    Some(NewIdentifier {
        kind,
        value: value?,
        qualifier,
    })
}

/// A general or local note the export writes from the column of notes.
fn column_note(object: &Object) -> Option<NewNote> {
    let Object::Node(node) = object else {
        return None;
    };
    if !is_only(node, "Note") {
        return None;
    }
    let text = label(node)?;
    let kind = match node.properties.as_slice() {
        [_] => NoteKind::General,
        [_, (predicate, value)]
            if predicate.strip_prefix(BF) == Some("noteType") && plain(value) == Some("local") =>
        {
            NoteKind::Local
        }
        _ => return None,
    };
    Some(NewNote { kind, text })
}

/// The concept of a Library of Congress vocabulary that `iri` names.
fn concept_of(iri: &str) -> Option<Value> {
    vocabulary::schemes().iter().find_map(|scheme| {
        let code = iri.strip_prefix(scheme.iri)?.strip_prefix('/')?;
        (!code.is_empty() && !code.contains('/')).then(|| Value::Concept {
            scheme: scheme.name.to_owned(),
            code: code.to_owned(),
        })
    })
}

const fn relation_kind(name: &str) -> Option<WorkRelationKind> {
    Some(match name.as_bytes() {
        b"translationOf" => WorkRelationKind::TranslationOf,
        b"derivativeOf" => WorkRelationKind::AdaptationOf,
        b"partOf" => WorkRelationKind::PartOf,
        b"supplementTo" => WorkRelationKind::SupplementTo,
        b"continues" => WorkRelationKind::ContinuationOf,
        _ => return None,
    })
}

fn agent_kind(node: &Node) -> Option<AgentKind> {
    classes(node, "").iter().find_map(|class| {
        Some(match class.as_str() {
            "Person" => AgentKind::Person,
            "Organization" => AgentKind::Organization,
            "Meeting" => AgentKind::Meeting,
            "Family" => AgentKind::Family,
            _ => return None,
        })
    })
}

fn subject_kind(node: &Node) -> Option<SubjectKind> {
    classes(node, "").iter().find_map(|class| {
        Some(match class.as_str() {
            "Topic" => SubjectKind::Topic,
            "Place" => SubjectKind::Place,
            "Person" => SubjectKind::Person,
            "Hub" => SubjectKind::Title,
            "GenreForm" => SubjectKind::GenreForm,
            "Temporal" => SubjectKind::Temporal,
            "Organization" => SubjectKind::Organization,
            "Meeting" => SubjectKind::Meeting,
            "Family" => SubjectKind::Family,
            _ => return None,
        })
    })
}

/// The kind of a subdivision, after its MADS class.
fn component_kind(node: &Node) -> Option<SubjectKind> {
    node.types.iter().find_map(|iri| {
        Some(match iri.strip_prefix(MADSRDF)? {
            "Topic" => SubjectKind::Topic,
            "Geographic" => SubjectKind::Place,
            "Temporal" => SubjectKind::Temporal,
            "GenreForm" => SubjectKind::GenreForm,
            _ => return None,
        })
    })
}

fn mads_list<'a>(node: &'a Node, property: &str) -> Option<&'a [Node]> {
    let predicate = format!("{MADSRDF}{property}");
    node.properties
        .iter()
        .find_map(|(name, object)| match object {
            Object::List(members) if *name == predicate => Some(members.as_slice()),
            _ => None,
        })
}

fn mads_literal<'a>(node: &'a Node, property: &str) -> Option<&'a str> {
    let predicate = format!("{MADSRDF}{property}");
    node.properties
        .iter()
        .find(|(name, _)| *name == predicate)
        .and_then(|(_, object)| plain(object))
}

/// Where each part of `name` after the first starts, from its MADS elements; empty when
/// the elements do not spell the name.
fn name_parts(kind: AgentKind, name: &str, elements: Option<&[Node]>) -> Vec<NamePart> {
    let Some(elements) = elements else {
        return Vec::new();
    };
    let mut parts = Vec::new();
    let mut spelled = String::new();
    for (index, element) in elements.iter().enumerate() {
        let Some(value) = mads_literal(element, "elementValue") else {
            return Vec::new();
        };
        if index > 0 {
            let Some(part) = element
                .types
                .first()
                .and_then(|iri| iri.strip_prefix(MADSRDF))
            else {
                return Vec::new();
            };
            let part = match part {
                "NameElement" if kind == AgentKind::Person => NamePartKind::Numeration,
                "NameElement" => NamePartKind::Subordinate,
                "TermsOfAddressNameElement" => NamePartKind::TermsOfAddress,
                "DateNameElement" => NamePartKind::Date,
                "FullNameElement" => NamePartKind::FullerForm,
                "GeographicElement" => NamePartKind::Place,
                "PartNumberElement" => NamePartKind::Number,
                "TitleElement" => NamePartKind::Title,
                _ => return Vec::new(),
            };
            parts.push(NamePart {
                kind: part,
                start: spelled.chars().count(),
            });
        }
        spelled.push_str(value);
    }
    if spelled == name { parts } else { Vec::new() }
}

/// The links of a name or subject: the authority records, the control numbers, then the
/// thing itself when its node is named.
fn links(node: &Node) -> Vec<AuthorityLink> {
    let authority = format!("{MADSRDF}isIdentifiedByAuthority");
    // The thing's own IRI first, as the imported records hold it ($1 before $0).
    let mut links: Vec<AuthorityLink> = node
        .iri
        .iter()
        .map(|iri| AuthorityLink {
            kind: AuthorityLinkKind::RealWorldObject,
            value: iri.clone(),
        })
        .collect();
    for (predicate, object) in &node.properties {
        if *predicate == authority {
            links.extend(iri_of(object).map(|iri| AuthorityLink {
                kind: AuthorityLinkKind::AuthorityRecord,
                value: iri.to_owned(),
            }));
        } else if predicate.strip_prefix(BF) == Some("identifiedBy") {
            links.extend(identifier_link(object));
        }
    }
    links
}

/// The link a `bf:Identifier` of an agent or subject holds: an IRI value names the thing
/// itself, a literal an authority record.
fn identifier_link(object: &Object) -> Option<AuthorityLink> {
    let Object::Node(node) = object else {
        return None;
    };
    let value = format!("{RDF}value");
    let [(predicate, held)] = node.properties.as_slice() else {
        return None;
    };
    if *predicate != value || !is_only(node, "Identifier") {
        return None;
    }
    match held {
        Object::Iri(iri) => Some(AuthorityLink {
            kind: AuthorityLinkKind::RealWorldObject,
            value: iri.clone(),
        }),
        _ => plain(held).map(|text| AuthorityLink {
            kind: AuthorityLinkKind::AuthorityRecord,
            value: text.to_owned(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rdf::record::tests::{items, record};
    use crate::rdf::{read_turtle, record_graph, write_turtle};

    #[test]
    fn a_written_record_is_read_back_as_it_was() {
        let base = IriBase::new("https://biblioteca.example/bf/");
        let stored = record();
        let text = write_turtle(&record_graph(&stored, &items(), &base));
        let read = records_from(&read_turtle(&text).unwrap(), &base);
        let expected = ReadRecord {
            work: stored.work,
            instance: stored.instance,
            record: stored.record,
            items: items(),
        };
        assert_eq!(read, [expected]);
    }
}
