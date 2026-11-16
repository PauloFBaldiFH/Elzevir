// SPDX-License-Identifier: GPL-3.0-or-later
//! RDF/XML (W3C RDF 1.1 XML Syntax) of a graph (bibframe-completo.md, item 28b): a typed
//! node element per named resource, its blank nodes nested inside their property.

use super::{Node, Object, PREFIXES, blocks};

const INDENT: &str = "  ";

/// The RDF/XML document of `nodes`, with the same blocks as the Turtle one.
#[must_use]
pub fn write_rdf_xml(nodes: &[Node]) -> String {
    let mut out = rdf_xml_header();
    out.push_str(&rdf_xml_blocks(nodes));
    out.push_str(RDF_XML_FOOTER);
    out
}

/// What closes an RDF/XML document.
pub(super) const RDF_XML_FOOTER: &str = "</rdf:RDF>\n";

/// The XML declaration and the opening `rdf:RDF` element with the namespaces.
pub(super) fn rdf_xml_header() -> String {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<rdf:RDF");
    for (prefix, namespace) in PREFIXES {
        for part in [
            "\n", INDENT, INDENT, "xmlns:", prefix, "=\"", namespace, "\"",
        ] {
            out.push_str(part);
        }
    }
    out.push_str(">\n");
    out
}

/// The node elements of `nodes`, inside `rdf:RDF`.
pub(super) fn rdf_xml_blocks(nodes: &[Node]) -> String {
    let mut out = String::new();
    for node in blocks(nodes) {
        node_element(node, 1, &mut out);
    }
    out
}

/// `node` as a node element at `depth`: named after its first class, with the others as
/// `rdf:type`.
fn node_element(node: &Node, depth: usize, out: &mut String) {
    let indent = INDENT.repeat(depth);
    let (element, other_types) = match node.types.split_first() {
        Some((first, rest)) => match qname(first) {
            Some(name) => (name, rest),
            None => ("rdf:Description".to_owned(), node.types.as_slice()),
        },
        None => ("rdf:Description".to_owned(), node.types.as_slice()),
    };
    out.push_str(&indent);
    out.push('<');
    out.push_str(&element);
    if let Some(iri) = &node.iri {
        out.push_str(" rdf:about=\"");
        out.push_str(&escaped(iri));
        out.push('"');
    }
    if other_types.is_empty() && node.properties.is_empty() {
        out.push_str("/>\n");
        return;
    }
    out.push_str(">\n");
    let inner = INDENT.repeat(depth + 1);
    for class in other_types {
        resource_property(&inner, "rdf:type", None, class, out);
    }
    for (predicate, object) in &node.properties {
        let (name, namespace) = predicate_name(predicate);
        match object {
            Object::Iri(iri) => resource_property(&inner, &name, namespace, iri, out),
            Object::Node(value) if value.iri.is_some() => {
                let iri = value.iri.as_deref().unwrap_or_default();
                resource_property(&inner, &name, namespace, iri, out);
            }
            Object::Literal { text, language } => {
                open_property(&inner, &name, namespace, out);
                if let Some(language) = language {
                    out.push_str(" xml:lang=\"");
                    out.push_str(&escaped(language));
                    out.push('"');
                }
                out.push('>');
                out.push_str(&escaped(text));
                close_property(&name, out);
            }
            Object::Node(value) => {
                open_property(&inner, &name, namespace, out);
                out.push_str(">\n");
                node_element(value, depth + 2, out);
                out.push_str(&inner);
                close_property(&name, out);
            }
            Object::List(members) => {
                open_property(&inner, &name, namespace, out);
                out.push_str(" rdf:parseType=\"Collection\">\n");
                for member in members {
                    match &member.iri {
                        // A named member is described in its own block.
                        Some(iri) => {
                            out.push_str(&INDENT.repeat(depth + 2));
                            out.push_str("<rdf:Description rdf:about=\"");
                            out.push_str(&escaped(iri));
                            out.push_str("\"/>\n");
                        }
                        None => node_element(member, depth + 2, out),
                    }
                }
                out.push_str(&inner);
                close_property(&name, out);
            }
        }
    }
    out.push_str(&indent);
    out.push_str("</");
    out.push_str(&element);
    out.push_str(">\n");
}

fn open_property(indent: &str, name: &str, namespace: Option<&str>, out: &mut String) {
    out.push_str(indent);
    out.push('<');
    out.push_str(name);
    if let Some(namespace) = namespace {
        out.push_str(" xmlns:ns=\"");
        out.push_str(&escaped(namespace));
        out.push('"');
    }
}

fn close_property(name: &str, out: &mut String) {
    out.push_str("</");
    out.push_str(name);
    out.push_str(">\n");
}

fn resource_property(
    indent: &str,
    name: &str,
    namespace: Option<&str>,
    iri: &str,
    out: &mut String,
) {
    open_property(indent, name, namespace, out);
    out.push_str(" rdf:resource=\"");
    out.push_str(&escaped(iri));
    out.push_str("\"/>\n");
}

/// A property's element name: a prefixed name when a prefix fits, else `ns:` with the
/// namespace to declare on the element.
fn predicate_name(iri: &str) -> (String, Option<&str>) {
    if let Some(name) = qname(iri) {
        return (name, None);
    }
    let split = iri.rfind(['#', '/']).map_or(0, |at| at.saturating_add(1));
    // Every predicate the export writes is a property of the ontology (statements are
    // checked against it when stored), whose names are XML names.
    let (namespace, local) = iri.split_at(split);
    (format!("ns:{local}"), Some(namespace))
}

/// `iri` as a prefixed name, when one of the prefixes fits.
fn qname(iri: &str) -> Option<String> {
    PREFIXES.iter().find_map(|(prefix, namespace)| {
        iri.strip_prefix(namespace)
            .filter(|local| is_name(local))
            .map(|local| format!("{prefix}:{local}"))
    })
}

/// Whether `local` is an XML name without a colon: a letter or `_`, then letters, digits,
/// `_`, `-` and `.`.
fn is_name(local: &str) -> bool {
    let mut chars = local.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || "_-.".contains(c))
}

/// Text or an attribute value with `&`, `<`, `>` and `"` escaped. Control characters that
/// XML 1.0 cannot carry become U+FFFD.
fn escaped(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' | '\n' => out.push(c),
            '\r' => out.push_str("&#13;"),
            c if c < ' ' => out.push('\u{FFFD}'),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rdf::literal;

    #[test]
    fn a_graph_is_written_as_rdf_xml() {
        let mut agent = Node::labelled("Person", "Assis & <cia>");
        agent.iri = Some("http://viaf.example/1".to_owned());
        let mut work = Node::blank("Work");
        work.types.push(format!("{}Text", crate::rdf::BF));
        work.iri = Some("http://elzevir.example/works/1".to_owned());
        work.add("agent", Object::Node(Box::new(agent)));
        work.node("note", Node::labelled("Note", "Nota"));
        work.add(
            "summary",
            Object::Literal {
                text: "Resumo".to_owned(),
                language: Some("pt".to_owned()),
            },
        );
        work.add("note", Object::Node(Box::default()));
        work.properties
            .push(("http://other.example/v#size".to_owned(), literal("1")));
        let expected = r#"<?xml version="1.0" encoding="UTF-8"?>
<rdf:RDF
    xmlns:bf="http://id.loc.gov/ontologies/bibframe/"
    xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
    xmlns:rdfs="http://www.w3.org/2000/01/rdf-schema#"
    xmlns:madsrdf="http://www.loc.gov/mads/rdf/v1#">
  <bf:Work rdf:about="http://elzevir.example/works/1">
    <rdf:type rdf:resource="http://id.loc.gov/ontologies/bibframe/Text"/>
    <bf:agent rdf:resource="http://viaf.example/1"/>
    <bf:note>
      <bf:Note>
        <rdfs:label>Nota</rdfs:label>
      </bf:Note>
    </bf:note>
    <bf:summary xml:lang="pt">Resumo</bf:summary>
    <bf:note>
      <rdf:Description/>
    </bf:note>
    <ns:size xmlns:ns="http://other.example/v#">1</ns:size>
  </bf:Work>
  <bf:Person rdf:about="http://viaf.example/1">
    <rdfs:label>Assis &amp; &lt;cia&gt;</rdfs:label>
  </bf:Person>
</rdf:RDF>
"#;
        assert_eq!(write_rdf_xml(&[work]), expected);
    }
}
