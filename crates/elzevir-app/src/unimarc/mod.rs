// SPDX-License-Identifier: GPL-3.0-or-later
//! UNIMARC at the edge (docs/specs/catalogo/unimarc.md). A bibliographic UNIMARC record is
//! read and written through its MARC21 projection, so that one mapping, MARC21 to
//! BIBFRAME, describes the model, and one table per field says how the two families
//! correspond.

mod charset;
mod coded;
mod fields;
#[cfg(test)]
mod tests;

pub(crate) use charset::{declare_utf8, decode, recognizes};
use elzevir_marc::{Field, Record};

/// Control fields both families share: the record number and the last change.
const SHARED_CONTROL: [&[u8; 3]; 2] = [b"001", b"005"];

/// The MARC21 projection of a UTF-8 UNIMARC record.
#[must_use]
pub fn to_marc21(record: &Record) -> Record {
    let coded = coded::marc21_fields(record);
    let mut fields: Vec<Field> = shared_control(record);
    fields.extend(coded.fields);
    for field in &record.fields {
        if let Field::Data {
            tag,
            indicators,
            subfields,
        } = field
            && let Some(field) = fields::to_marc21(*tag, *indicators, subfields)
        {
            fields.push(field);
        }
    }
    fields.sort_by_key(|field| *field.tag().as_bytes());
    Record {
        leader: coded::marc21_leader(&record.leader, coded.aacr),
        fields,
    }
}

/// `record`, a UTF-8 MARC21 record, as UNIMARC; fields UNIMARC has no place for are left
/// out (item 14).
#[must_use]
pub fn from_marc21(record: &Record) -> Record {
    let mut fields: Vec<Field> = shared_control(record);
    fields.extend(coded::unimarc_fields(record));
    for field in &record.fields {
        if let Field::Data {
            tag,
            indicators,
            subfields,
        } = field
            && let Some(field) = fields::to_unimarc(*tag, *indicators, subfields)
        {
            fields.push(field);
        }
    }
    fields.sort_by_key(|field| *field.tag().as_bytes());
    Record {
        leader: coded::unimarc_leader(&record.leader),
        fields,
    }
}

/// Coded fields the projection reads besides the data fields of the table.
const CODED: [&[u8; 3]; 6] = [b"100", b"101", b"102", b"105", b"106", b"801"];

/// Adds to `out` the fields of `original`, a UTF-8 UNIMARC record, that the projection
/// does not read, so that they leave as they came (item 19).
pub(crate) fn keep_unread(out: &mut Record, original: &Record) {
    let unread = original.fields.iter().filter(|field| match field {
        Field::Control { tag, .. } => !SHARED_CONTROL.contains(&tag.as_bytes()),
        Field::Data {
            tag,
            indicators,
            subfields,
        } => {
            !CODED.contains(&tag.as_bytes())
                && fields::to_marc21(*tag, *indicators, subfields).is_none()
        }
    });
    out.fields.extend(unread.cloned());
    out.fields.sort_by_key(|field| *field.tag().as_bytes());
}

fn shared_control(record: &Record) -> Vec<Field> {
    record
        .fields
        .iter()
        .filter(|field| SHARED_CONTROL.contains(&field.tag().as_bytes()))
        .cloned()
        .collect()
}
