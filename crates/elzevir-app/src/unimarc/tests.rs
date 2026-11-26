// SPDX-License-Identifier: GPL-3.0-or-later
//! UNIMARC both ways (docs/specs/catalogo/unimarc.md, §3).

use elzevir_marc::{Field, Limits, Record, RecordRef, Subfield, Tag};

use super::{from_marc21, to_marc21};
use crate::catalog::NewRecord;
use crate::marc_mapping::{bibframe_to_marc, marc_to_bibframe};

/// The first 200 records of the test base (crates/elzevir-app/tests/citation.rs).
const SAMPLE: &[u8] = include_bytes!("../../tests/data/koha-staff-bibs-1-200.mrc");

/// The sample records MARC21 cannot hand to UNIMARC whole (item 14), by reason.
const LOSSES: [(usize, &str); 28] = [
    (18, "relator term ($e)"),
    (19, "relator term ($e)"),
    (20, "relator term ($e)"),
    (55, "subject heading with a title (610 $t)"),
    (96, "form of work (380)"),
    (97, "relator term ($e)"),
    (99, "literary form: novels"),
    (102, "relator term ($e)"),
    (103, "relator term ($e)"),
    (105, "variant title: distinctive"),
    (109, "relator term ($e)"),
    (114, "relator term ($e)"),
    (121, "relator term ($e)"),
    (126, "variant title: distinctive"),
    (
        134,
        "variant title: distinctive; script of the language note (546 $b)",
    ),
    (140, "literary form: novels"),
    (141, "variant title: portion"),
    (145, "relator term ($e)"),
    (146, "relator term ($e)"),
    (149, "relator term ($e)"),
    (150, "relator term ($e)"),
    (152, "relator term ($e)"),
    (156, "relator term ($e)"),
    (169, "audience: specialized"),
    (172, "audience: specialized"),
    (174, "relator term ($e)"),
    (176, "relator term ($e)"),
    (178, "relator term ($e)"),
];

fn sample() -> Vec<Record> {
    SAMPLE
        .split_inclusive(|&b| b == 0x1d)
        .filter(|raw| raw.len() > 1)
        .map(|raw| {
            let mut record = Record::from_ref(&RecordRef::parse(raw, &Limits::default()).unwrap());
            record.marc21_to_utf8();
            record
        })
        .collect()
}

/// The model of a UNIMARC record, as the import maps it.
fn model(unimarc: &Record) -> NewRecord {
    let mut model = marc_to_bibframe(&to_marc21(unimarc), None).unwrap();
    model.source = None;
    model
}

/// The UNIMARC export of a model.
fn exported(model: &NewRecord) -> Record {
    from_marc21(&bibframe_to_marc(model, None))
}

#[test]
fn a_unimarc_record_comes_back_as_it_went() {
    for (position, record) in sample().iter().enumerate() {
        // The first export carries the ISBD punctuation of the MARC21 writer around
        // relator terms, which UNIMARC drops; from the second on, nothing changes.
        let unimarc = exported(&model(&exported(&marc_to_bibframe(record, None).unwrap())));
        let read = model(&unimarc);
        let again = exported(&read);
        assert_eq!(again.leader, unimarc.leader, "{position}");
        assert_eq!(again.fields, unimarc.fields, "{position}");
        assert_eq!(model(&again), read, "{position}");
    }
}

#[test]
fn marc21_records_lose_only_what_unimarc_cannot_hold() {
    let mut differing = Vec::new();
    for (position, record) in sample().iter().enumerate() {
        let mut before = marc_to_bibframe(record, None).unwrap();
        before.source = None;
        let after = model(&exported(&before));
        if after != before {
            differing.push(position);
        }
    }
    assert_eq!(differing, LOSSES.map(|(position, _)| position));
}

fn data(tag: [u8; 3], indicators: [u8; 2], subfields: &[(u8, &str)]) -> Field {
    Field::Data {
        tag: Tag::new(tag),
        indicators,
        subfields: subfields
            .iter()
            .map(|(code, value)| Subfield {
                code: *code,
                value: value.as_bytes().to_vec(),
            })
            .collect(),
    }
}

fn field(record: &Record, tag: [u8; 3]) -> &Field {
    record
        .fields
        .iter()
        .find(|field| *field.tag().as_bytes() == tag)
        .unwrap()
}

fn marc21(fields: Vec<Field>) -> Record {
    Record {
        leader: *b"00000nam a2200000 a 4500",
        fields,
    }
}

#[test]
fn names_titles_and_subjects_take_their_unimarc_places() {
    let record = marc21(vec![
        Field::Control {
            tag: Tag::new(*b"008"),
            value: b"261126s2009    bl a          000 1 por d".to_vec(),
        },
        data(
            *b"100",
            *b"1 ",
            &[(b'a', "Lemos, Ana Maria"), (b'd', "1950-"), (b'4', "aut")],
        ),
        data(
            *b"245",
            *b"14",
            &[(b'a', "The rio"), (b'b', "um romance"), (b'c', "Ana Lemos")],
        ),
        data(
            *b"650",
            *b" 0",
            &[(b'a', "Rivers"), (b'z', "Brazil"), (b'v', "Fiction")],
        ),
        data(
            *b"952",
            *b"  ",
            &[(b'a', "CEN"), (b'p', "0001"), (b'o', "869.3 L557r")],
        ),
    ]);
    let unimarc = from_marc21(&record);
    assert_eq!(&unimarc.leader, b"00000nam  2200000   4500");
    assert_eq!(
        field(&unimarc, *b"700"),
        &data(
            *b"700",
            *b" 1",
            &[
                (b'a', "Lemos"),
                (b'b', "Ana Maria"),
                (b'f', "1950-"),
                (b'4', "aut")
            ]
        )
    );
    assert_eq!(
        field(&unimarc, *b"200"),
        &data(
            *b"200",
            *b"1 ",
            &[(b'a', "The rio"), (b'e', "um romance"), (b'f', "Ana Lemos")]
        )
    );
    assert_eq!(
        field(&unimarc, *b"606"),
        &data(
            *b"606",
            *b"  ",
            &[
                (b'a', "Rivers"),
                (b'y', "Brazil"),
                (b'j', "Fiction"),
                (b'2', "lcsh")
            ]
        )
    );
    assert_eq!(
        field(&unimarc, *b"995"),
        &data(
            *b"995",
            *b"  ",
            &[(b'b', "CEN"), (b'f', "0001"), (b'k', "869.3 L557r")]
        )
    );
    let Field::Data { subfields, .. } = field(&unimarc, *b"100") else {
        panic!()
    };
    let general = &subfields[0].value;
    assert_eq!(general.len(), 36);
    assert_eq!(&general[..17], b"20261126d2009    ");
    assert_eq!(&general[26..30], b"50  ");
    assert_eq!(
        field(&unimarc, *b"105"),
        &data(*b"105", *b"  ", &[(b'a', "a   z   000ay")])
    );
    let back = to_marc21(&unimarc);
    assert_eq!(
        field(&back, *b"100"),
        &data(
            *b"100",
            *b"1 ",
            &[(b'a', "Lemos, Ana Maria"), (b'd', "1950-"), (b'4', "aut")]
        )
    );
    assert_eq!(
        field(&back, *b"650"),
        &data(
            *b"650",
            *b" 0",
            &[(b'a', "Rivers"), (b'z', "Brazil"), (b'v', "Fiction")]
        )
    );
}

#[test]
fn other_agencies_fields_and_sorting_marks_are_read() {
    let unimarc = Record {
        leader: *b"00000nam  2200000   4500",
        fields: vec![
            data(*b"200", *b"1 ", &[(b'a', "\u{98}Le \u{9c}petit prince")]),
            data(
                *b"702",
                *b" 1",
                &[(b'a', "Silva"), (b'b', "Ana"), (b'4', "trl")],
            ),
            data(*b"712", *b"02", &[(b'a', "Editora Exemplo")]),
            data(*b"606", *b"  ", &[(b'a', "Contos"), (b'2', "local")]),
        ],
    };
    let marc21 = to_marc21(&unimarc);
    assert_eq!(
        field(&marc21, *b"245"),
        &data(*b"245", *b"10", &[(b'a', "Le petit prince")])
    );
    assert_eq!(
        field(&marc21, *b"700"),
        &data(*b"700", *b"1 ", &[(b'a', "Silva, Ana"), (b'4', "trl")])
    );
    assert_eq!(
        field(&marc21, *b"710"),
        &data(*b"710", *b"2 ", &[(b'a', "Editora Exemplo")])
    );
    assert_eq!(
        field(&marc21, *b"650"),
        &data(*b"650", *b" 7", &[(b'a', "Contos"), (b'2', "local")])
    );
}
