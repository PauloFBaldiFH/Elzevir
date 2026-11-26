// SPDX-License-Identifier: GPL-3.0-or-later
//! A bibliographic UNIMARC projection. The unmodified ISO record remains available
//! even for fields without a MARC21 equivalent.
use super::RecordFailure;
use crate::catalog::{NewRecord, OriginalRecord};
use crate::marc_mapping::marc_to_bibframe;
use elzevir_marc::Record;

pub(super) fn recognizes(record: &Record) -> bool {
    crate::unimarc::recognizes(record)
}

pub(super) fn map(mut record: Record, bytes: Vec<u8>) -> Result<NewRecord, RecordFailure> {
    let (declared, decoded) = decode(&mut record)?;
    let mapped = crate::unimarc::to_marc21(&record);
    let mut normalized = Vec::new();
    mapped
        .write_iso2709(&mut normalized)
        .map_err(RecordFailure::Source)?;
    let mut model = marc_to_bibframe(&mapped, Some(normalized)).map_err(RecordFailure::Mapping)?;
    model.instance.original = Some(OriginalRecord {
        serialization: "ISO 2709".into(),
        bytes,
        declared,
        decoded,
        family: "UNIMARC".into(),
    });
    Ok(model)
}

pub(super) fn decode(record: &mut Record) -> Result<(String, String), RecordFailure> {
    crate::unimarc::decode(record).map_err(|_| RecordFailure::Encoding)
}

pub(super) fn declare_utf8(record: &mut Record) {
    crate::unimarc::declare_utf8(record);
}
