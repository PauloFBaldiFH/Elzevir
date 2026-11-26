// SPDX-License-Identifier: GPL-3.0-or-later
//! Simple Dublin Core from a MARC21 record (docs/specs/opac/oai-pmh.md, item 16).
//!
//! Koha: `koha-tmpl/intranet-tmpl/prog/en/xslt/MARC21slim2OAIDC.xsl`.

use elzevir_marc::{Field, Record, Subfield, Tag};

/// The Dublin Core elements the crosswalk fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DcElement {
    /// `dc:title`.
    Title,
    /// `dc:creator`.
    Creator,
    /// `dc:type`.
    Type,
    /// `dc:publisher`.
    Publisher,
    /// `dc:date`.
    Date,
    /// `dc:language`.
    Language,
    /// `dc:format`.
    Format,
    /// `dc:description`.
    Description,
    /// `dc:subject`.
    Subject,
    /// `dc:coverage`.
    Coverage,
    /// `dc:relation`.
    Relation,
    /// `dc:identifier`.
    Identifier,
    /// `dc:rights`.
    Rights,
}

impl DcElement {
    /// The element's local name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Creator => "creator",
            Self::Type => "type",
            Self::Publisher => "publisher",
            Self::Date => "date",
            Self::Language => "language",
            Self::Format => "format",
            Self::Description => "description",
            Self::Subject => "subject",
            Self::Coverage => "coverage",
            Self::Relation => "relation",
            Self::Identifier => "identifier",
            Self::Rights => "rights",
        }
    }
}

/// The elements of `record`, in the crosswalk's order, none empty.
#[must_use]
pub fn dublin_core(record: &Record) -> Vec<(DcElement, String)> {
    let mut out = Crosswalk {
        record,
        elements: Vec::new(),
    };
    out.each(&[b"245"], DcElement::Title, |field| {
        joined(field, b"abfghk")
    });
    out.document_order(
        &[b"100", b"110", b"111", b"700", b"710", b"711", b"720"],
        DcElement::Creator,
    );
    out.push(DcElement::Type, &kind(&record.leader));
    out.each(&[b"655"], DcElement::Type, |field| joined(field, b""));
    for tag in [b"260", b"264"] {
        out.each(&[tag], DcElement::Publisher, |field| joined(field, b"ab"));
        out.every(*tag, b'c', DcElement::Date);
    }
    let language = control(record, *b"008")
        .and_then(|value| value.get(35..38))
        .map(text)
        .unwrap_or_default();
    out.push(DcElement::Language, &language);
    out.every(*b"856", b'q', DcElement::Format);
    out.each(&[b"520"], DcElement::Description, |field| {
        first(field, b'a')
    });
    out.each(&[b"521"], DcElement::Description, |field| {
        first(field, b'a')
    });
    out.descriptions();
    for tag in [b"600", b"610", b"611", b"630", b"650", b"653"] {
        out.each(&[tag], DcElement::Subject, |field| joined(field, b"abcdq"));
    }
    out.each(&[b"752"], DcElement::Coverage, |field| {
        joined(field, b"abcd")
    });
    out.each(&[b"530"], DcElement::Relation, |field| {
        joined(field, b"abcdu")
    });
    out.document_order_selected(
        &[
            b"760", b"762", b"765", b"767", b"770", b"772", b"773", b"774", b"775", b"776", b"777",
            b"780", b"785", b"786", b"787",
        ],
        DcElement::Relation,
        b"ot",
    );
    out.each(&[b"856"], DcElement::Identifier, |field| first(field, b'u'));
    out.each(&[b"020"], DcElement::Identifier, |field| {
        let isbn = first(field, b'a');
        if isbn.is_empty() {
            isbn
        } else {
            format!("URN:ISBN:{isbn}")
        }
    });
    out.each(&[b"506"], DcElement::Rights, |field| first(field, b'a'));
    out.each(&[b"540"], DcElement::Rights, |field| first(field, b'a'));
    out.elements
}

struct Crosswalk<'a> {
    record: &'a Record,
    elements: Vec<(DcElement, String)>,
}

impl Crosswalk<'_> {
    fn push(&mut self, element: DcElement, value: &str) {
        let value = value.trim();
        if !value.is_empty() {
            self.elements.push((element, value.to_owned()));
        }
    }

    /// One element per field with one of `tags`, tag by tag.
    fn each(
        &mut self,
        tags: &[&[u8; 3]],
        element: DcElement,
        value: impl Fn(&[Subfield]) -> String,
    ) {
        for tag in tags {
            let values: Vec<String> = data_fields(self.record, **tag).map(&value).collect();
            for one in values {
                self.push(element, &one);
            }
        }
    }

    /// One element per field with one of `tags`, in the record's order.
    fn document_order(&mut self, tags: &[&[u8; 3]], element: DcElement) {
        self.document_order_selected(tags, element, b"");
    }

    fn document_order_selected(&mut self, tags: &[&[u8; 3]], element: DcElement, codes: &[u8]) {
        let values: Vec<String> = self
            .record
            .fields
            .iter()
            .filter_map(|field| match field {
                Field::Data { tag, subfields, .. }
                    if tags.iter().any(|wanted| Tag::new(**wanted) == *tag) =>
                {
                    Some(joined(subfields, codes))
                }
                _ => None,
            })
            .collect();
        for value in values {
            self.push(element, &value);
        }
    }

    /// One element per `code` subfield of each field with `tag`.
    fn every(&mut self, tag: [u8; 3], code: u8, element: DcElement) {
        let values: Vec<String> = data_fields(self.record, tag)
            .flat_map(|subfields| {
                subfields
                    .iter()
                    .filter(move |subfield| subfield.code == code)
                    .map(|subfield| text(&subfield.value))
            })
            .collect();
        for value in values {
            self.push(element, &value);
        }
    }

    /// The first `$a` of the notes `501` to `599`, but for the ones of other elements.
    fn descriptions(&mut self) {
        let values: Vec<String> = self
            .record
            .fields
            .iter()
            .filter_map(|field| match field {
                Field::Data { tag, subfields, .. } => {
                    let number = std::str::from_utf8(tag.as_bytes())
                        .ok()?
                        .parse::<u16>()
                        .ok()?;
                    let note = (501..=599).contains(&number)
                        && ![506, 520, 521, 530, 540, 546].contains(&number);
                    note.then(|| first(subfields, b'a'))
                }
                Field::Control { .. } => None,
            })
            .collect();
        for value in values {
            self.push(DcElement::Description, &value);
        }
    }
}

fn data_fields(record: &Record, tag: [u8; 3]) -> impl Iterator<Item = &[Subfield]> {
    let tag = Tag::new(tag);
    record.fields.iter().filter_map(move |field| match field {
        Field::Data {
            tag: found,
            subfields,
            ..
        } if *found == tag => Some(subfields.as_slice()),
        _ => None,
    })
}

fn control(record: &Record, tag: [u8; 3]) -> Option<&[u8]> {
    let tag = Tag::new(tag);
    record.fields.iter().find_map(|field| match field {
        Field::Control { tag: found, value } if *found == tag => Some(value.as_slice()),
        _ => None,
    })
}

/// The subfields with one of `codes` (all of them when empty), joined by a space.
fn joined(subfields: &[Subfield], codes: &[u8]) -> String {
    subfields
        .iter()
        .filter(|subfield| codes.is_empty() || codes.contains(&subfield.code))
        .map(|subfield| text(&subfield.value))
        .filter(|value| !value.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn first(subfields: &[Subfield], code: u8) -> String {
    subfields
        .iter()
        .find(|subfield| subfield.code == code)
        .map(|subfield| text(&subfield.value))
        .unwrap_or_default()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).trim().to_owned()
}

/// `dc:type` from the leader: collection, manuscript and the type of record.
fn kind(leader: &[u8]) -> String {
    let type_of_record = leader.get(6).copied().unwrap_or(b' ');
    let level = leader.get(7).copied().unwrap_or(b' ');
    let mut parts = Vec::new();
    if level == b'c' {
        parts.push("collection");
    }
    if matches!(type_of_record, b'd' | b'f' | b'p' | b't') {
        parts.push("manuscript");
    }
    let named = match type_of_record {
        b'a' | b't' => Some("text"),
        b'e' | b'f' => Some("cartographic"),
        b'c' | b'd' => Some("notated music"),
        b'i' | b'j' => Some("sound recording"),
        b'k' => Some("still image"),
        b'g' => Some("moving image"),
        b'r' => Some("three dimensional object"),
        b'm' => Some("software, multimedia"),
        b'p' => Some("mixed material"),
        _ => None,
    };
    parts.extend(named);
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(tag: [u8; 3], subfields: &[(u8, &str)]) -> Field {
        Field::Data {
            tag: Tag::new(tag),
            indicators: *b"  ",
            subfields: subfields
                .iter()
                .map(|(code, value)| Subfield {
                    code: *code,
                    value: value.as_bytes().to_vec(),
                })
                .collect(),
        }
    }

    #[test]
    fn a_book_crosses_to_dublin_core_in_the_stylesheets_order() {
        let mut leader = [b' '; 24];
        leader[6] = b'a';
        leader[7] = b'm';
        let record = Record {
            leader,
            fields: vec![
                Field::Control {
                    tag: Tag::new(*b"008"),
                    value: b"261126s2026    bl            000 0 por d".to_vec(),
                },
                data(*b"020", &[(b'a', "9788535914849")]),
                data(
                    *b"100",
                    &[(b'a', "Assis, Machado de,"), (b'd', "1839-1908.")],
                ),
                data(
                    *b"245",
                    &[(b'a', "Dom Casmurro /"), (b'c', "Machado de Assis.")],
                ),
                data(
                    *b"264",
                    &[
                        (b'a', "Rio de Janeiro :"),
                        (b'b', "Garnier,"),
                        (b'c', "1899."),
                    ],
                ),
                // The stylesheet takes the notes after 500, not 500 itself.
                data(*b"500", &[(b'a', "Primeira edição.")]),
                data(*b"504", &[(b'a', "Bibliografia.")]),
                data(*b"520", &[(b'a', "Bentinho e Capitu.")]),
                data(*b"650", &[(b'a', "Ficção brasileira."), (b'x', "ignored")]),
                data(
                    *b"700",
                    &[(b'a', "Lemos, Teodora,"), (b'e', "ilustradora.")],
                ),
                data(
                    *b"856",
                    &[
                        (b'u', "https://example.org/dom-casmurro"),
                        (b'q', "text/html"),
                    ],
                ),
            ],
        };
        let elements: Vec<(&str, String)> = dublin_core(&record)
            .into_iter()
            .map(|(element, value)| (element.name(), value))
            .collect();
        assert_eq!(
            elements,
            [
                ("title", "Dom Casmurro /".to_owned()),
                ("creator", "Assis, Machado de, 1839-1908.".to_owned()),
                ("creator", "Lemos, Teodora, ilustradora.".to_owned()),
                ("type", "text".to_owned()),
                ("publisher", "Rio de Janeiro : Garnier,".to_owned()),
                ("date", "1899.".to_owned()),
                ("language", "por".to_owned()),
                ("format", "text/html".to_owned()),
                ("description", "Bentinho e Capitu.".to_owned()),
                ("description", "Bibliografia.".to_owned()),
                ("subject", "Ficção brasileira.".to_owned()),
                ("identifier", "https://example.org/dom-casmurro".to_owned()),
                ("identifier", "URN:ISBN:9788535914849".to_owned()),
            ]
        );
    }

    #[test]
    fn the_type_names_collections_and_manuscripts() {
        let leader = |record_type: u8, level: u8| {
            let mut leader = [b' '; 24];
            leader[6] = record_type;
            leader[7] = level;
            leader
        };
        assert_eq!(kind(&leader(b't', b'c')), "collection manuscript text");
        assert_eq!(kind(&leader(b'p', b'm')), "manuscript mixed material");
        assert_eq!(kind(&leader(b'z', b'm')), "");
    }
}
