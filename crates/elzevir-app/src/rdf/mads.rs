// SPDX-License-Identifier: GPL-3.0-or-later
//! The structure of names and subjects as MADS/RDF (bibframe-completo.md, item 28c): the
//! parts of a name as a `madsrdf:elementList`, the subdivisions of a subject as the
//! `madsrdf:componentList` of a `madsrdf:ComplexSubject`, as the Library of Congress
//! conversion writes them.

use elzevir_domain::catalog::{AgentKind, NamePartKind, SubjectKind};

use super::{MADSRDF, Node, Object, literal};
use crate::catalog::{NamePart, NewSubject};

/// What separates a subject's subdivisions in its heading.
const SUBDIVISION: &str = " -- ";

/// The elements of `name`, an agent's heading, cut where each of `parts` starts; `None`
/// when the name is one part, or the parts do not fit it.
///
/// The first element is the name itself; each part takes the MADS element of its kind.
/// A person's numeration and an organization's subordinate unit are both a
/// `NameElement`, told apart by the agent's class.
pub(super) fn name_elements(kind: AgentKind, name: &str, parts: &[NamePart]) -> Option<Object> {
    if parts.is_empty() {
        return None;
    }
    let starts: Vec<usize> = name.char_indices().map(|(at, _)| at).collect();
    let byte = |start: usize| {
        if start == starts.len() {
            Some(name.len())
        } else {
            starts.get(start).copied()
        }
    };
    let mut elements = Vec::with_capacity(parts.len() + 1);
    let mut from = 0;
    let mut class = main_element(kind);
    for part in parts {
        // Each part starts after the one before it, and the first after the name.
        let at = byte(part.start)?;
        if at <= from {
            return None;
        }
        elements.push(element(class, name.get(from..at)?));
        from = at;
        class = part_element(part.kind);
    }
    elements.push(element(class, name.get(from..)?));
    Some(Object::List(elements))
}

/// The structure of `subject` added to `node`: its subdivisions as a complex subject, and
/// the parts of the name it names.
pub(super) fn describe_subject(subject: &NewSubject, node: &mut Node) {
    let components: Vec<&str> = subject.heading.split(SUBDIVISION).collect();
    let agent = subject.kind.agent();
    if components.len() < 2 {
        if let Some(elements) =
            agent.and_then(|kind| name_elements(kind, &subject.heading, &subject.name_parts))
        {
            node.add_mads("elementList", elements);
        }
        return;
    }
    node.types.push(format!("{MADSRDF}ComplexSubject"));
    let typed_from = components.len().saturating_sub(subject.subdivisions.len());
    let mut list = Vec::with_capacity(components.len());
    for (index, text) in components.iter().enumerate() {
        let kind = if index == 0 {
            subject.kind
        } else {
            index
                .checked_sub(typed_from)
                .and_then(|at| subject.subdivisions.get(at).copied())
                .unwrap_or(SubjectKind::Topic)
        };
        let mut component = Node::mads(component_class(kind));
        component.add_mads("authoritativeLabel", literal(text));
        if index == 0
            && let Some(elements) =
                agent.and_then(|agent| name_elements(agent, text, &subject.name_parts))
        {
            component.add_mads("elementList", elements);
        }
        list.push(component);
    }
    node.add_mads("componentList", Object::List(list));
}

fn element(class: &str, text: &str) -> Node {
    let mut element = Node::mads(class);
    element.add_mads("elementValue", literal(text));
    element
}

const fn main_element(kind: AgentKind) -> &'static str {
    match kind {
        AgentKind::Person => "FullNameElement",
        AgentKind::Family => "FamilyNameElement",
        AgentKind::Organization | AgentKind::Meeting => "NameElement",
    }
}

const fn part_element(kind: NamePartKind) -> &'static str {
    match kind {
        NamePartKind::Numeration | NamePartKind::Subordinate => "NameElement",
        NamePartKind::TermsOfAddress => "TermsOfAddressNameElement",
        NamePartKind::Date => "DateNameElement",
        NamePartKind::FullerForm => "FullNameElement",
        NamePartKind::Place => "GeographicElement",
        NamePartKind::Number => "PartNumberElement",
        NamePartKind::Title => "TitleElement",
    }
}

/// The MADS class of a component of a complex subject.
const fn component_class(kind: SubjectKind) -> &'static str {
    match kind {
        SubjectKind::Topic => "Topic",
        SubjectKind::Place => "Geographic",
        SubjectKind::Person => "PersonalName",
        SubjectKind::Organization => "CorporateName",
        SubjectKind::Meeting => "ConferenceName",
        SubjectKind::Family => "FamilyName",
        SubjectKind::Title => "Title",
        SubjectKind::GenreForm => "GenreForm",
        SubjectKind::Temporal => "Temporal",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(object: Option<Object>) -> Vec<(String, String)> {
        let Some(Object::List(elements)) = object else {
            return Vec::new();
        };
        elements
            .iter()
            .map(|element| {
                let class = element.types[0].trim_start_matches(MADSRDF).to_owned();
                let Object::Literal { text, .. } = &element.properties[0].1 else {
                    panic!("an element has a value");
                };
                (class, text.clone())
            })
            .collect()
    }

    #[test]
    fn a_name_is_cut_into_its_elements() {
        let part = |kind, start| NamePart { kind, start };
        let name = "Pedro II, Imperador do Brasil, 1825-1891";
        let parts = [
            part(NamePartKind::Numeration, 6),
            part(NamePartKind::TermsOfAddress, 10),
            part(NamePartKind::Date, 31),
        ];
        assert_eq!(
            values(name_elements(AgentKind::Person, name, &parts)),
            [
                ("FullNameElement".to_owned(), "Pedro ".to_owned()),
                ("NameElement".to_owned(), "II, ".to_owned()),
                (
                    "TermsOfAddressNameElement".to_owned(),
                    "Imperador do Brasil, ".to_owned()
                ),
                ("DateNameElement".to_owned(), "1825-1891".to_owned()),
            ]
        );
        // One part, parts out of order or past the end: no elements.
        assert!(name_elements(AgentKind::Person, name, &[]).is_none());
        assert!(name_elements(AgentKind::Person, name, &[part(NamePartKind::Date, 0)]).is_none());
        assert!(name_elements(AgentKind::Person, name, &[part(NamePartKind::Date, 99)]).is_none());
        assert!(
            name_elements(
                AgentKind::Person,
                name,
                &[
                    part(NamePartKind::Date, 31),
                    part(NamePartKind::Numeration, 6)
                ]
            )
            .is_none()
        );
        // Accented letters count as one character each.
        assert_eq!(
            values(name_elements(
                AgentKind::Organization,
                "São Paulo. Câmara",
                &[part(NamePartKind::Subordinate, 11)]
            )),
            [
                ("NameElement".to_owned(), "São Paulo. ".to_owned()),
                ("NameElement".to_owned(), "Câmara".to_owned()),
            ]
        );
    }
}
