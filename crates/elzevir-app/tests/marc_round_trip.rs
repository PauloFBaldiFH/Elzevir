// SPDX-License-Identifier: GPL-3.0-or-later
//! Import → export round trips over the oracle's staff export
//! (docs/specs/catalogo/mapeamento-marc21.md, §7; vision A.4, item 3).
#![expect(
    clippy::unwrap_used,
    reason = "integration test: helper functions fail the test on error"
)]

use std::collections::BTreeMap;

use elzevir_app::catalog::{ItemData, NewRecord};
use elzevir_app::marc_mapping::{bibframe_to_marc, marc_to_bibframe};
use elzevir_marc::{Field, Limits, Record, RecordRef};

const STAFF_EXPORT: &[u8] = include_bytes!("data/koha-staff-bibs-1-200.mrc");

/// Raw bytes and the decoded record of each exported record.
fn records() -> Vec<(Vec<u8>, Record)> {
    STAFF_EXPORT
        .split_inclusive(|&b| b == 0x1d)
        .filter(|raw| raw.len() > 1)
        .map(|raw| {
            let mut record = Record::from_ref(&RecordRef::parse(raw, &Limits::default()).unwrap());
            record.marc21_to_utf8();
            (raw.to_vec(), record)
        })
        .collect()
}

fn iso2709(record: &Record) -> Vec<u8> {
    let mut out = Vec::new();
    record.write_iso2709(&mut out).unwrap();
    out
}

fn tags(record: &Record, tag: [u8; 3]) -> Vec<Field> {
    record
        .fields
        .iter()
        .filter(|field| *field.tag().as_bytes() == tag)
        .cloned()
        .collect()
}

fn without(record: &Record, tag: [u8; 3]) -> Vec<Field> {
    record
        .fields
        .iter()
        .filter(|field| *field.tag().as_bytes() != tag)
        .cloned()
        .collect()
}

fn model(record: &Record) -> NewRecord {
    marc_to_bibframe(record, None).unwrap()
}

#[test]
fn unedited_records_come_back_byte_for_byte() {
    for (position, (raw, record)) in records().iter().enumerate() {
        let exported = bibframe_to_marc(&model(record), Some(record));
        assert_eq!(&exported, record, "record {position}");
        assert_eq!(
            &iso2709(&exported),
            raw,
            "record {position}: ISO 2709 bytes"
        );
    }
}

#[test]
fn a_record_written_from_scratch_maps_back_to_the_same_model() {
    for (position, (_, record)) in records().iter().enumerate() {
        let original = model(record);
        let written = bibframe_to_marc(&original, None);
        let again = marc_to_bibframe(&written, None).unwrap();
        assert_eq!(again, original, "record {position}");
    }
}

#[test]
fn editing_the_title_touches_only_the_245() {
    let (_, record) = &records()[18];
    let mut edited = model(record);
    edited.instance.title = "A presença inquietante : edição revista".to_owned();
    let exported = bibframe_to_marc(&edited, Some(record));
    assert_eq!(without(&exported, *b"245"), without(record, *b"245"));
    assert_eq!(model(&exported).instance.title, edited.instance.title);
    // The statement of responsibility ($c) stays.
    let Field::Data { subfields, .. } = &tags(&exported, *b"245")[0] else {
        panic!("245 is a data field");
    };
    assert!(subfields.iter().any(|s| s.code == b'c'));
}

#[test]
fn editing_one_item_keeps_the_others_and_koha_subfields() {
    let records = records();
    let (position, (_, record)) = records
        .iter()
        .enumerate()
        .find(|(_, (_, r))| tags(r, *b"952").len() >= 3)
        .unwrap();
    let mut edited = model(record);
    edited.items[1].call_number = Some("320 X99".to_owned());
    let exported = bibframe_to_marc(&edited, Some(record));
    let (before, after) = (tags(record, *b"952"), tags(&exported, *b"952"));
    assert_eq!(after.len(), before.len(), "record {position}");
    assert_eq!(after[0], before[0]);
    assert_eq!(after[2..], before[2..]);
    let (Field::Data { subfields: old, .. }, Field::Data { subfields: new, .. }) =
        (&before[1], &after[1])
    else {
        panic!("952 is a data field");
    };
    let changed: Vec<u8> = old
        .iter()
        .zip(new)
        .filter(|(a, b)| a != b)
        .map(|(a, _)| a.code)
        .collect();
    assert_eq!(changed, [b'o'], "only $o changes; $9 and $6 stay");
    assert_eq!(without(&exported, *b"952"), without(record, *b"952"));
}

#[test]
fn a_new_item_becomes_a_new_952_after_the_others() {
    let (_, record) = &records()[0];
    let mut edited = model(record);
    let mut item: ItemData = edited.items[0].clone();
    item.barcode = Some("NOVO-1".to_owned());
    item.inventory_number = Some("2026/0001".to_owned());
    edited.items.push(item.clone());
    let exported = bibframe_to_marc(&edited, Some(record));
    let items = tags(&exported, *b"952");
    assert_eq!(items.len(), tags(record, *b"952").len() + 1);
    assert_eq!(model(&exported).items.last(), Some(&item));
    assert!(matches!(exported.fields.last(), Some(f) if f.tag().as_bytes() >= b"952"));
}

#[test]
fn accents_and_call_numbers_survive_an_edit_elsewhere() {
    for (position, (_, record)) in records().iter().enumerate() {
        let mut edited = model(record);
        edited.summary = Some("Resumo acrescentado à mão".to_owned());
        let exported = bibframe_to_marc(&edited, Some(record));
        for tag in [b"082", b"090", b"245", b"100", b"650", b"952"] {
            assert_eq!(
                tags(&exported, *tag),
                tags(record, *tag),
                "record {position}, {tag:?}"
            );
        }
        assert_eq!(model(&exported).summary, edited.summary);
    }
}

#[test]
fn editing_a_list_keeps_untouched_fields_with_their_authority_links() {
    let (_, record) = &records()[18];
    let mut edited = model(record);
    // Drop the first subject and the main author's role; the added author keeps its links.
    edited.subjects.remove(0);
    edited.contributions[0].role = Some("edt".to_owned());
    let exported = bibframe_to_marc(&edited, Some(record));
    assert_eq!(tags(&exported, *b"650"), tags(record, *b"650")[1..]);
    assert_eq!(tags(&exported, *b"700"), tags(record, *b"700"));
    let again = model(&exported);
    assert_eq!(again.subjects, edited.subjects);
    assert_eq!(again.contributions, edited.contributions);
}

#[test]
fn notes_map_both_ways_and_an_edited_note_touches_only_its_field() {
    use elzevir_app::catalog::NewNote;
    use elzevir_domain::catalog::NoteKind;

    let records = records();
    let with_notes = records
        .iter()
        .filter(|(_, record)| !model(record).instance.notes.is_empty())
        .count();
    assert!(with_notes > 0, "the staff export has notes");
    // Record 147 has a contents note with "Karl Marx" (docs/specs/catalogo/busca.md, §4).
    let (_, record) = &records[146];
    let mut edited = model(record);
    let contents = edited
        .instance
        .notes
        .iter_mut()
        .find(|note| note.kind == NoteKind::Contents)
        .unwrap();
    assert!(contents.text.contains("Marx"));
    contents.text = "Nota de conteúdo reescrita.".to_owned();
    edited.instance.notes.push(NewNote {
        kind: NoteKind::Local,
        text: "Doação da família.".to_owned(),
    });
    let exported = bibframe_to_marc(&edited, Some(record));
    for tag in [*b"500", *b"505", *b"590"] {
        let others = |r: &Record| {
            r.fields
                .iter()
                .filter(|f| ![*b"500", *b"505", *b"590"].contains(f.tag().as_bytes()))
                .cloned()
                .collect::<Vec<_>>()
        };
        assert_eq!(
            others(&exported),
            others(record),
            "only notes change ({tag:?})"
        );
    }
    assert_eq!(tags(&exported, *b"500"), tags(record, *b"500"));
    assert_eq!(model(&exported).instance.notes, edited.instance.notes);
}

/// Why a field written from the model alone differs from the oracle's.
#[derive(Clone, Copy)]
enum Gap {
    /// A local field with no BIBFRAME destination: it stays only in the source record.
    Local,
    /// Koha's own data, written by the Koha edge (item 30 of bibframe-completo.md).
    Koha,
    /// Still to be closed by a later part of stage 5f.
    Pending(&'static str),
}

impl Gap {
    fn reason(self) -> &'static str {
        match self {
            Self::Local => "local field, kept only in the source record",
            Self::Koha => "Koha data, written by the Koha edge",
            Self::Pending(reason) => reason,
        }
    }
}

/// What the export from the model alone still writes differently: the tag (or `LDR/nn`
/// for a Leader position, `order` for the order of the tags), the number of the oracle's
/// records where it differs and why. The list may only shrink: a fix lowers a count or
/// removes a line, and the test fails until the list says so.
const KNOWN_DIFFERENCES: &[(&str, usize, Gap)] = &[
    ("LDR/17", 133, Gap::Pending("encoding level")),
    ("LDR/18", 186, Gap::Pending("descriptive cataloguing form")),
    (
        "007",
        8,
        Gap::Pending("physical description fixed field not mapped"),
    ),
    (
        "008",
        200,
        Gap::Pending("fill characters and positions 18-39 not mapped"),
    ),
    ("012", 16, Gap::Local),
    (
        "016",
        1,
        Gap::Pending("national bibliography agency number not mapped"),
    ),
    (
        "020",
        49,
        Gap::Pending("a qualifier written in $a comes back in $q"),
    ),
    ("022", 1, Gap::Pending("first indicator")),
    ("039", 3, Gap::Local),
    (
        "040",
        93,
        Gap::Pending("$c written equal to $a when the source has none"),
    ),
    (
        "041",
        172,
        Gap::Pending("first indicator 0 where the source leaves it blank"),
    ),
    (
        "043",
        2,
        Gap::Pending("codes come back in lowercase and padded"),
    ),
    (
        "044",
        4,
        Gap::Pending("country of producing entity not mapped"),
    ),
    ("045", 3, Gap::Pending("time period of content not mapped")),
    ("050", 1, Gap::Pending("LC classification not mapped")),
    ("080", 19, Gap::Pending("UDC not mapped")),
    ("082", 153, Gap::Pending("indicators")),
    ("083", 1, Gap::Pending("additional Dewey number not mapped")),
    ("084", 1, Gap::Pending("other classification not mapped")),
    ("090", 33, Gap::Local),
    ("092", 51, Gap::Local),
    ("094", 1, Gap::Local),
    ("098", 1, Gap::Local),
    (
        "100",
        154,
        Gap::Pending("first indicator and subfields joined into $a"),
    ),
    (
        "110",
        8,
        Gap::Pending("first indicator and subfields joined into $a"),
    ),
    ("111", 1, Gap::Pending("subfields joined into $a")),
    (
        "130",
        1,
        Gap::Pending("a uniform title main entry comes back as 240"),
    ),
    ("210", 1, Gap::Pending("ISBD punctuation")),
    (
        "240",
        15,
        Gap::Pending("indicators, $l and ISBD punctuation; a 130 comes back here"),
    ),
    (
        "243",
        3,
        Gap::Pending("indicators, $l and ISBD punctuation"),
    ),
    ("245", 199, Gap::Pending("indicators and ISBD punctuation")),
    ("246", 5, Gap::Pending("indicators")),
    ("250", 23, Gap::Pending("ISBD punctuation")),
    (
        "260",
        190,
        Gap::Pending("the publication comes back as 264"),
    ),
    (
        "264",
        192,
        Gap::Pending("second indicator 1 and ISBD punctuation"),
    ),
    ("290", 1, Gap::Local),
    ("300", 87, Gap::Pending("ISBD punctuation")),
    ("334", 12, Gap::Pending("mode of issuance not written")),
    ("336", 13, Gap::Pending("content type not written")),
    ("337", 13, Gap::Pending("$0 not kept")),
    ("338", 13, Gap::Pending("$0 not kept")),
    (
        "362",
        2,
        Gap::Pending("serials: dates of publication not mapped"),
    ),
    ("380", 1, Gap::Pending("form of work not mapped")),
    ("440", 3, Gap::Pending("indicators and ISBD punctuation")),
    ("490", 38, Gap::Pending("first indicator")),
    ("504", 2, Gap::Pending("repeated $a joined")),
    ("505", 3, Gap::Pending("first indicator")),
    ("520", 6, Gap::Pending("ISBD punctuation")),
    ("541", 2, Gap::Pending("source of acquisition not mapped")),
    ("555", 1, Gap::Pending("first indicator")),
    ("590", 1, Gap::Pending("$8 not kept")),
    ("593", 1, Gap::Local),
    ("595", 14, Gap::Local),
    ("596", 1, Gap::Local),
    ("600", 12, Gap::Pending("subfields joined into $a")),
    (
        "610",
        3,
        Gap::Pending("subject of an organization not written"),
    ),
    ("630", 1, Gap::Pending("ISBD punctuation")),
    ("650", 155, Gap::Pending("second indicator")),
    ("651", 3, Gap::Pending("$y comes back as $x")),
    ("655", 1, Gap::Pending("ISBD punctuation")),
    ("690", 2, Gap::Local),
    ("695", 2, Gap::Local),
    (
        "700",
        40,
        Gap::Pending("subfields joined into $a; $0 and $1 not kept"),
    ),
    (
        "710",
        8,
        Gap::Pending("first indicator and subfields joined into $a"),
    ),
    (
        "740",
        2,
        Gap::Pending("first indicator and ISBD punctuation"),
    ),
    ("775", 2, Gap::Pending("ISBD punctuation")),
    ("830", 8, Gap::Pending("ISBD punctuation")),
    (
        "850",
        1,
        Gap::Pending("serials: holding institution not mapped"),
    ),
    ("852", 51, Gap::Local),
    (
        "853",
        1,
        Gap::Pending("serials: captions and pattern not mapped"),
    ),
    (
        "855",
        1,
        Gap::Pending("serials: captions and pattern not mapped"),
    ),
    (
        "856",
        15,
        Gap::Pending("$z and $y of an 856 4# stay in the source record"),
    ),
    ("942", 200, Gap::Koha),
    ("949", 1, Gap::Local),
    ("952", 199, Gap::Koha),
    ("990", 2, Gap::Local),
    ("999", 200, Gap::Koha),
];

/// What differs between `original` and `written`, by the keys of [`KNOWN_DIFFERENCES`].
fn differences(original: &Record, written: &Record) -> Vec<String> {
    // Positions 00-04 and 12-16 are lengths and addresses, computed when writing.
    let mut found: Vec<String> = (5..12)
        .chain(17..24)
        .filter(|&i| original.leader.get(i) != written.leader.get(i))
        .map(|i| format!("LDR/{i:02}"))
        .collect();
    let mut all: Vec<[u8; 3]> = original
        .fields
        .iter()
        .chain(&written.fields)
        .map(|field| *field.tag().as_bytes())
        .collect();
    all.sort_unstable();
    all.dedup();
    found.extend(
        all.into_iter()
            .filter(|&tag| tags(original, tag) != tags(written, tag))
            .map(|tag| String::from_utf8_lossy(&tag).into_owned()),
    );
    // The tags both records have, in the order each writes them.
    let shared = |one: &Record, other: &Record| {
        let mut order: Vec<[u8; 3]> = one
            .fields
            .iter()
            .map(|field| *field.tag().as_bytes())
            .filter(|tag| other.fields.iter().any(|f| f.tag().as_bytes() == tag))
            .collect();
        order.dedup();
        order
    };
    if shared(original, written) != shared(written, original) {
        found.push("order".to_owned());
    }
    found
}

#[test]
fn records_written_from_the_model_alone_differ_only_where_listed() {
    let mut found: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for (position, (_, record)) in records().iter().enumerate() {
        let written = bibframe_to_marc(&model(record), None);
        for key in differences(record, &written) {
            found.entry(key).or_insert((0, position)).0 += 1;
        }
    }
    let mut problems = Vec::new();
    for (key, count, gap) in KNOWN_DIFFERENCES {
        match found.remove(*key) {
            Some((actual, _)) if actual == *count => {}
            Some((actual, first)) if actual > *count => problems.push(format!(
                "{key}: {actual} records differ, {count} listed ({}); first: {first}",
                gap.reason()
            )),
            Some((actual, _)) => {
                problems.push(format!("{key}: down to {actual} records, lower the list"));
            }
            None => problems.push(format!("{key}: no longer differs, remove it from the list")),
        }
    }
    problems.extend(found.into_iter().map(|(key, (actual, first))| {
        format!("{key}: {actual} records differ, not listed; first: {first}")
    }));
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
