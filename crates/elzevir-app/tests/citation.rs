// SPDX-License-Identifier: GPL-3.0-or-later
//! The four citation formats over records of the oracle's staff export
//! (docs/specs/catalogo/citacao-e-intercambio.md).
#![expect(
    clippy::unwrap_used,
    reason = "integration test: helper functions fail the test on error"
)]

use elzevir_app::citation::{CitationFormat, cite};
use elzevir_domain::catalog::InstanceId;
use elzevir_marc::{Limits, Record, RecordRef};

/// The first 200 records of the test base, exported by `tools/export.pl` (ISO 2709, with
/// items) on 2026-11-13. Bibliographic data only; no patron data is embedded in `952`.
const STAFF_EXPORT: &[u8] = include_bytes!("data/koha-staff-bibs-1-200.mrc");

/// A book with 490, subjects with 651, one with 264 and 041, one with 856, a periodical.
const SNAPSHOT_POSITIONS: [usize; 5] = [0, 16, 27, 66, 134];

const FORMATS: [CitationFormat; 4] = [
    CitationFormat::Ris,
    CitationFormat::Bibtex,
    CitationFormat::Mods,
    CitationFormat::DublinCore,
];

fn records() -> Vec<Record> {
    STAFF_EXPORT
        .split_inclusive(|&b| b == 0x1d)
        .filter(|raw| raw.len() > 1)
        .map(|raw| {
            let mut record = Record::from_ref(&RecordRef::parse(raw, &Limits::default()).unwrap());
            record.marc21_to_utf8();
            record
        })
        .collect()
}

#[test]
fn every_record_exports_in_the_four_formats() {
    let records = records();
    assert_eq!(records.len(), 200);
    for (position, record) in records.iter().enumerate() {
        let instance = InstanceId(i64::try_from(position).unwrap() + 1);
        let ris = cite(record, instance, None, CitationFormat::Ris);
        assert!(ris.starts_with("TY  - "), "{position}");
        assert!(ris.contains("\r\nTI  - "), "{position}: {ris}");
        assert!(ris.ends_with("ER  - \r\n"), "{position}");
        let bibtex = cite(record, instance, None, CitationFormat::Bibtex);
        assert!(
            bibtex.starts_with(&format!("@book{{{},", instance.0)),
            "{position}"
        );
        assert!(bibtex.contains("\ttitle = {"), "{position}");
        let mods = cite(record, instance, None, CitationFormat::Mods);
        assert!(mods.contains("<titleInfo>"), "{position}");
        assert!(mods.ends_with("</mods>\n"), "{position}");
        let dc = cite(record, instance, None, CitationFormat::DublinCore);
        assert!(dc.contains("<dc:title>"), "{position}");
    }
}

#[test]
fn snapshots_of_chosen_records() {
    let records = records();
    for position in SNAPSHOT_POSITIONS {
        let instance = InstanceId(i64::try_from(position).unwrap() + 1);
        for format in FORMATS {
            let written = cite(&records[position], instance, None, format);
            insta::assert_snapshot!(
                format!(
                    "citation_{position:03}{}",
                    format.file_suffix().replace('.', "_")
                ),
                written.replace("\r\n", "\n")
            );
        }
    }
}
