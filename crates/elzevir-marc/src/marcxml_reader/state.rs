// SPDX-License-Identifier: GPL-3.0-or-later
//! MARCXML grammar and owned record construction; no catalog mapping here.

use quick_xml::events::BytesStart;

use super::{XmlLimits, XmlProblem};
use crate::{Field, Record, Subfield, Tag};

enum Node {
    Collection,
    Record,
    Leader,
    Control(Tag),
    Data(Tag, [u8; 2]),
    Subfield(u8),
}

/// The open grammar path and one record under construction.
#[derive(Default)]
pub(super) struct State {
    pub(super) record: Option<Record>,
    pub(super) closed: bool,
    path: Vec<Node>,
    value: Vec<u8>,
    subfields: Vec<Subfield>,
    leader: bool,
    elements: usize,
}

impl State {
    pub(super) fn begin(
        &mut self,
        start: &BytesStart<'_>,
        limits: &XmlLimits,
    ) -> Result<(), XmlProblem> {
        if self.path.len() >= limits.max_depth {
            return Err(XmlProblem::Limit("depth"));
        }
        for attribute in start.attributes() {
            let attribute = attribute.map_err(|_| XmlProblem::Invalid("attributes"))?;
            let value = attribute
                .normalized_value(quick_xml::XmlVersion::Explicit1_0)
                .map_err(|_| XmlProblem::Invalid("attribute value"))?;
            if !legal_text(&value) {
                return Err(XmlProblem::Invalid("XML character"));
            }
        }
        let name = start.local_name();
        let node = match (self.path.last(), name.as_ref()) {
            (None, b"collection") if !self.closed => Node::Collection,
            (None | Some(Node::Collection), b"record") if !self.closed => {
                if attribute(start, b"type")?.is_some_and(|value| value != b"Bibliographic") {
                    return Err(XmlProblem::Forbidden("record type"));
                }
                self.record = Some(Record {
                    leader: [b' '; 24],
                    fields: Vec::new(),
                });
                self.leader = false;
                self.elements = 0;
                Node::Record
            }
            (Some(Node::Record), b"leader") if !self.leader => Node::Leader,
            (Some(Node::Record), b"controlfield") if self.leader => {
                Node::Control(tag(start, true)?)
            }
            (Some(Node::Record), b"datafield") if self.leader => {
                let [first] = required(start, b"ind1")?;
                let [second] = required(start, b"ind2")?;
                if ![first, second]
                    .iter()
                    .all(|byte| *byte == b' ' || byte.is_ascii_graphic())
                {
                    return Err(XmlProblem::Invalid("indicators"));
                }
                Node::Data(tag(start, false)?, [first, second])
            }
            (Some(Node::Data(..)), b"subfield") => {
                let [code] = required(start, b"code")?;
                if !code.is_ascii_graphic() {
                    return Err(XmlProblem::Invalid("subfield code"));
                }
                Node::Subfield(code)
            }
            _ => return Err(XmlProblem::Invalid("record structure")),
        };
        if self.record.is_some() {
            if self.elements >= limits.max_elements_per_record {
                return Err(XmlProblem::Limit("elements per record"));
            }
            self.elements += 1;
        }
        self.path.push(node);
        Ok(())
    }

    pub(super) fn append(&mut self, text: &str, limits: &XmlLimits) -> Result<(), XmlProblem> {
        if !legal_text(text) {
            return Err(XmlProblem::Invalid("XML character"));
        }
        if matches!(
            self.path.last(),
            Some(Node::Leader | Node::Control(_) | Node::Subfield(_))
        ) {
            if text.len() > limits.max_element_bytes.saturating_sub(self.value.len()) {
                return Err(XmlProblem::Limit("scalar bytes"));
            }
            self.value.extend_from_slice(text.as_bytes());
        } else if !text
            .bytes()
            .all(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
        {
            return Err(XmlProblem::Invalid("text outside a field"));
        }
        Ok(())
    }

    pub(super) fn end(&mut self) -> Result<Option<Record>, XmlProblem> {
        let node = self
            .path
            .pop()
            .ok_or(XmlProblem::Invalid("closing element"))?;
        match node {
            Node::Leader => {
                let mut leader: [u8; 24] = self
                    .value
                    .as_slice()
                    .try_into()
                    .map_err(|_| XmlProblem::Invalid("leader"))?;
                if !leader.is_ascii()
                    || !leader
                        .get(..5)
                        .is_some_and(|part| part.iter().all(u8::is_ascii_digit))
                    || !leader
                        .get(12..17)
                        .is_some_and(|part| part.iter().all(u8::is_ascii_digit))
                {
                    return Err(XmlProblem::Invalid("leader"));
                }
                if leader.get(6) == Some(&b'z') {
                    return Err(XmlProblem::Forbidden("authority record"));
                }
                *leader.get_mut(9).ok_or(XmlProblem::Invalid("leader"))? = b'a';
                self.record
                    .as_mut()
                    .ok_or(XmlProblem::Invalid("record"))?
                    .leader = leader;
                self.value.clear();
                self.leader = true;
            }
            Node::Control(tag) => self
                .record
                .as_mut()
                .ok_or(XmlProblem::Invalid("record"))?
                .fields
                .push(Field::Control {
                    tag,
                    value: std::mem::take(&mut self.value),
                }),
            Node::Data(tag, indicators) => self
                .record
                .as_mut()
                .ok_or(XmlProblem::Invalid("record"))?
                .fields
                .push(Field::Data {
                    tag,
                    indicators,
                    subfields: std::mem::take(&mut self.subfields),
                }),
            Node::Subfield(code) => self.subfields.push(Subfield {
                code,
                value: std::mem::take(&mut self.value),
            }),
            Node::Record if !self.leader => return Err(XmlProblem::Invalid("missing leader")),
            Node::Record => {
                self.closed = self.path.is_empty();
                return self
                    .record
                    .take()
                    .map(Some)
                    .ok_or(XmlProblem::Invalid("record"));
            }
            Node::Collection => self.closed = true,
        }
        Ok(None)
    }
}

pub(super) fn legal_text(text: &str) -> bool {
    text.chars().all(|c| matches!(c, '\t' | '\n' | '\r' | ' '..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}'))
}

fn attribute(start: &BytesStart<'_>, key: &[u8]) -> Result<Option<Vec<u8>>, XmlProblem> {
    start
        .try_get_attribute(key)
        .map_err(|_| XmlProblem::Invalid("attributes"))?
        .map(|attribute| {
            attribute
                .normalized_value(quick_xml::XmlVersion::Explicit1_0)
                .map(|value| value.into_owned().into_bytes())
                .map_err(|_| XmlProblem::Invalid("attribute value"))
        })
        .transpose()
}

fn required<const N: usize>(start: &BytesStart<'_>, key: &[u8]) -> Result<[u8; N], XmlProblem> {
    attribute(start, key)?
        .ok_or(XmlProblem::Invalid("missing attribute"))?
        .try_into()
        .map_err(|_| XmlProblem::Invalid("attribute length"))
}

fn tag(start: &BytesStart<'_>, control: bool) -> Result<Tag, XmlProblem> {
    let bytes = required::<3>(start, b"tag")?;
    let tag = Tag::new(bytes);
    if !bytes.iter().all(u8::is_ascii_digit) || bytes == *b"000" || tag.is_control() != control {
        return Err(XmlProblem::Invalid("field tag"));
    }
    Ok(tag)
}
