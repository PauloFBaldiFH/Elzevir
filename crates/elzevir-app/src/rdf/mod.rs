// SPDX-License-Identifier: GPL-3.0-or-later
//! The catalogue as BIBFRAME RDF (docs/specs/catalogo/bibframe-completo.md, item 28): a
//! record becomes a few named resources, each a tree of blank nodes, written as Turtle.
//!
//! The trees are built from the model alone, never from the stored MARC record: what the
//! RDF says is what the catalogue holds.

mod items;
mod mads;
mod parse;
mod read;
mod record;
mod turtle;
mod xml;

pub use items::HeldItem;
pub use parse::{RdfReadError, read_turtle};
pub use read::{ReadRecord, records_from};
pub use record::{foreign_terms, record_graph};
pub use turtle::write_turtle;
pub use xml::write_rdf_xml;

use std::collections::{BTreeSet, VecDeque};
use std::io::Write;
use std::num::NonZeroUsize;

use elzevir_domain::catalog::{BranchId, InstanceId};

use crate::catalog::CatalogReadError;
use crate::export::{ExportError, RecordSource, stored_record};
use elzevir_domain::statement::{EntityKind, EntityRef};

/// The BIBFRAME namespace.
pub const BF: &str = elzevir_data::bibframe::NAMESPACE;
/// The RDF namespace.
pub const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
/// The RDF Schema namespace.
pub const RDFS: &str = "http://www.w3.org/2000/01/rdf-schema#";
/// The MADS/RDF namespace, for links to authority records.
pub const MADSRDF: &str = "http://www.loc.gov/mads/rdf/v1#";

/// The syntax of an RDF export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RdfFormat {
    /// Turtle.
    Turtle,
    /// RDF/XML.
    RdfXml,
}

/// One stored record, its Work, Instance and Items, in `format`; `None` when the Instance
/// does not exist.
///
/// # Errors
///
/// When the catalog cannot be read.
pub fn export_rdf<S: RecordSource + ?Sized>(
    source: &S,
    instance: InstanceId,
    format: RdfFormat,
    base: &IriBase,
) -> Result<Option<String>, CatalogReadError> {
    let Some(stored) = stored_record(source, instance)? else {
        return Ok(None);
    };
    let items = source.held_items(instance)?;
    let graph = record_graph(&stored, &items, base);
    Ok(Some(match format {
        RdfFormat::Turtle => write_turtle(&graph),
        RdfFormat::RdfXml => write_rdf_xml(&graph),
    }))
}

impl RdfFormat {
    fn header(self) -> String {
        match self {
            Self::Turtle => turtle::turtle_header(),
            Self::RdfXml => xml::rdf_xml_header(),
        }
    }

    fn blocks(self, nodes: &[Node]) -> String {
        match self {
            Self::Turtle => turtle::turtle_blocks(nodes),
            Self::RdfXml => xml::rdf_xml_blocks(nodes),
        }
    }

    const fn footer(self) -> &'static str {
        match self {
            Self::Turtle => "",
            Self::RdfXml => xml::RDF_XML_FOOTER,
        }
    }
}

/// Writes every stored record, its Work, Instance and Items, as one document in `format`,
/// in Instance order, reading `batch` records at a time; returns how many were written.
///
/// # Errors
///
/// When the catalog cannot be read or the output cannot be written.
pub fn export_rdf_catalog<W: Write, S: RecordSource + ?Sized>(
    source: &S,
    out: &mut W,
    format: RdfFormat,
    base: &IriBase,
    batch: NonZeroUsize,
) -> Result<usize, ExportError> {
    out.write_all(format.header().as_bytes())?;
    let mut written = 0_usize;
    let mut after = None;
    loop {
        let records = source.records_after(after, batch)?;
        let Some(last) = records.last() else {
            break;
        };
        after = Some(last.instance);
        for stored in &records {
            let items = source.held_items(stored.instance)?;
            let graph = record_graph(stored, &items, base);
            out.write_all(format.blocks(&graph).as_bytes())?;
            written = written.saturating_add(1);
        }
    }
    out.write_all(format.footer().as_bytes())?;
    out.flush()?;
    Ok(written)
}

/// The prefixes every document declares, in order.
const PREFIXES: [(&str, &str); 4] = [
    ("bf", BF),
    ("rdf", RDF),
    ("rdfs", RDFS),
    ("madsrdf", MADSRDF),
];

/// The resources written as blocks of their own: `nodes`, then each named resource met as
/// a value, once per IRI (a unit holding many Items is described once).
fn blocks(nodes: &[Node]) -> Vec<&Node> {
    fn named<'a>(node: &'a Node, queue: &mut VecDeque<&'a Node>) {
        for inner in node
            .properties
            .iter()
            .flat_map(|(_, object)| object.nodes())
        {
            if inner.iri.is_some() {
                queue.push_back(inner);
            } else {
                named(inner, queue);
            }
        }
    }
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    let mut queue: VecDeque<&Node> = nodes.iter().collect();
    while let Some(node) = queue.pop_front() {
        if node.iri.as_deref().is_some_and(|iri| !seen.insert(iri)) {
            continue;
        }
        out.push(node);
        named(node, &mut queue);
    }
    out
}

/// A resource: named by an IRI, or a blank node written in place.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Node {
    /// Its IRI; `None` for a blank node.
    pub iri: Option<String>,
    /// The IRIs of its classes, in order.
    pub types: Vec<String>,
    /// Its properties: the predicate's IRI and the value, in order.
    pub properties: Vec<(String, Object)>,
}

/// The value of a property.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Object {
    /// Another resource, by its IRI.
    Iri(String),
    /// Text, with its language tag.
    Literal {
        /// The text.
        text: String,
        /// Its language tag, such as `pt`.
        language: Option<String>,
    },
    /// A resource described here: a blank node, or a named one whose description follows.
    Node(Box<Node>),
    /// An ordered list of resources (`rdf:List`), such as the elements of a MADS name.
    List(Vec<Node>),
}

impl Object {
    /// The resources described in this value: the node, or the list's members.
    #[must_use]
    pub fn nodes(&self) -> &[Node] {
        match self {
            Self::Node(node) => std::slice::from_ref(node),
            Self::List(nodes) => nodes,
            Self::Iri(_) | Self::Literal { .. } => &[],
        }
    }
}

impl Node {
    /// A blank node of the MADS/RDF class `class`.
    #[must_use]
    pub fn mads(class: &str) -> Self {
        Self {
            iri: None,
            types: vec![format!("{MADSRDF}{class}")],
            properties: Vec::new(),
        }
    }

    /// Adds the MADS/RDF property `property` with `object`.
    pub fn add_mads(&mut self, property: &str, object: Object) {
        self.properties
            .push((format!("{MADSRDF}{property}"), object));
    }

    /// A blank node of the BIBFRAME class `class`.
    #[must_use]
    pub fn blank(class: &str) -> Self {
        Self {
            iri: None,
            types: vec![format!("{BF}{class}")],
            properties: Vec::new(),
        }
    }

    /// Adds the BIBFRAME property `property` with `object`.
    pub fn add(&mut self, property: &str, object: Object) {
        self.properties.push((format!("{BF}{property}"), object));
    }

    /// Adds `property` with a plain literal.
    pub fn text(&mut self, property: &str, text: &str) {
        self.add(property, literal(text));
    }

    /// Adds `property` with a node.
    pub fn node(&mut self, property: &str, node: Self) {
        self.add(property, Object::Node(Box::new(node)));
    }

    /// Adds `rdfs:label`.
    pub fn label(&mut self, text: &str) {
        self.properties
            .push((format!("{RDFS}label"), literal(text)));
    }

    /// Adds `rdf:value`.
    pub fn value(&mut self, text: &str) {
        self.properties.push((format!("{RDF}value"), literal(text)));
    }

    /// A blank node of `class` with only a label.
    #[must_use]
    pub fn labelled(class: &str, text: &str) -> Self {
        let mut node = Self::blank(class);
        node.label(text);
        node
    }
}

/// A plain literal.
#[must_use]
pub fn literal(text: &str) -> Object {
    Object::Literal {
        text: text.to_owned(),
        language: None,
    }
}

/// Where the IRIs of the catalogue's own resources start, such as
/// `https://biblioteca.example/bibframe`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IriBase(String);

impl IriBase {
    /// The base `base`, without a trailing slash.
    #[must_use]
    pub fn new(base: &str) -> Self {
        Self(base.trim_end_matches('/').to_owned())
    }

    /// The IRI of a Hub, Work, Instance or Item.
    #[must_use]
    pub fn entity(&self, entity: EntityRef) -> String {
        let kind = match entity.entity {
            EntityKind::Hub => "hubs",
            EntityKind::Work => "works",
            EntityKind::Instance => "instances",
            EntityKind::Item => "items",
        };
        format!("{}/{kind}/{}", self.0, entity.id)
    }

    /// The Hub, Work, Instance or Item `iri` names, when it is one of the catalogue's.
    #[must_use]
    pub fn entity_of(&self, iri: &str) -> Option<EntityRef> {
        let (kind, id) = iri
            .strip_prefix(&self.0)?
            .strip_prefix('/')?
            .split_once('/')?;
        let entity = match kind {
            "hubs" => EntityKind::Hub,
            "works" => EntityKind::Work,
            "instances" => EntityKind::Instance,
            "items" => EntityKind::Item,
            _ => return None,
        };
        let id = id.parse().ok()?;
        (self.entity(EntityRef { entity, id }) == iri).then_some(EntityRef { entity, id })
    }

    /// The unit `iri` names, when it is one of the catalogue's.
    #[must_use]
    pub fn organization_of(&self, iri: &str) -> Option<BranchId> {
        let id = iri
            .strip_prefix(&self.0)?
            .strip_prefix("/organizations/")?
            .parse()
            .ok()?;
        (self.organization(BranchId(id)) == iri).then_some(BranchId(id))
    }

    /// The IRI of a unit of the library, a `bf:Organization`.
    #[must_use]
    pub fn organization(&self, branch: BranchId) -> String {
        format!("{}/organizations/{}", self.0, branch.0)
    }
}
