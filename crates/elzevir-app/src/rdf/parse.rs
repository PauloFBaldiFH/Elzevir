// SPDX-License-Identifier: GPL-3.0-or-later
//! Turtle back into resources (bibframe-completo.md, item 28d): the triples, read by a
//! W3C-conformant parser, regrouped as the trees the export writes.

use std::collections::{BTreeMap, BTreeSet};

use oxrdf::{NamedOrBlankNode, Term};
use oxttl::TurtleParser;

use super::{Node, Object, RDF};

/// Most nesting of blank nodes read; the export nests a few levels.
const MAX_DEPTH: usize = 32;
/// Most members of one list read; a name has a few elements.
const MAX_LIST: usize = 1_000;

/// Why a document could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RdfReadError {
    /// The document is not valid Turtle.
    #[error("invalid Turtle: {0}")]
    Syntax(String),
    /// Blank nodes or lists go deeper than any export writes, or do not form trees.
    #[error("blank nodes nested too deep or shared")]
    TooDeep,
}

/// A resource's key: `<iri>` or `_:id`.
type Key = String;

/// The triples of one subject, in document order.
#[derive(Default)]
struct Described {
    types: Vec<String>,
    properties: Vec<(String, Term)>,
}

/// The resources of a Turtle document: each named resource with its blank nodes nested in
/// place and its lists as [`Object::List`], in the order they first appear. A named
/// resource met as a value is an [`Object::Iri`]; its description is its own node.
///
/// # Errors
///
/// [`RdfReadError`] when the document is not Turtle or its blank nodes do not form trees.
pub fn read_turtle(text: &str) -> Result<Vec<Node>, RdfReadError> {
    let mut subjects: BTreeMap<Key, Described> = BTreeMap::new();
    let mut order: Vec<Key> = Vec::new();
    let mut objects: BTreeSet<Key> = BTreeSet::new();
    for triple in TurtleParser::new().for_slice(text.as_bytes()) {
        let triple = triple.map_err(|e| RdfReadError::Syntax(e.to_string()))?;
        let subject = match &triple.subject {
            NamedOrBlankNode::NamedNode(node) => format!("<{}>", node.as_str()),
            NamedOrBlankNode::BlankNode(node) => format!("_:{}", node.as_str()),
        };
        // A blank node written in place is the value of one property only; one shared by
        // many would be read once for each, and a chain of them grows without bound.
        if let Term::BlankNode(node) = &triple.object
            && !objects.insert(format!("_:{}", node.as_str()))
        {
            return Err(RdfReadError::TooDeep);
        }
        let described = subjects.entry(subject.clone()).or_insert_with(|| {
            order.push(subject);
            Described::default()
        });
        let predicate = triple.predicate.as_str();
        match &triple.object {
            Term::NamedNode(class) if predicate == RDF_TYPE => {
                described.types.push(class.as_str().to_owned());
            }
            object => described
                .properties
                .push((predicate.to_owned(), object.clone())),
        }
    }
    let graph = Graph { subjects };
    order
        .iter()
        .filter(|key| !objects.contains(*key))
        .map(|key| graph.node(key, 0))
        .collect()
}

const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";

struct Graph {
    subjects: BTreeMap<Key, Described>,
}

impl Graph {
    fn node(&self, key: &str, depth: usize) -> Result<Node, RdfReadError> {
        if depth > MAX_DEPTH {
            return Err(RdfReadError::TooDeep);
        }
        let mut node = Node {
            iri: key
                .strip_prefix('<')
                .and_then(|iri| iri.strip_suffix('>'))
                .map(str::to_owned),
            ..Node::default()
        };
        let Some(described) = self.subjects.get(key) else {
            return Ok(node);
        };
        node.types.clone_from(&described.types);
        for (predicate, object) in &described.properties {
            let value = match object {
                Term::NamedNode(iri) => Object::Iri(iri.as_str().to_owned()),
                Term::Literal(literal) => Object::Literal {
                    text: literal.value().to_owned(),
                    language: literal.language().map(str::to_owned),
                },
                Term::BlankNode(blank) => {
                    let key = format!("_:{}", blank.as_str());
                    match self.list(&key, depth + 1)? {
                        Some(members) => Object::List(members),
                        None => Object::Node(Box::new(self.node(&key, depth + 1)?)),
                    }
                }
            };
            node.properties.push((predicate.clone(), value));
        }
        Ok(node)
    }

    /// The members of the `rdf:List` that starts at `key`, or `None` when it is no list.
    fn list(&self, key: &str, depth: usize) -> Result<Option<Vec<Node>>, RdfReadError> {
        let first = format!("{RDF}first");
        let rest = format!("{RDF}rest");
        let nil = format!("{RDF}nil");
        let mut members = Vec::new();
        let mut at = key.to_owned();
        loop {
            if members.len() > MAX_LIST {
                return Err(RdfReadError::TooDeep);
            }
            let Some(cell) = self.subjects.get(&at) else {
                return Ok(None);
            };
            let value = |name: &str| {
                cell.properties
                    .iter()
                    .find(|(predicate, _)| predicate == name)
                    .map(|(_, object)| object)
            };
            let (Some(member), Some(next)) = (value(&first), value(&rest)) else {
                return Ok(None);
            };
            members.push(match member {
                Term::BlankNode(blank) => self.node(&format!("_:{}", blank.as_str()), depth + 1)?,
                Term::NamedNode(iri) => Node {
                    iri: Some(iri.as_str().to_owned()),
                    ..Node::default()
                },
                Term::Literal(_) => return Ok(None),
            });
            match next {
                Term::NamedNode(end) if end.as_str() == nil => return Ok(Some(members)),
                Term::BlankNode(blank) => at = format!("_:{}", blank.as_str()),
                _ => return Ok(None),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rdf::write_turtle;

    #[test]
    fn what_the_writer_writes_is_read_as_the_same_trees() {
        let mut agent = Node::labelled("Person", "Assis, Machado de");
        agent.iri = Some("http://viaf.example/1".to_owned());
        let mut name = Node::mads("FullNameElement");
        name.add_mads("elementValue", crate::rdf::literal("Assis"));
        let mut work = Node::blank("Work");
        work.iri = Some("http://elzevir.example/works/1".to_owned());
        work.add("agent", Object::Node(Box::new(agent.clone())));
        work.node("note", Node::labelled("Note", "Nota \"citada\"\n"));
        work.add(
            "summary",
            Object::Literal {
                text: "Resumo".to_owned(),
                language: Some("pt".to_owned()),
            },
        );
        work.add_mads("elementList", Object::List(vec![name]));
        let read = read_turtle(&write_turtle(std::slice::from_ref(&work))).unwrap();
        let mut expected = work;
        expected.properties[0].1 = Object::Iri("http://viaf.example/1".to_owned());
        assert_eq!(read, [expected, agent]);
        assert!(read_turtle("<a> <b> .").is_err());
        assert_eq!(read_turtle("").unwrap(), []);
    }

    #[test]
    fn nesting_and_lists_past_the_limits_are_refused() {
        let deep = format!(
            "<x:a> <x:p> {}<x:b>{} .",
            "[ <x:p> ".repeat(40),
            " ]".repeat(40)
        );
        assert_eq!(read_turtle(&deep), Err(RdfReadError::TooDeep));
        let endless = format!("<x:a> <x:p> _:l . _:l <{RDF}first> <x:b> ; <{RDF}rest> _:l .");
        assert_eq!(read_turtle(&endless), Err(RdfReadError::TooDeep));
        // Blank nodes that only refer to each other are no resource of their own.
        assert_eq!(read_turtle("_:a <x:p> _:b . _:b <x:p> _:a .").unwrap(), []);
        let shared = "<x:a> <x:p> _:b ; <x:q> _:b . _:b <x:p> <x:c> .";
        assert_eq!(read_turtle(shared), Err(RdfReadError::TooDeep));
    }
}
