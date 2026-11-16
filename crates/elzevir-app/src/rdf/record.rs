// SPDX-License-Identifier: GPL-3.0-or-later
//! A stored record as its Work and Instance (bibframe-completo.md, item 28a): the typed
//! columns as the Library of Congress MARC 21 to BIBFRAME conversion writes them, then the
//! classes and statements as they are stored.

use elzevir_data::vocabulary;
use elzevir_domain::catalog::{
    AgentKind, AuthorityLinkKind, IdentifierKind, NoteKind, SubjectKind, WorkRelationKind,
};
use elzevir_domain::statement::{Description, EntityKind, EntityRef, Statement, Value};

use super::items::{HeldItem, item_node};
use super::mads::{describe_subject, name_elements};
use super::{BF, IriBase, MADSRDF, Node, Object, RDF};
use crate::catalog::{AuthorityLink, NewContribution, NewInstance, NewRecord, NewSubject};
use crate::export::StoredRecord;

/// The Work, the Instance and the Items of `stored`, in that order. The Items are `items`,
/// read by name (item 28b), not the edge-coded ones of `stored`.
#[must_use]
pub fn record_graph(stored: &StoredRecord, items: &[HeldItem], base: &IriBase) -> Vec<Node> {
    let work_iri = base.entity(entity(EntityKind::Work, stored.work.0));
    let instance_iri = base.entity(entity(EntityKind::Instance, stored.instance.0));
    let mut work = work_node(&stored.record, base);
    work.iri = Some(work_iri.clone());
    work.add("hasInstance", Object::Iri(instance_iri.clone()));
    let mut instance = instance_node(&stored.record.instance, base);
    instance.add("instanceOf", Object::Iri(work_iri));
    let items = items
        .iter()
        .map(|item| item_node(item, &instance_iri, base))
        .collect::<Vec<_>>();
    for item in &items {
        if let Some(iri) = &item.iri {
            instance.add("hasItem", Object::Iri(iri.clone()));
        }
    }
    instance.iri = Some(instance_iri);
    let mut nodes = vec![work, instance];
    nodes.extend(items);
    nodes
}

const fn entity(entity: EntityKind, id: i64) -> EntityRef {
    EntityRef { entity, id }
}

fn work_node(record: &NewRecord, base: &IriBase) -> Node {
    let mut work = described("Work", &record.work_description);
    work.node("title", title(&record.work_title));
    if let Some(hub) = record.hub {
        work.add(
            "expressionOf",
            Object::Iri(base.entity(entity(EntityKind::Hub, hub.0))),
        );
    }
    for contribution in &record.contributions {
        work.node("contribution", contribution_node(contribution));
    }
    for subject in &record.subjects {
        work.node("subject", subject_node(subject));
    }
    for language in &record.languages {
        work.add("language", concept("languages", language));
    }
    if let Some(number) = &record.classification {
        work.node(
            "classification",
            classification(number, record.classification_scheme.as_deref()),
        );
    }
    if let Some(summary) = &record.summary {
        work.node("summary", Node::labelled("Summary", summary));
    }
    if let Some(audience) = &record.audience {
        work.node(
            "intendedAudience",
            Node::labelled("IntendedAudience", audience),
        );
    }
    for relation in &record.relations {
        work.add(
            relation_property(relation.kind),
            Object::Iri(base.entity(entity(EntityKind::Work, relation.work.0))),
        );
    }
    statements(&mut work, &record.work_description.statements, base);
    work
}

fn instance_node(data: &NewInstance, base: &IriBase) -> Node {
    let mut instance = described("Instance", &data.description);
    instance.node("title", title(&data.title));
    if let Some(edition) = &data.edition {
        instance.text("editionStatement", edition);
    }
    if data.place.is_some() || data.publisher.is_some() || data.date.is_some() {
        let mut publication = Node::blank("Publication");
        if let Some(place) = &data.place {
            publication.node("place", Node::labelled("Place", place));
        }
        if let Some(publisher) = &data.publisher {
            publication.node("agent", Node::labelled("Agent", publisher));
        }
        if let Some(date) = &data.date {
            publication.text("date", date);
        }
        instance.node("provisionActivity", publication);
    }
    if let Some(extent) = &data.extent {
        instance.node("extent", Node::labelled("Extent", extent));
    }
    if let Some(series) = &data.series {
        instance.text("seriesStatement", series);
    }
    for identifier in &data.identifiers {
        let mut node = Node::blank(identifier_class(identifier.kind));
        node.value(&identifier.value);
        if let Some(qualifier) = &identifier.qualifier {
            node.text("qualifier", qualifier);
        }
        instance.node("identifiedBy", node);
    }
    for note in &data.notes {
        match note.kind {
            NoteKind::Contents => {
                instance.node(
                    "tableOfContents",
                    Node::labelled("TableOfContents", &note.text),
                );
            }
            NoteKind::General => instance.node("note", Node::labelled("Note", &note.text)),
            NoteKind::Local => {
                let mut local = Node::labelled("Note", &note.text);
                local.text("noteType", "local");
                instance.node("note", local);
            }
        }
    }
    statements(&mut instance, &data.description.statements, base);
    instance
}

/// A node of `entity` and of each of its classes, without statements yet.
fn described(entity: &str, description: &Description) -> Node {
    let mut node = Node::blank(entity);
    node.types.extend(
        description
            .classes
            .iter()
            .map(|class| format!("{BF}{class}")),
    );
    node
}

fn title(main: &str) -> Node {
    let mut title = Node::blank("Title");
    title.text("mainTitle", main);
    title
}

fn contribution_node(contribution: &NewContribution) -> Node {
    let mut node = Node::blank(if contribution.primary {
        "PrimaryContribution"
    } else {
        "Contribution"
    });
    let mut agent = linked(
        agent_class(contribution.kind),
        &contribution.heading,
        &contribution.identifiers,
    );
    if let Some(elements) = name_elements(
        contribution.kind,
        &contribution.heading,
        &contribution.name_parts,
    ) {
        agent.add_mads("elementList", elements);
    }
    node.add("agent", Object::Node(Box::new(agent)));
    if let Some(role) = &contribution.role {
        node.add("role", role_object(role));
    }
    if let Some(term) = &contribution.role_term {
        node.node("role", Node::labelled("Role", term));
    }
    node
}

/// A relator code is a concept of the `relators` list; any other role is a term.
fn role_object(role: &str) -> Object {
    vocabulary::scheme("relators")
        .filter(|scheme| scheme.concept(role).is_some())
        .map_or_else(
            || Object::Node(Box::new(Node::labelled("Role", role))),
            |scheme| Object::Iri(scheme.concept_iri(role)),
        )
}

fn subject_node(subject: &NewSubject) -> Node {
    let mut node = linked(
        subject_class(subject.kind),
        &subject.heading,
        &subject.identifiers,
    );
    describe_subject(subject, &mut node);
    if let Some(thesaurus) = &subject.thesaurus {
        let mut source = Node::blank("Source");
        source.text("code", thesaurus);
        node.node("source", source);
    }
    node
}

/// A named entity with its links: the first link to the thing itself names the node
/// (MARC21 `$1`), a link to an authority record is `madsrdf:isIdentifiedByAuthority`
/// (`$0` as an IRI), and a control number is an identifier (`$0` as a number).
fn linked(class: &str, label: &str, links: &[AuthorityLink]) -> Node {
    let mut node = Node::labelled(class, label);
    for link in links {
        let is_iri = link.value.starts_with("http://") || link.value.starts_with("https://");
        match link.kind {
            AuthorityLinkKind::RealWorldObject if is_iri && node.iri.is_none() => {
                node.iri = Some(link.value.clone());
            }
            AuthorityLinkKind::AuthorityRecord if is_iri => node.properties.push((
                format!("{MADSRDF}isIdentifiedByAuthority"),
                Object::Iri(link.value.clone()),
            )),
            // Another IRI of the thing itself is the value of an identifier as an IRI,
            // so it reads back as a link to the thing and not to an authority record.
            AuthorityLinkKind::RealWorldObject if is_iri => {
                let mut identifier = Node::blank("Identifier");
                identifier
                    .properties
                    .push((format!("{RDF}value"), Object::Iri(link.value.clone())));
                node.node("identifiedBy", identifier);
            }
            AuthorityLinkKind::AuthorityRecord | AuthorityLinkKind::RealWorldObject => {
                let mut identifier = Node::blank("Identifier");
                identifier.value(&link.value);
                node.node("identifiedBy", identifier);
            }
        }
    }
    node
}

fn classification(number: &str, scheme: Option<&str>) -> Node {
    let class = match scheme {
        Some("ddc") => "ClassificationDdc",
        Some("lcc") => "ClassificationLcc",
        Some("udc") => "ClassificationUdc",
        Some("nlm") => "ClassificationNlm",
        _ => "Classification",
    };
    let mut node = Node::blank(class);
    node.text("classificationPortion", number);
    if let (Some(code), "Classification") = (scheme, class) {
        let mut source = Node::blank("Source");
        source.text("code", code);
        node.node("source", source);
    }
    node
}

/// The concept `code` of `scheme`, by its IRI.
fn concept(scheme: &str, code: &str) -> Object {
    Object::Iri(vocabulary::scheme(scheme).map_or_else(
        || format!("http://id.loc.gov/vocabulary/{scheme}/{code}"),
        |scheme| scheme.concept_iri(code),
    ))
}

/// The stored statements of a resource, after what its columns gave.
pub(super) fn statements(node: &mut Node, statements: &[Statement], base: &IriBase) {
    for statement in statements {
        let object = value_object(&statement.value, base);
        node.add(&statement.property, object);
    }
}

fn value_object(value: &Value, base: &IriBase) -> Object {
    match value {
        Value::Literal { text, language } => Object::Literal {
            text: text.clone(),
            language: language.clone(),
        },
        Value::Concept { scheme, code } => concept(scheme, code),
        Value::Entity(reference) => Object::Iri(base.entity(*reference)),
        Value::Iri(iri) => Object::Iri(iri.clone()),
        Value::Resource(resource) => {
            let mut node = Node::blank(&resource.class);
            if let Some(label) = &resource.label {
                node.label(label);
            }
            statements(&mut node, &resource.statements, base);
            Object::Node(Box::new(node))
        }
    }
}

const fn agent_class(kind: AgentKind) -> &'static str {
    match kind {
        AgentKind::Person => "Person",
        AgentKind::Organization => "Organization",
        AgentKind::Meeting => "Meeting",
        AgentKind::Family => "Family",
    }
}

const fn subject_class(kind: SubjectKind) -> &'static str {
    match kind {
        SubjectKind::Topic => "Topic",
        SubjectKind::Place => "Place",
        SubjectKind::Person => "Person",
        SubjectKind::Title => "Hub",
        SubjectKind::GenreForm => "GenreForm",
        SubjectKind::Temporal => "Temporal",
        SubjectKind::Organization => "Organization",
        SubjectKind::Meeting => "Meeting",
        SubjectKind::Family => "Family",
    }
}

const fn identifier_class(kind: IdentifierKind) -> &'static str {
    match kind {
        IdentifierKind::Isbn => "Isbn",
        IdentifierKind::Issn => "Issn",
        IdentifierKind::Lccn => "Lccn",
        IdentifierKind::Ean => "Ean",
        IdentifierKind::SystemControlNumber => "Local",
        IdentifierKind::Doi => "Doi",
        IdentifierKind::Ismn => "Ismn",
    }
}

const fn relation_property(kind: WorkRelationKind) -> &'static str {
    match kind {
        WorkRelationKind::TranslationOf => "translationOf",
        WorkRelationKind::AdaptationOf => "derivativeOf",
        WorkRelationKind::PartOf => "partOf",
        WorkRelationKind::SupplementTo => "supplementTo",
        WorkRelationKind::ContinuationOf => "continues",
    }
}

/// The `bf:` classes and properties of `nodes` that the ontology does not have, in the
/// order met; empty when the graph uses only the ontology's terms.
#[must_use]
pub fn foreign_terms(nodes: &[Node]) -> Vec<String> {
    fn walk(node: &Node, found: &mut Vec<String>) {
        let local = |iri: &str| iri.strip_prefix(BF).map(str::to_owned);
        for class in node.types.iter().filter_map(|iri| local(iri)) {
            if elzevir_data::bibframe::class(&class).is_none() {
                found.push(class);
            }
        }
        for (predicate, object) in &node.properties {
            if let Some(name) = local(predicate)
                && elzevir_data::bibframe::property(&name).is_none()
            {
                found.push(name);
            }
            for inner in object.nodes() {
                walk(inner, found);
            }
        }
    }
    let mut found = Vec::new();
    for node in nodes {
        walk(node, &mut found);
    }
    found
}

#[cfg(test)]
pub(super) mod tests {
    use elzevir_domain::catalog::{BranchId, HubId, InstanceId, ItemId, WorkId};
    use elzevir_domain::statement::Resource;

    use super::*;
    use crate::catalog::{ItemStatus, NamePart, NewIdentifier, NewNote, NewWorkRelation};
    use crate::description::printed_book;
    use crate::rdf::write_turtle;
    use elzevir_domain::catalog::NamePartKind;

    fn contributions() -> Vec<NewContribution> {
        let link = |kind, value: &str| AuthorityLink {
            kind,
            value: value.to_owned(),
        };
        vec![
            NewContribution {
                kind: AgentKind::Person,
                heading: "Assis, Machado de, 1839-1908".to_owned(),
                role: Some("aut".to_owned()),
                role_term: Some("autor".to_owned()),
                primary: true,
                name_parts: vec![NamePart {
                    kind: NamePartKind::Date,
                    start: 19,
                }],
                identifiers: vec![
                    link(
                        AuthorityLinkKind::RealWorldObject,
                        "http://viaf.org/viaf/2467372",
                    ),
                    link(
                        AuthorityLinkKind::RealWorldObject,
                        "http://www.wikidata.org/entity/Q311145",
                    ),
                    link(
                        AuthorityLinkKind::AuthorityRecord,
                        "http://id.loc.gov/authorities/names/n79029745",
                    ),
                ],
            },
            NewContribution {
                kind: AgentKind::Organization,
                heading: "Editora \"Garnier\"".to_owned(),
                role: Some("ilustrador".to_owned()),
                role_term: None,
                primary: false,
                name_parts: Vec::new(),
                identifiers: vec![link(AuthorityLinkKind::AuthorityRecord, "(BR)123")],
            },
        ]
    }

    pub(in crate::rdf) fn items() -> Vec<HeldItem> {
        let shelved = HeldItem {
            id: ItemId(11),
            barcode: Some("000123".to_owned()),
            inventory_number: Some("T1".to_owned()),
            branch: BranchId(1),
            branch_name: "Biblioteca Central".to_owned(),
            policy: "Empréstimo normal".to_owned(),
            call_number: Some("869.3 A848d".to_owned()),
            sublocation: Some("Estantes gerais".to_owned()),
            collection: Some("Literatura".to_owned()),
            copy_number: Some("ex. 1".to_owned()),
            acquired_on: Some("2020-03-01".to_owned()),
            status: ItemStatus::default(),
            description: Description::default(),
        };
        let lost = HeldItem {
            id: ItemId(12),
            barcode: None,
            inventory_number: None,
            call_number: None,
            sublocation: None,
            collection: None,
            copy_number: None,
            acquired_on: None,
            status: ItemStatus {
                lost: 1,
                restricted: 2,
                ..ItemStatus::default()
            },
            ..shelved.clone()
        };
        vec![shelved, lost]
    }

    pub(in crate::rdf) fn record() -> StoredRecord {
        let (mut work, mut instance) = printed_book();
        work.statements.push(Statement {
            property: "genreForm".to_owned(),
            value: Value::Resource(Resource {
                class: "GenreForm".to_owned(),
                label: Some("Romances".to_owned()),
                statements: Vec::new(),
            }),
        });
        work.statements.push(Statement {
            property: "genreForm".to_owned(),
            value: Value::Iri("http://id.loc.gov/authorities/genreForms/gf2015026020".to_owned()),
        });
        instance.statements.push(Statement {
            property: "dimensions".to_owned(),
            value: Value::Literal {
                text: "21 cm".to_owned(),
                language: None,
            },
        });
        StoredRecord {
            instance: InstanceId(7),
            work: WorkId(3),
            record: NewRecord {
                work_title: "Dom Casmurro".to_owned(),
                work_description: work,
                hub: Some(HubId(2)),
                relations: vec![NewWorkRelation {
                    kind: WorkRelationKind::AdaptationOf,
                    work: WorkId(1),
                }],
                classification: Some("869.3".to_owned()),
                classification_scheme: Some("ddc".to_owned()),
                summary: Some("Bentinho recorda Capitu.".to_owned()),
                audience: Some("Ensino médio.".to_owned()),
                languages: vec!["por".to_owned()],
                contributions: contributions(),
                subjects: vec![NewSubject {
                    kind: SubjectKind::Topic,
                    heading: "Ciúme -- Ficção".to_owned(),
                    thesaurus: Some("larpcal".to_owned()),
                    subdivisions: vec![SubjectKind::GenreForm],
                    name_parts: Vec::new(),
                    identifiers: Vec::new(),
                }],
                instance: NewInstance {
                    title: "Dom Casmurro".to_owned(),
                    description: instance,
                    edition: Some("2. ed.".to_owned()),
                    place: Some("Rio de Janeiro".to_owned()),
                    publisher: Some("Garnier".to_owned()),
                    date: Some("1899".to_owned()),
                    extent: Some("195 p.".to_owned()),
                    series: Some("Clássicos".to_owned()),
                    identifiers: vec![NewIdentifier {
                        kind: IdentifierKind::Isbn,
                        value: "9788535910667".to_owned(),
                        qualifier: Some("(broch.)".to_owned()),
                    }],
                    notes: vec![
                        NewNote {
                            kind: NoteKind::Contents,
                            text: "Cap. 1 -- Cap. 2.".to_owned(),
                        },
                        NewNote {
                            kind: NoteKind::Local,
                            text: "Doação.".to_owned(),
                        },
                    ],
                },
                items: Vec::new(),
                source: None,
            },
        }
    }

    #[test]
    fn a_record_uses_only_the_ontology_s_classes_and_properties() {
        let graph = record_graph(
            &record(),
            &items(),
            &IriBase::new("https://biblioteca.example/bf/"),
        );
        assert_eq!(foreign_terms(&graph), Vec::<String>::new());
    }

    #[test]
    fn a_record_is_written_as_turtle() {
        let graph = record_graph(
            &record(),
            &items(),
            &IriBase::new("https://biblioteca.example/bf/"),
        );
        insta::assert_snapshot!(write_turtle(&graph));
    }

    #[test]
    fn a_record_is_written_as_rdf_xml() {
        let graph = record_graph(
            &record(),
            &items(),
            &IriBase::new("https://biblioteca.example/bf/"),
        );
        insta::assert_snapshot!(crate::rdf::write_rdf_xml(&graph));
    }
}
