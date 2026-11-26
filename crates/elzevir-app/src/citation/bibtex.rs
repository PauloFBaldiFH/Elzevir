// SPDX-License-Identifier: GPL-3.0-or-later
//! BibTeX (docs/specs/catalogo/citacao-e-intercambio.md, items 8 and 9).
//!
//! Koha: `C4::Record::marc2bibtex`, MARC21 branch, without
//! `BibtexExportAdditionalFields`.

use elzevir_domain::catalog::InstanceId;
use elzevir_marc::Record;

use super::subfield;

/// The name fields of `author`, each read once, like `MARC::Record::subfield`.
const AUTHORS: [&[u8; 3]; 6] = [b"100", b"110", b"111", b"700", b"710", b"711"];

pub(super) fn bibtex(record: &Record, instance: InstanceId, deposit_url: Option<&str>) -> String {
    let first_of = |choices: &[(&[u8; 3], u8)]| {
        choices
            .iter()
            .map(|(tag, code)| subfield(record, **tag, *code))
            .find(|value| !value.is_empty())
            .unwrap_or_default()
    };
    let author = AUTHORS
        .iter()
        .map(|tag| subfield(record, **tag, b'a'))
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>()
        .join(" and ");
    let url = first_of(&[(b"856", b'u')]);
    let url = if url.is_empty() {
        deposit_url.unwrap_or_default().to_owned()
    } else {
        url
    };
    let entries = [
        ("author", author),
        ("title", first_of(&[(b"245", b'a')])),
        ("editor", first_of(&[(b"260", b'f')])),
        ("publisher", first_of(&[(b"264", b'b'), (b"260", b'b')])),
        (
            "year",
            first_of(&[(b"264", b'c'), (b"260", b'c'), (b"260", b'g')]),
        ),
        ("series", first_of(&[(b"490", b'a')])),
        ("address", first_of(&[(b"264", b'a'), (b"260", b'a')])),
        ("edition", first_of(&[(b"250", b'a')])),
        ("note", first_of(&[(b"500", b'a')])),
        ("url", url),
    ];
    let mut lines = vec![instance.0.to_string()];
    lines.extend(
        entries
            .iter()
            .filter(|(_, value)| !value.is_empty())
            .map(|(name, value)| format!("\t{name} = {{{}}}", escaped(value))),
    );
    format!("@book{{{}\n}}\n", lines.join(",\n"))
}

/// `value` with its braces escaped, so that none closes the field (item 9).
fn escaped(value: &str) -> String {
    value.replace('{', "\\{").replace('}', "\\}")
}
