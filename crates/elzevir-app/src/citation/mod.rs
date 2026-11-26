// SPDX-License-Identifier: GPL-3.0-or-later
//! The citation and interchange formats of one record: RIS, BibTeX, MODS and Dublin Core
//! (docs/specs/catalogo/citacao-e-intercambio.md), all from the MARC21 record the export
//! writes.
//!
//! Koha: `opac/opac-export.pl` with `C4::Ris::marc2ris`, `C4::Record::marc2bibtex`,
//! `C4::Record::marc2modsxml` and `C4::Record::marc2dcxml`.

mod bibtex;
mod mods;
mod ris;
#[cfg(test)]
mod tests;

use elzevir_domain::catalog::InstanceId;
use elzevir_marc::{Field, Record, Subfield, Tag};

use crate::oai::{DcElement, dc_element, dublin_core};

/// The formats a record exports to (item 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CitationFormat {
    /// RIS, for reference managers.
    Ris,
    /// A BibTeX entry.
    Bibtex,
    /// A MODS 3.1 document.
    Mods,
    /// Simple Dublin Core, as `oai_dc:dc`.
    DublinCore,
}

impl CitationFormat {
    /// The format a `format=` value names: `ris`, `bibtex`, `mods` or `dc`.
    #[must_use]
    pub fn of(name: &str) -> Option<Self> {
        match name {
            "ris" => Some(Self::Ris),
            "bibtex" => Some(Self::Bibtex),
            "mods" => Some(Self::Mods),
            "dc" => Some(Self::DublinCore),
            _ => None,
        }
    }

    /// The `Content-Type` of the file.
    #[must_use]
    pub const fn content_type(self) -> &'static str {
        match self {
            Self::Ris => "application/x-research-info-systems; charset=utf-8",
            Self::Bibtex => "application/x-bibtex; charset=utf-8",
            Self::Mods => "application/mods+xml; charset=utf-8",
            Self::DublinCore => "application/xml; charset=utf-8",
        }
    }

    /// The end of the file's name, after `elzevir-{id}`.
    #[must_use]
    pub const fn file_suffix(self) -> &'static str {
        match self {
            Self::Ris => ".ris",
            Self::Bibtex => ".bib",
            Self::Mods => "-mods.xml",
            Self::DublinCore => "-dc.xml",
        }
    }
}

/// `record`, the record of `instance`, in `format`, with the address of the Instance's
/// published deposit when it has one (item 5).
#[must_use]
pub fn cite(
    record: &Record,
    instance: InstanceId,
    deposit_url: Option<&str>,
    format: CitationFormat,
) -> String {
    match format {
        CitationFormat::Ris => ris::ris(record, deposit_url),
        CitationFormat::Bibtex => bibtex::bibtex(record, instance, deposit_url),
        CitationFormat::Mods => mods::mods(record, deposit_url),
        CitationFormat::DublinCore => {
            let mut elements = dublin_core(record);
            if let Some(url) = deposit_url {
                elements.push((DcElement::Identifier, url.to_owned()));
            }
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{}\n",
                dc_element(&elements)
            )
        }
    }
}

/// One data field: its indicators and subfields.
#[derive(Clone, Copy)]
struct DataField<'a> {
    indicators: [u8; 2],
    subfields: &'a [Subfield],
}

impl<'a> DataField<'a> {
    /// The first `code` subfield, as text.
    fn first(self, code: u8) -> Option<String> {
        self.subfields
            .iter()
            .find(|subfield| subfield.code == code)
            .map(|subfield| text(&subfield.value))
    }

    /// The first `code` subfield, or empty.
    fn value(self, code: u8) -> String {
        self.first(code).unwrap_or_default()
    }

    fn has(self, code: u8) -> bool {
        self.subfields.iter().any(|subfield| subfield.code == code)
    }

    /// Every `code` subfield, in order.
    fn all(self, code: u8) -> impl Iterator<Item = String> + 'a {
        self.subfields
            .iter()
            .filter(move |subfield| subfield.code == code)
            .map(|subfield| text(&subfield.value))
    }

    /// The subfields with one of `codes`, in order, joined by `delimiter`.
    ///
    /// Koha: `subfieldSelect` of `MARC21slimUtils-MODS31.xsl`.
    fn select_with(self, codes: &[u8], delimiter: &str) -> String {
        self.subfields
            .iter()
            .filter(|subfield| codes.contains(&subfield.code))
            .map(|subfield| text(&subfield.value))
            .collect::<Vec<_>>()
            .join(delimiter)
    }

    fn select(self, codes: &[u8]) -> String {
        self.select_with(codes, " ")
    }

    /// The subfields with one of `any`, or one of `before` with an `axis` subfield after
    /// it, or one of `after` with an `axis` subfield before it, joined by a space.
    ///
    /// Koha: `specialSubfieldSelect` of `MARC21slim2MODS3-1.xsl`.
    fn special(self, axis: u8, any: &[u8], before: &[u8], after: &[u8]) -> String {
        let subfields = self.subfields;
        subfields
            .iter()
            .enumerate()
            .filter(|(position, subfield)| {
                let code = subfield.code;
                let axis_after = || {
                    subfields
                        .iter()
                        .skip(position + 1)
                        .any(|later| later.code == axis)
                };
                let axis_before = || {
                    subfields
                        .iter()
                        .take(*position)
                        .any(|earlier| earlier.code == axis)
                };
                any.contains(&code)
                    || (before.contains(&code) && axis_after())
                    || (after.contains(&code) && axis_before())
            })
            .map(|(_, subfield)| text(&subfield.value))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// The data fields with `tag`, in order.
fn fields(record: &Record, tag: [u8; 3]) -> impl Iterator<Item = DataField<'_>> + '_ {
    let tag = Tag::new(tag);
    record.fields.iter().filter_map(move |field| match field {
        Field::Data {
            tag: found,
            indicators,
            subfields,
        } if *found == tag => Some(DataField {
            indicators: *indicators,
            subfields,
        }),
        _ => None,
    })
}

/// The data fields with one of `tags`, in the record's order.
fn fields_of<'a>(
    record: &'a Record,
    tags: &'a [&'a [u8; 3]],
) -> impl Iterator<Item = (Tag, DataField<'a>)> + 'a {
    record.fields.iter().filter_map(move |field| match field {
        Field::Data {
            tag,
            indicators,
            subfields,
        } if tags.iter().any(|wanted| Tag::new(**wanted) == *tag) => Some((
            *tag,
            DataField {
                indicators: *indicators,
                subfields,
            },
        )),
        _ => None,
    })
}

fn first_field(record: &Record, tag: [u8; 3]) -> Option<DataField<'_>> {
    fields(record, tag).next()
}

/// The first `code` subfield of the first field with `tag`, or empty, like
/// `MARC::Record::subfield`.
fn subfield(record: &Record, tag: [u8; 3], code: u8) -> String {
    first_field(record, tag)
        .map(|field| field.value(code))
        .unwrap_or_default()
}

/// The control field with `tag`.
fn control(record: &Record, tag: [u8; 3]) -> Option<String> {
    let tag = Tag::new(tag);
    record.fields.iter().find_map(|field| match field {
        Field::Control { tag: found, value } if *found == tag => Some(text(value)),
        _ => None,
    })
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// `text` without the characters of `punctuation` at its end.
///
/// Koha: `chopPunctuation` of `MARC21slimUtils-MODS31.xsl`.
fn chop(text: &str, punctuation: &str) -> String {
    text.trim_end_matches(|c| punctuation.contains(c))
        .to_owned()
}

/// [`chop`] with the stylesheet's default `.:,;/` and space.
fn chopped(text: &str) -> String {
    chop(text, ".:,;/ ")
}
