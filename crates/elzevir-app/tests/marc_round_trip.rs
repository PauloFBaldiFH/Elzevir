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
fn edited_classes_write_the_leader_and_an_electronic_edition_its_007() {
    let records = records();
    let (_, record) = records
        .iter()
        .find(|(_, record)| tags(record, *b"007").is_empty())
        .unwrap();
    let mut edited = model(record);
    edited.work_description.classes = vec!["NotatedMusic".to_owned(), "Serial".to_owned()];
    edited.instance.description.classes = vec!["Electronic".to_owned()];
    let exported = bibframe_to_marc(&edited, Some(record));
    assert_eq!(&exported.leader[6..8], b"cs");
    assert_eq!(&exported.leader[..6], &record.leader[..6]);
    assert!(tags(&exported, *b"007").iter().any(|field| matches!(
        field,
        Field::Control { value, .. } if value.as_slice() == b"cr"
    )));
    let again = model(&exported);
    assert_eq!(again.work_description.classes, ["NotatedMusic", "Serial"]);
    assert_eq!(again.instance.description.classes, ["Electronic"]);
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
    /// The source leaves blank an indicator that MARC defines; the export writes it.
    Blank,
    /// The source holds fill characters (`|`, no attempt to code); the export writes what
    /// the LC way back writes there.
    Fill,
    /// A standard field that the LC conversions do not read: BIBFRAME has no place for it,
    /// so it stays only in the source record.
    NoDestination(&'static str),
    /// The source breaks the MARC format there; the export writes it right.
    Source(&'static str),
    /// Only the ISBD punctuation differs: the export writes it as the LC way back does.
    Punctuation,
    /// Still to be closed by a later part of stage 5f.
    Pending(&'static str),
}

impl Gap {
    fn reason(self) -> &'static str {
        match self {
            Self::Local => "local field, kept only in the source record",
            Self::Koha => "Koha data, written by the Koha edge",
            Self::Blank => "blank indicator in the source",
            Self::Fill => "fill characters in the source",
            Self::Punctuation => "ISBD punctuation only",
            Self::NoDestination(reason) | Self::Source(reason) | Self::Pending(reason) => reason,
        }
    }
}

/// What the export from the model alone still writes differently: the key of
/// [`field_differences`] (or `LDR/nn` for a Leader position, `008/nn-nn` for a group of
/// [`fixed_differences`], `order` for the order of the
/// tags), the number of the oracle's
/// records where it differs and why. The list may only shrink: a fix lowers a count or
/// removes a line, and the test fails until the list says so.
const KNOWN_DIFFERENCES: &[(&str, usize, Gap)] = &[
    ("LDR/17", 133, Gap::Pending("encoding level")),
    (
        "007",
        8,
        Gap::NoDestination(
            "text (t): the LC conversion reads nothing of it, and its way back writes t| for \
             every printed resource",
        ),
    ),
    (
        "008 length",
        29,
        Gap::Source("the field does not have its 40 positions"),
    ),
    (
        "008/00-05",
        2,
        Gap::Source("the date entered is not a date"),
    ),
    (
        "008/06",
        1,
        Gap::Pending("a type of date n (unknown) with dates comes back as m"),
    ),
    (
        "008/11-14",
        16,
        Gap::Source("a second date that the type of date does not take (0000, 9999, ----)"),
    ),
    ("008/11-14 fill", 80, Gap::Fill),
    (
        "008/18-21",
        4,
        Gap::Source("hyphens as fill characters; a field out of place"),
    ),
    ("008/18-21 fill", 80, Gap::Fill),
    ("008/22", 2, Gap::Source("a hyphen as fill character")),
    ("008/22 fill", 81, Gap::Fill),
    (
        "008/23",
        16,
        Gap::Pending(
            "form of item r (regular print reproduction) not kept, and the LC way back \
             never writes it; hyphens as fill characters",
        ),
    ),
    ("008/23 fill", 81, Gap::Fill),
    (
        "008/24-27",
        6,
        Gap::Pending(
            "a biography also gives h and law codes come back as g, as the LC way back \
             writes them; hyphens and a field out of place in the source",
        ),
    ),
    ("008/24-27 fill", 81, Gap::Fill),
    (
        "008/28",
        4,
        Gap::Pending("government publication: only an LC extension (bflc) holds it"),
    ),
    ("008/28 fill", 81, Gap::Fill),
    (
        "008/29-31",
        8,
        Gap::Source("blanks where a book takes 0 or 1; a field out of place"),
    ),
    ("008/29-31 fill", 80, Gap::Fill),
    (
        "008/32",
        5,
        Gap::Source("the undefined position filled; a field out of place"),
    ),
    (
        "008/33",
        4,
        Gap::Source("blank where a book takes a code; a field out of place"),
    ),
    ("008/33 fill", 82, Gap::Fill),
    (
        "008/34",
        15,
        Gap::Pending(
            "d (contains biographical information): the LC conversion does not read it; \
             fields out of place",
        ),
    ),
    ("008/34 fill", 80, Gap::Fill),
    (
        "008/35-37",
        4,
        Gap::Source("a language code in uppercase or out of place"),
    ),
    ("008/38", 2, Gap::Source("a hyphen as fill character")),
    ("008/38 fill", 80, Gap::Fill),
    (
        "008/39",
        100,
        Gap::Pending("cataloguing source: the LC conversion does not read it and writes blank"),
    ),
    ("008/39 fill", 80, Gap::Fill),
    ("012", 16, Gap::Local),
    ("016/ind1 blank", 1, Gap::Blank),
    (
        "020",
        49,
        Gap::Pending("a qualifier written in $a comes back in $q"),
    ),
    (
        "022/ind1",
        1,
        Gap::Pending("level of international interest not mapped"),
    ),
    ("039", 3, Gap::Local),
    (
        "040",
        93,
        Gap::Pending("$c written equal to $a when the source has none"),
    ),
    (
        "041",
        84,
        Gap::Pending(
            "a single language, already in 008, written in 041 by the source; $c not kept",
        ),
    ),
    (
        "043",
        2,
        Gap::Pending("codes come back in lowercase and padded"),
    ),
    ("044", 4, Gap::NoDestination("country of producing entity")),
    (
        "045",
        1,
        Gap::Source("the field repeated, which MARC does not allow; the export joins them"),
    ),
    (
        "082",
        11,
        Gap::Pending("$2 written before $a in the source; a local $d not kept"),
    ),
    ("082/ind2 blank", 88, Gap::Blank),
    ("083", 1, Gap::NoDestination("additional Dewey number")),
    ("090", 33, Gap::Local),
    ("092", 51, Gap::Local),
    ("094", 1, Gap::Local),
    ("098", 1, Gap::Local),
    ("100", 1, Gap::Pending("$2 source of the heading not kept")),
    ("100 punctuation", 25, Gap::Punctuation),
    (
        "100/ind1",
        3,
        Gap::Pending("type of personal name (forename) not mapped"),
    ),
    ("100/ind1 blank", 73, Gap::Blank),
    (
        "110/ind1",
        2,
        Gap::Pending("type of corporate name (jurisdiction) not mapped"),
    ),
    ("110/ind1 blank", 6, Gap::Blank),
    ("111 punctuation", 1, Gap::Punctuation),
    (
        "130",
        1,
        Gap::Pending("a uniform title main entry comes back as 240"),
    ),
    ("210 punctuation", 1, Gap::Punctuation),
    (
        "240",
        14,
        Gap::Pending("$l not kept; a 130 comes back here"),
    ),
    ("240 punctuation", 1, Gap::Punctuation),
    (
        "240/ind1",
        9,
        Gap::Pending("printed or displayed not mapped"),
    ),
    (
        "240/ind2",
        4,
        Gap::Pending("nonfiling characters not mapped"),
    ),
    ("243", 3, Gap::Pending("$l not kept")),
    (
        "243/ind1",
        3,
        Gap::Pending("printed or displayed not mapped"),
    ),
    (
        "243/ind2",
        1,
        Gap::Pending("nonfiling characters not mapped"),
    ),
    ("245 punctuation", 99, Gap::Punctuation),
    (
        "245/ind1",
        3,
        Gap::Pending("added entry decided as the LC way back does; these sources differ"),
    ),
    ("245/ind1 blank", 80, Gap::Blank),
    (
        "245/ind2",
        40,
        Gap::Pending("nonfiling characters not mapped"),
    ),
    ("245/ind2 blank", 1, Gap::Blank),
    ("246 punctuation", 4, Gap::Punctuation),
    (
        "246/ind1",
        5,
        Gap::Pending("note and added entry control not mapped"),
    ),
    ("246/ind2", 1, Gap::Pending("type of title not kept")),
    ("250 punctuation", 23, Gap::Punctuation),
    (
        "260",
        11,
        Gap::Pending(
            "places and publishers joined lose their pairs; $b and $c swapped in the source; \
             a 260 of a record not under AACR 2 comes back as 264",
        ),
    ),
    ("260 punctuation", 72, Gap::Punctuation),
    (
        "260/ind1",
        2,
        Gap::Pending("sequence of publishing statements not mapped"),
    ),
    (
        "264",
        4,
        Gap::Pending("a 260 of a record not under AACR 2 comes back as 264"),
    ),
    ("264 punctuation", 10, Gap::Punctuation),
    ("290", 1, Gap::Local),
    ("300 punctuation", 87, Gap::Punctuation),
    (
        "334",
        13,
        Gap::Pending("$0 (RDA registry IRI) not kept; sources without 334 or $2"),
    ),
    ("336", 13, Gap::Pending("$0 not kept; labels in Portuguese")),
    ("337", 13, Gap::Pending("$0 not kept; labels in Portuguese")),
    ("338", 13, Gap::Pending("$0 not kept")),
    (
        "362",
        2,
        Gap::Pending("serials: dates of publication not mapped"),
    ),
    ("440 punctuation", 2, Gap::Punctuation),
    (
        "440/ind2",
        3,
        Gap::Pending("nonfiling characters not mapped"),
    ),
    (
        "490",
        6,
        Gap::Pending("a 490 with only $v is lost; a 440 also comes back as 490"),
    ),
    ("490 punctuation", 7, Gap::Punctuation),
    (
        "490/ind1",
        14,
        Gap::Pending("series traced differently not derived from the series relation"),
    ),
    ("490/ind1 blank", 18, Gap::Blank),
    (
        "490/ind2",
        2,
        Gap::Pending("not defined in MARC; the source fills it"),
    ),
    ("504", 1, Gap::Pending("repeated $a joined")),
    (
        "504/ind2",
        1,
        Gap::Pending("not defined in MARC; the source fills it"),
    ),
    (
        "505/ind1",
        3,
        Gap::Pending("display constant (partial contents) not mapped"),
    ),
    ("520 punctuation", 6, Gap::Punctuation),
    (
        "541",
        2,
        Gap::Pending("source of acquisition: an Item property, with the items of the Koha edge"),
    ),
    ("555/ind1", 1, Gap::Pending("display constant not mapped")),
    ("590", 1, Gap::Pending("$8 not kept")),
    ("593", 1, Gap::Local),
    ("595", 14, Gap::Local),
    ("596", 1, Gap::Local),
    ("600 punctuation", 1, Gap::Punctuation),
    (
        "600/ind1",
        3,
        Gap::Pending("type of personal name (forename) not mapped"),
    ),
    (
        "610/ind1",
        2,
        Gap::Pending("type of corporate name (jurisdiction) not mapped"),
    ),
    ("630 punctuation", 1, Gap::Punctuation),
    ("650", 1, Gap::Pending("a field without a heading, only $2")),
    ("650 punctuation", 34, Gap::Punctuation),
    ("650/ind1", 90, Gap::Pending("level of subject not mapped")),
    ("650/ind2 blank", 44, Gap::Blank),
    ("651 punctuation", 1, Gap::Punctuation),
    ("655 punctuation", 1, Gap::Punctuation),
    ("690", 2, Gap::Local),
    ("695", 2, Gap::Local),
    ("700", 1, Gap::Pending("$2 source of the heading not kept")),
    ("700 punctuation", 23, Gap::Punctuation),
    (
        "700/ind1",
        1,
        Gap::Pending("type of personal name (forename) not mapped"),
    ),
    ("700/ind1 blank", 1, Gap::Blank),
    (
        "710",
        2,
        Gap::Pending("$k not kept; a repeated $a joins the $b"),
    ),
    ("710 punctuation", 4, Gap::Punctuation),
    (
        "710/ind1",
        2,
        Gap::Pending("type of corporate name (jurisdiction) not mapped"),
    ),
    ("710/ind1 blank", 1, Gap::Blank),
    ("740 punctuation", 2, Gap::Punctuation),
    (
        "740/ind1",
        2,
        Gap::Pending("nonfiling characters not mapped"),
    ),
    ("775 punctuation", 2, Gap::Punctuation),
    ("830 punctuation", 6, Gap::Punctuation),
    (
        "830/ind2",
        2,
        Gap::Pending("nonfiling characters not mapped"),
    ),
    ("830/ind2 blank", 1, Gap::Blank),
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

/// The subfields of `field` without the ISBD punctuation and spaces around each value.
fn without_punctuation(field: &Field) -> Vec<(u8, String)> {
    let Field::Data { subfields, .. } = field else {
        return vec![(0, format!("{field:?}"))];
    };
    subfields
        .iter()
        .map(|subfield| {
            let text = String::from_utf8_lossy(&subfield.value);
            let text = text.trim_matches([' ', '.', ',', ':', ';', '/', '=', '-', '+']);
            (subfield.code, text.to_owned())
        })
        .collect()
}

/// What differs between the fields of one tag: `245` when a field is missing, added or has
/// other subfields, `245 punctuation` when only the ISBD punctuation of the subfields differs, `245/ind1` when an indicator differs and `245/ind1 blank` when the
/// source left blank an indicator that the export writes.
fn field_differences(original: &[Field], written: &[Field], tag: [u8; 3]) -> Vec<String> {
    let tag = String::from_utf8_lossy(&tag);
    let mut found = Vec::new();
    let same_subfields = original.len() == written.len()
        && original.iter().zip(written).all(|pair| match pair {
            (
                Field::Data { subfields: one, .. },
                Field::Data {
                    subfields: other, ..
                },
            ) => one == other,
            (one, other) => one == other,
        });
    if !same_subfields {
        let same_text = original.len() == written.len()
            && original
                .iter()
                .zip(written)
                .all(|(one, other)| without_punctuation(one) == without_punctuation(other));
        found.push(if same_text {
            format!("{tag} punctuation")
        } else {
            tag.to_string()
        });
    }
    let mut indicators: Vec<String> = original
        .iter()
        .zip(written)
        .flat_map(|pair| match pair {
            (
                Field::Data {
                    indicators: one, ..
                },
                Field::Data {
                    indicators: other, ..
                },
            ) => one
                .iter()
                .zip(other)
                .enumerate()
                .filter(|(_, (one, other))| one != other)
                .map(|(position, (one, _))| {
                    let blank = if *one == b' ' { " blank" } else { "" };
                    format!("{tag}/ind{}{blank}", position + 1)
                })
                .collect(),
            _ => Vec::new(),
        })
        .collect();
    indicators.sort_unstable();
    indicators.dedup();
    found.extend(indicators);
    found
}

/// What differs between `original` and `written`, by the keys of [`KNOWN_DIFFERENCES`].
/// The groups of `008` positions of a book, as the conversions of the LC read them: date
/// entered, type of date, the two dates, place, illustrations, target audience, form of
/// item, nature of contents, government publication, conference, festschrift and index,
/// undefined, literary form, biography, language, modified record and cataloguing source.
const FIXED_GROUPS: [(usize, usize); 17] = [
    (0, 6),
    (6, 7),
    (7, 11),
    (11, 15),
    (15, 18),
    (18, 22),
    (22, 23),
    (23, 24),
    (24, 28),
    (28, 29),
    (29, 32),
    (32, 33),
    (33, 34),
    (34, 35),
    (35, 38),
    (38, 39),
    (39, 40),
];

/// What differs in `008`, by group of positions (`008/35-37`); `fill` after the key when
/// the source holds only fill characters there (`|`, no attempt to code, or blank where
/// the format defines nothing), and `length`
/// when the source field does not have its 40 positions.
fn fixed_differences(original: &Record, written: &Record) -> Vec<String> {
    let value = |record: &Record| match tags(record, *b"008").first() {
        Some(Field::Control { value, .. }) => value.clone(),
        _ => Vec::new(),
    };
    let (source, export) = (value(original), value(written));
    let mut found = Vec::new();
    if source.len() != 40 {
        found.push("008 length".to_owned());
    }
    for (start, end) in FIXED_GROUPS {
        let one = source.get(start..end.min(source.len())).unwrap_or_default();
        let other = export.get(start..end.min(export.len())).unwrap_or_default();
        if one == other || (source.len() < end && one.iter().all(|b| *b == b' ')) {
            continue;
        }
        let key = if end - start == 1 {
            format!("008/{start:02}")
        } else {
            format!("008/{start:02}-{:02}", end - 1)
        };
        // A fill character in every position the format defines (`32` is undefined).
        found.push(
            if one.contains(&b'|') && one.iter().all(|b| matches!(b, b'|' | b' ')) {
                format!("{key} fill")
            } else {
                key
            },
        );
    }
    found
}

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
    for tag in all {
        if tag == *b"008" {
            found.extend(fixed_differences(original, written));
            continue;
        }
        found.extend(field_differences(
            &tags(original, tag),
            &tags(written, tag),
            tag,
        ));
    }
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

#[test]
fn the_245_gets_the_isbd_punctuation_of_the_lc_way_back() {
    let subfields = |record: &Record| match &tags(record, *b"245")[0] {
        Field::Data {
            indicators,
            subfields,
            ..
        } => (
            *indicators,
            subfields
                .iter()
                .map(|s| (s.code, String::from_utf8_lossy(&s.value).into_owned()))
                .collect::<Vec<_>>(),
        ),
        Field::Control { .. } => panic!("245 is a data field"),
    };
    let (_, record) = &records()[10];
    let mut written = model(record);
    let as_written = |code: u8, text: &str| (code, text.to_owned());
    assert_eq!(
        subfields(&bibframe_to_marc(&written, None)),
        (
            *b"10",
            vec![
                as_written(b'a', "Regimes políticos :"),
                as_written(b'b', "uma viagem /"),
                as_written(b'c', "Jayme Brener."),
            ]
        )
    );
    // Without a statement of responsibility, the subtitle closes the field.
    written
        .instance
        .description
        .statements
        .retain(|s| s.property != "responsibilityStatement");
    written.contributions.retain(|c| !c.primary);
    assert_eq!(
        subfields(&bibframe_to_marc(&written, None)),
        (
            *b"00",
            vec![
                as_written(b'a', "Regimes políticos :"),
                as_written(b'b', "uma viagem."),
            ]
        )
    );
}

#[test]
fn one_language_goes_to_the_008_and_more_than_one_to_the_041_too() {
    let (_, record) = &records()[0];
    let mut edited = model(record);
    assert_eq!(edited.languages, ["por"]);
    let fixed = |record: &Record| match tags(record, *b"008").first() {
        Some(Field::Control { value, .. }) => value.get(35..38).unwrap_or_default().to_vec(),
        _ => Vec::new(),
    };
    let written = bibframe_to_marc(&edited, None);
    assert_eq!(fixed(&written), b"por");
    assert!(tags(&written, *b"041").is_empty());
    assert_eq!(model(&written).languages, ["por"]);
    // A second language writes both in `041 $a`, the first also in `008`.
    edited.languages = vec!["eng".to_owned(), "por".to_owned()];
    let written = bibframe_to_marc(&edited, Some(record));
    assert_eq!(fixed(&written), b"eng");
    assert_eq!(model(&written).languages, ["eng", "por"]);
}
