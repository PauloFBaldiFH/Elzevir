// SPDX-License-Identifier: GPL-3.0-or-later
//! The heading of a MARC21 authority record (docs/specs/catalogo/fontes-de-autoridade.md,
//! items 9 to 14), read by the same rules as the subject fields of a bibliographic record.

use elzevir_domain::catalog::SubjectKind;
use elzevir_marc::Record;

use super::Marc;
use super::text;
use super::work::subject_of;

/// The heading fields of an authority record, in the order they are looked for.
const HEADING_TAGS: [&[u8; 3]; 8] = [
    b"100", b"110", b"111", b"130", b"148", b"150", b"151", b"155",
];
/// The variant ("see from") fields.
const VARIANT_TAGS: [&[u8; 3]; 8] = [
    b"400", b"410", b"411", b"430", b"448", b"450", b"451", b"455",
];
/// Most variants a list shows (item 14).
const MOST_VARIANTS: usize = 3;

/// What a list of authority drafts shows of a record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorityHeading {
    /// The class, by the heading's tag (item 11).
    pub kind: SubjectKind,
    /// The heading, its subdivisions after ` -- ` (item 10).
    pub heading: String,
    /// The dates of a person (`100 $d`, item 12).
    pub dates: Option<String>,
    /// The control number as a MARC21 `$0`: `(<003>)<001>` (item 13).
    pub identifier: Option<String>,
    /// Up to three variant forms (item 14).
    pub variants: Vec<String>,
}

/// The heading of an authority record; `None` when the record is not an authority
/// (leader 06 other than `z`) or has no heading field (item 9).
#[must_use]
pub fn authority_heading(record: &Record) -> Option<AuthorityHeading> {
    let marc = Marc(record);
    if marc.leader_byte(6) != b'z' {
        return None;
    }
    let (tag, field) = marc.data_in(&HEADING_TAGS).next()?;
    let subject = subject_of(as_subject(*tag), field)?;
    let dates = if tag == b"100" {
        field.text(b'd')
    } else {
        None
    };
    let number = marc.control(*b"001").and_then(text::trimmed);
    let identifier = number.map(
        |number| match marc.control(*b"003").and_then(text::trimmed) {
            Some(organization) => format!("({organization}){number}"),
            None => number,
        },
    );
    let variants = marc
        .data_in(&VARIANT_TAGS)
        .filter_map(|(tag, field)| subject_of(as_subject(*tag), field))
        .map(|variant| variant.heading)
        .take(MOST_VARIANTS)
        .collect();
    Some(AuthorityHeading {
        kind: subject.kind,
        heading: subject.heading,
        dates,
        identifier,
        variants,
    })
}

/// The subject tag that reads like `tag` of an authority record: `100` as `600`.
const fn as_subject(tag: [u8; 3]) -> [u8; 3] {
    let [_, middle, last] = tag;
    [b'6', middle, last]
}
