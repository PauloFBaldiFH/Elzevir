// SPDX-License-Identifier: GPL-3.0-or-later
//! The catalogue as BIBFRAME RDF (docs/specs/catalogo/bibframe-completo.md, item 28): a
//! record becomes a few named resources, each a tree of blank nodes, written as Turtle.
//!
//! The trees are built from the model alone, never from the stored MARC record: what the
//! RDF says is what the catalogue holds.

mod record;
mod turtle;

pub use record::record_graph;
pub use turtle::write_turtle;

use elzevir_domain::statement::{EntityKind, EntityRef};

/// The BIBFRAME namespace.
pub const BF: &str = elzevir_data::bibframe::NAMESPACE;
/// The RDF namespace.
pub const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
/// The RDF Schema namespace.
pub const RDFS: &str = "http://www.w3.org/2000/01/rdf-schema#";
/// The MADS/RDF namespace, for links to authority records.
pub const MADSRDF: &str = "http://www.loc.gov/mads/rdf/v1#";

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
}

impl Node {
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
}
