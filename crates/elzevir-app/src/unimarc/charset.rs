// SPDX-License-Identifier: GPL-3.0-or-later
//! The character set of a UNIMARC record (docs/specs/catalogo/unimarc.md, item 2).

use elzevir_marc::{Field, Record, iso5426_to_utf8};

/// A character set the import does not read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("unsupported UNIMARC character set")]
pub(crate) struct UnsupportedEncoding;

/// Whether `record` is UNIMARC: it has a `200` (title) and no `245`.
pub(crate) fn recognizes(record: &Record) -> bool {
    record.fields.iter().any(|f| f.tag().as_bytes() == b"200")
        && !record.fields.iter().any(|f| f.tag().as_bytes() == b"245")
}

/// Decodes `record` to UTF-8 after the character set `100 $a/26-29` declares: ISO 5426
/// (`01` and `03`) or UTF-8 (`50`, or undeclared when the bytes are UTF-8). Returns the
/// declaration and the decoder used.
///
/// # Errors
///
/// [`UnsupportedEncoding`] for any other set, or for undeclared bytes that are not UTF-8.
pub(crate) fn decode(record: &mut Record) -> Result<(String, String), UnsupportedEncoding> {
    let code = record.fields.iter().find_map(|f| match f {
        Field::Data { tag, subfields, .. } if tag.as_bytes() == b"100" => {
            subfields.iter().find(|s| s.code == b'a')?.value.get(26..30)
        }
        _ => None,
    });
    let valid_utf8 = record.fields.iter().all(|f| match f {
        Field::Control { value, .. } => std::str::from_utf8(value).is_ok(),
        Field::Data { subfields, .. } => subfields
            .iter()
            .all(|s| std::str::from_utf8(&s.value).is_ok()),
    });
    let legacy = match code {
        Some(v)
            if v.chunks_exact(2)
                .all(|c| matches!(c, b"01" | b"03" | b"  "))
                && v.chunks_exact(2).any(|c| c == b"03") =>
        {
            true
        }
        Some(b"50  " | b"    ") | None if valid_utf8 => false,
        _ => return Err(UnsupportedEncoding),
    };
    let declared = code.map_or_else(
        || "undeclared".into(),
        |v| String::from_utf8_lossy(v).into_owned(),
    );

    if legacy {
        for field in &mut record.fields {
            match field {
                Field::Control { value, .. } => {
                    *value = iso5426_to_utf8(value).into_owned().into_bytes();
                }
                Field::Data { subfields, .. } => {
                    for s in subfields {
                        s.value = iso5426_to_utf8(&s.value).into_owned().into_bytes();
                    }
                }
            }
        }
        declare_utf8(record);
    }
    record.leader[9] = b'a';
    Ok((declared, if legacy { "ISO 5426" } else { "UTF-8" }.into()))
}

/// Marks `record` as UTF-8 in `100 $a/26-29`.
pub(crate) fn declare_utf8(record: &mut Record) {
    // Mark the working record as UTF-8. Keep the original declaration in evidence.
    for f in &mut record.fields {
        if let Field::Data { tag, subfields, .. } = f
            && tag.as_bytes() == b"100"
        {
            for s in subfields.iter_mut().filter(|s| s.code == b'a') {
                if let Some(code) = s.value.get_mut(26..30) {
                    code.copy_from_slice(b"50  ");
                }
            }
        }
    }
}
