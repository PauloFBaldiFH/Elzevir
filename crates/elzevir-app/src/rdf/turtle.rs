// SPDX-License-Identifier: GPL-3.0-or-later
//! Turtle (W3C RDF 1.1 Turtle) of a graph (bibframe-completo.md, item 28a): one block per
//! named resource, its blank nodes nested in `[ ]`.

use super::{Node, Object, PREFIXES, blocks};

const INDENT: &str = "    ";

/// The Turtle document of `nodes`: the prefixes, then a block per named resource. A named
/// resource met as a value is written by its IRI, and described in a block of its own after
/// the others.
#[must_use]
pub fn write_turtle(nodes: &[Node]) -> String {
    let mut out = String::new();
    for (prefix, namespace) in PREFIXES {
        for part in ["@prefix ", prefix, ": <", namespace, "> .\n"] {
            out.push_str(part);
        }
    }
    for node in blocks(nodes) {
        out.push('\n');
        out.push_str(&node.iri.as_deref().map_or_else(|| "[]".to_owned(), term));
        body(node, 1, &mut out);
        out.push_str(" .\n");
    }
    out
}

/// The types and properties of `node`, each on its own line at `depth`.
fn body(node: &Node, depth: usize, out: &mut String) {
    let indent = INDENT.repeat(depth);
    let mut first = true;
    let mut separator = |out: &mut String| {
        out.push_str(if first { "\n" } else { " ;\n" });
        first = false;
        out.push_str(&indent);
    };
    if !node.types.is_empty() {
        separator(out);
        out.push_str("a ");
        let types: Vec<String> = node.types.iter().map(|iri| term(iri)).collect();
        out.push_str(&types.join(", "));
    }
    for (predicate, object) in &node.properties {
        separator(out);
        out.push_str(&term(predicate));
        out.push(' ');
        match object {
            Object::Iri(iri) => out.push_str(&term(iri)),
            Object::Literal { text, language } => {
                out.push_str(&quoted(text));
                if let Some(language) = language {
                    out.push('@');
                    out.push_str(language);
                }
            }
            Object::Node(inner) => match &inner.iri {
                Some(iri) => out.push_str(&term(iri)),
                None if inner.types.is_empty() && inner.properties.is_empty() => {
                    out.push_str("[]");
                }
                None => {
                    out.push('[');
                    body(inner, depth + 1, out);
                    out.push('\n');
                    out.push_str(&indent);
                    out.push(']');
                }
            },
        }
    }
}

/// An IRI as a prefixed name when one of the prefixes fits, else in `< >`.
fn term(iri: &str) -> String {
    PREFIXES
        .iter()
        .find_map(|(prefix, namespace)| {
            iri.strip_prefix(namespace)
                .filter(|local| is_local_name(local))
                .map(|local| format!("{prefix}:{local}"))
        })
        .unwrap_or_else(|| format!("<{}>", escaped_iri(iri)))
}

/// Whether `local` can follow a prefix as it is: letters, digits, `_` and `-`, not
/// starting with `-`.
fn is_local_name(local: &str) -> bool {
    !local.is_empty()
        && !local.starts_with('-')
        && local
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// An IRI with the characters Turtle forbids inside `< >` escaped as `\u` sequences.
fn escaped_iri(iri: &str) -> String {
    let mut out = String::with_capacity(iri.len());
    for c in iri.chars() {
        if c <= ' ' || "<>\"{}|^`\\".contains(c) {
            push_escape(&mut out, c);
        } else {
            out.push(c);
        }
    }
    out
}

/// `c` as a `\\u` escape; only called for characters below U+0080.
fn push_escape(out: &mut String, c: char) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    out.push_str("\\u00");
    let code = u32::from(c) & 0xFF;
    for shift in [4, 0] {
        let digit = HEX
            .get(((code >> shift) & 0xF) as usize)
            .copied()
            .unwrap_or(b'0');
        out.push(char::from(digit));
    }
}

/// A string literal in double quotes, escaped.
fn quoted(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < ' ' => push_escape(&mut out, c),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rdf::{BF, literal};

    #[test]
    fn literals_and_iris_are_escaped() {
        assert_eq!(quoted("a \"b\"\\\n\u{1}"), r#""a \"b\"\\\n\u0001""#);
        assert_eq!(term("http://x.example/a b"), r"<http://x.example/a\u0020b>");
        assert_eq!(term(&format!("{BF}Work")), "bf:Work");
        assert_eq!(term(&format!("{BF}a.b")), format!("<{BF}a.b>"));
    }

    #[test]
    fn named_values_get_a_block_of_their_own() {
        let mut agent = Node::labelled("Person", "Assis, Machado de");
        agent.iri = Some("http://viaf.example/1".to_owned());
        let mut work = Node::blank("Work");
        work.iri = Some("http://elzevir.example/works/1".to_owned());
        work.add("agent", Object::Node(Box::new(agent)));
        work.add(
            "summary",
            Object::Literal {
                text: "Resumo".to_owned(),
                language: Some("pt".to_owned()),
            },
        );
        work.node("note", Node::blank("Note"));
        work.add("note", Object::Node(Box::default()));
        work.add("editionStatement", literal("2. ed."));
        let expected = "\
@prefix bf: <http://id.loc.gov/ontologies/bibframe/> .
@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix madsrdf: <http://www.loc.gov/mads/rdf/v1#> .

<http://elzevir.example/works/1>
    a bf:Work ;
    bf:agent <http://viaf.example/1> ;
    bf:summary \"Resumo\"@pt ;
    bf:note [
        a bf:Note
    ] ;
    bf:note [] ;
    bf:editionStatement \"2. ed.\" .

<http://viaf.example/1>
    a bf:Person ;
    rdfs:label \"Assis, Machado de\" .
";
        assert_eq!(write_turtle(&[work]), expected);
    }
}
