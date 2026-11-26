// SPDX-License-Identifier: GPL-3.0-or-later
//! The coded data (docs/specs/catalogo/unimarc.md, items 3 to 5): what MARC21 spreads
//! over the Leader, `003`, `008` and `040`, UNIMARC spreads over the Leader, `100`, `101`,
//! `102`, `105`, `106` and `801`. Each code table is a list of `(MARC21, UNIMARC)` pairs,
//! read one way to write and the other way to read.

use elzevir_marc::{Field, Record, Subfield, Tag};

type Codes = &'static [(u8, u8)];

/// Leader/06, the type of record.
const RECORD_TYPES: Codes = &[
    (b'a', b'a'),
    (b't', b'b'),
    (b'c', b'c'),
    (b'd', b'd'),
    (b'e', b'e'),
    (b'f', b'f'),
    (b'g', b'g'),
    (b'i', b'i'),
    (b'j', b'j'),
    (b'k', b'k'),
    (b'm', b'l'),
    (b'o', b'm'),
    (b'p', b'm'),
    (b'r', b'r'),
];

/// Leader/07, the bibliographic level.
const LEVELS: Codes = &[
    (b'm', b'm'),
    (b's', b's'),
    (b'i', b'i'),
    (b'c', b'c'),
    (b'a', b'a'),
    (b'b', b'a'),
    (b'd', b'a'),
];

/// Leader/05, the record status.
const STATUSES: Codes = &[
    (b'n', b'n'),
    (b'c', b'c'),
    (b'd', b'd'),
    (b'p', b'p'),
    (b'a', b'o'),
];

/// Leader/18: AACR 2 has no UNIMARC code and goes in `801 $g` (item 3).
const CONVENTIONS: Codes = &[(b'i', b' '), (b'c', b'i'), (b' ', b'n')];

/// `008/06`, the type of date, against `100 $a/8`.
const DATE_TYPES: Codes = &[
    (b's', b'd'),
    (b'm', b'g'),
    (b'r', b'e'),
    (b't', b'h'),
    (b'c', b'a'),
    (b'd', b'b'),
    (b'u', b'c'),
    (b'n', b'u'),
    (b'e', b'j'),
    (b'p', b'i'),
    (b'q', b'f'),
    (b'i', b'g'),
    (b'k', b'g'),
];

/// `008/22` of a book, the target audience, against `100 $a/17`.
const AUDIENCES: Codes = &[
    (b'j', b'a'),
    (b'a', b'b'),
    (b'b', b'c'),
    (b'c', b'd'),
    (b'd', b'e'),
    (b'g', b'm'),
    (b'e', b'k'),
    (b'f', b'k'),
];

/// `008/18-21` of a book, the illustrations, against `105 $a/0-3`.
const ILLUSTRATIONS: Codes = &[
    (b'a', b'a'),
    (b'b', b'b'),
    (b'c', b'c'),
    (b'd', b'd'),
    (b'e', b'e'),
    (b'f', b'f'),
    (b'g', b'g'),
    (b'h', b'h'),
    (b'i', b'i'),
    (b'j', b'j'),
    (b'k', b'k'),
    (b'l', b'l'),
    (b'm', b'm'),
    (b'p', b'o'),
];

/// `008/24-27` of a book, the nature of contents, against `105 $a/4-7`.
const CONTENTS: Codes = &[
    (b'b', b'a'),
    (b'c', b'b'),
    (b'i', b'c'),
    (b'a', b'd'),
    (b'd', b'e'),
    (b'e', b'f'),
    (b'r', b'g'),
    (b's', b'i'),
    (b'p', b'j'),
    (b'j', b'k'),
    (b'u', b'l'),
    (b'm', b'm'),
    (b'l', b'n'),
    (b'g', b'n'),
    (b'v', b'n'),
    (b'w', b'n'),
    (b't', b'p'),
    (b'n', b'r'),
    (b'z', b's'),
    (b'6', b't'),
];

/// `008/33` of a book, the literary form, against `105 $a/11`.
const LITERARY_FORMS: Codes = &[
    (b'0', b'y'),
    (b'1', b'a'),
    (b'd', b'b'),
    (b'e', b'c'),
    (b'h', b'd'),
    (b'i', b'e'),
    (b'j', b'f'),
    (b'p', b'g'),
    (b's', b'h'),
    (b'm', b'z'),
    (b'f', b'a'),
];

/// `008/34` of a book, biography, against `105 $a/12`.
const BIOGRAPHIES: Codes = &[
    (b' ', b'y'),
    (b'a', b'a'),
    (b'b', b'b'),
    (b'c', b'c'),
    (b'd', b'd'),
];

/// `008/23` of a book, the form of item, against `106 $a`.
const FORMS: Codes = &[
    (b'd', b'd'),
    (b'f', b'f'),
    (b'r', b'r'),
    (b'a', b'g'),
    (b'o', b's'),
    (b'q', b't'),
    (b's', b'l'),
];

fn unimarc_code(codes: Codes, marc21: u8) -> Option<u8> {
    codes.iter().find(|(m, _)| *m == marc21).map(|(_, u)| *u)
}

fn marc21_code(codes: Codes, unimarc: u8) -> Option<u8> {
    codes.iter().find(|(_, u)| *u == unimarc).map(|(m, _)| *m)
}

fn byte(value: &[u8], position: usize) -> u8 {
    value.get(position).copied().unwrap_or(b' ')
}

/// Whether Leader/06-07 describe a book, whose `008/18-34` have the positions above.
fn is_book(leader: &[u8; 24]) -> bool {
    matches!(leader[6], b'a' | b't') && !matches!(leader[7], b'b' | b'i' | b's')
}

/// The UNIMARC Leader of a MARC21 record.
pub(super) fn unimarc_leader(leader: &[u8; 24]) -> [u8; 24] {
    let mut out = *b"00000nam  22000003  4500";
    out[5] = unimarc_code(STATUSES, leader[5]).unwrap_or(b'n');
    out[6] = unimarc_code(RECORD_TYPES, leader[6]).unwrap_or(b'a');
    out[7] = unimarc_code(LEVELS, leader[7]).unwrap_or(b'm');
    out[17] = b' ';
    out[18] = unimarc_code(CONVENTIONS, leader[18]).unwrap_or(b' ');
    out
}

/// The MARC21 Leader of a UNIMARC record.
pub(super) fn marc21_leader(leader: &[u8; 24], aacr: bool) -> [u8; 24] {
    let mut out = *b"00000nam a2200000 i 4500";
    out[5] = marc21_code(STATUSES, leader[5]).unwrap_or(b'n');
    out[6] = marc21_code(RECORD_TYPES, leader[6]).unwrap_or(b'a');
    out[7] = marc21_code(LEVELS, leader[7]).unwrap_or(b'm');
    out[18] = if aacr {
        b'a'
    } else {
        marc21_code(CONVENTIONS, leader[18]).unwrap_or(b' ')
    };
    out
}

/// The UNIMARC coded fields of a MARC21 record: `100`, `101` (when no `041` gives it),
/// `102`, `105`, `106` and `801`, in tag order.
pub(super) fn unimarc_fields(record: &Record) -> Vec<Field> {
    let fixed = control(record, *b"008").unwrap_or_default();
    let cataloguing = first_subfield(record, *b"040", b'b');
    let mut fields = vec![data(
        *b"100",
        *b"  ",
        vec![(
            b'a',
            general(&record.leader, &fixed, cataloguing.as_deref()),
        )],
    )];
    let has_041 = record.fields.iter().any(|f| f.tag().as_bytes() == b"041");
    let language = fixed
        .get(35..38)
        .filter(|code| code.iter().all(u8::is_ascii_alphabetic));
    if !has_041 && let Some(code) = language {
        fields.push(data(*b"101", *b"  ", vec![(b'a', code.to_vec())]));
    }
    if let Some(country) = fixed
        .get(15..18)
        .filter(|code| code.iter().any(|b| *b != b' ' && *b != b'|'))
    {
        let code: Vec<u8> = country.iter().copied().filter(|b| *b != b' ').collect();
        fields.push(data(
            *b"102",
            *b"  ",
            vec![(b'b', code), (b'2', b"marccountry".to_vec())],
        ));
    }
    if is_book(&record.leader) && fixed.len() >= 35 {
        fields.push(data(*b"105", *b"  ", vec![(b'a', textual(&fixed))]));
        if let Some(form) = unimarc_code(FORMS, byte(&fixed, 23)) {
            fields.push(data(*b"106", *b"  ", vec![(b'a', vec![form])]));
        }
    }
    fields.extend(sources(record));
    fields
}

/// `100 $a`, the general processing data.
fn general(leader: &[u8; 24], fixed: &[u8], cataloguing: Option<&[u8]>) -> Vec<u8> {
    let mut out = b"        u        u  y0   y50      ba".to_vec();
    // 0-7: date entered on file, `yyyymmdd`, from `008/00-05` `yymmdd`.
    if let Some(entered) = fixed.get(..6).filter(|d| d.iter().all(u8::is_ascii_digit)) {
        let century: &[u8] = if entered.get(..2) < Some(b"65".as_slice()) {
            b"20"
        } else {
            b"19"
        };
        splice(&mut out, 0, century);
        splice(&mut out, 2, entered);
    }
    set(
        &mut out,
        8,
        unimarc_code(DATE_TYPES, byte(fixed, 6)).unwrap_or(b'u'),
    );
    splice(&mut out, 9, fixed.get(7..11).unwrap_or(b"    "));
    splice(&mut out, 13, fixed.get(11..15).unwrap_or(b"    "));
    let audience = if is_book(leader) {
        unimarc_code(AUDIENCES, byte(fixed, 22))
    } else {
        None
    };
    set(&mut out, 17, audience.unwrap_or(b'u'));
    if let Some(language) = cataloguing.filter(|code| code.len() == 3) {
        splice(&mut out, 22, language);
    }
    out
}

/// `105 $a`, the coded data of a textual book: illustrations (`y` for none), nature of
/// contents (`z` for none), conference, festschrift, index, literary form, biography.
fn textual(fixed: &[u8]) -> Vec<u8> {
    let mut out = b"            y".to_vec();
    let illustrations: Vec<u8> = (18..22)
        .filter_map(|p| unimarc_code(ILLUSTRATIONS, byte(fixed, p)))
        .collect();
    splice(
        &mut out,
        0,
        if illustrations.is_empty() {
            b"y"
        } else {
            &illustrations
        },
    );
    let contents: Vec<u8> = (24..28)
        .filter_map(|p| unimarc_code(CONTENTS, byte(fixed, p)))
        .collect();
    splice(
        &mut out,
        4,
        if contents.is_empty() { b"z" } else { &contents },
    );
    for (slot, position) in [(8, 29), (9, 30), (10, 31)] {
        set(
            &mut out,
            slot,
            if byte(fixed, position) == b'1' {
                b'1'
            } else {
                b'0'
            },
        );
    }
    set(
        &mut out,
        11,
        unimarc_code(LITERARY_FORMS, byte(fixed, 33)).unwrap_or(b'y'),
    );
    set(
        &mut out,
        12,
        unimarc_code(BIOGRAPHIES, byte(fixed, 34)).unwrap_or(b'y'),
    );
    out
}

/// `801`, the originating source, from `040` and `003` (item 4).
fn sources(record: &Record) -> Vec<Field> {
    let mut out = Vec::new();
    let values = |code: u8| -> Vec<Vec<u8>> {
        record
            .fields
            .iter()
            .filter_map(|field| match field {
                Field::Data { tag, subfields, .. } if tag.as_bytes() == b"040" => Some(subfields),
                _ => None,
            })
            .take(1)
            .flatten()
            .filter(|s| s.code == code)
            .map(|s| s.value.clone())
            .collect()
    };
    let mut original: Vec<(u8, Vec<u8>)> = values(b'a').into_iter().map(|v| (b'b', v)).collect();
    original.extend(values(b'e').into_iter().map(|v| (b'g', v)));
    if record.leader.get(18) == Some(&b'a') {
        original.push((b'g', AACR2.to_vec()));
    }
    if !original.is_empty() {
        out.push(data(*b"801", *b" 0", original));
    }
    for (indicator, code) in [(*b" 1", b'c'), (*b" 2", b'd')] {
        for value in values(code) {
            out.push(data(*b"801", indicator, vec![(b'b', value)]));
        }
    }
    if let Some(assigner) = control(record, *b"003") {
        out.push(data(*b"801", *b" 3", vec![(b'b', assigner)]));
    }
    out
}

/// What `801 $g` says for Leader/18 `a`.
const AACR2: &[u8] = b"AACR2";

/// The MARC21 `003`, `008` and `040` of a UNIMARC record, and whether it says AACR 2.
pub(super) struct Marc21Coded {
    pub(super) fields: Vec<Field>,
    pub(super) aacr: bool,
}

/// The MARC21 coded fields of a UNIMARC record, the reverse of [`unimarc_fields`].
pub(super) fn marc21_fields(record: &Record) -> Marc21Coded {
    let general = first_subfield(record, *b"100", b'a').unwrap_or_default();
    let fixed = marc21_fixed(record, &general);
    let mut source = marc21_source(record);
    if let Some(language) = general
        .get(22..25)
        .filter(|code| code.iter().all(u8::is_ascii_alphabetic))
    {
        source.cataloguing.push((b'b', language.to_vec()));
    }
    // In the order the MARC21 writer gives `040` (marc_mapping::export).
    let order = |code: u8| b"abcde".iter().position(|c| *c == code);
    source.cataloguing.sort_by_key(|(code, _)| order(*code));
    let mut fields = Vec::new();
    if let Some(assigner) = source.assigner {
        fields.push(Field::Control {
            tag: Tag::new(*b"003"),
            value: assigner,
        });
    }
    fields.push(Field::Control {
        tag: Tag::new(*b"008"),
        value: fixed,
    });
    if !source.cataloguing.is_empty() {
        fields.push(data(*b"040", *b"  ", source.cataloguing));
    }
    Marc21Coded {
        fields,
        aacr: source.aacr,
    }
}

/// `008` from `100`, `101`, `102`, `105` and `106`.
fn marc21_fixed(record: &Record, general: &[u8]) -> Vec<u8> {
    let mut fixed = vec![b' '; 40];
    set(&mut fixed, 39, b'd');
    if let Some(entered) = general
        .get(2..8)
        .filter(|d| d.iter().all(u8::is_ascii_digit))
    {
        splice(&mut fixed, 0, entered);
    }
    set(
        &mut fixed,
        6,
        marc21_code(DATE_TYPES, byte(general, 8)).unwrap_or(b'|'),
    );
    splice(&mut fixed, 7, general.get(9..13).unwrap_or(b"    "));
    splice(&mut fixed, 11, general.get(13..17).unwrap_or(b"    "));
    if let Some(mut country) = first_subfield(record, *b"102", b'b') {
        country.resize(3, b' ');
        splice(&mut fixed, 15, &country);
    }
    let leader = marc21_leader(&record.leader, false);
    if is_book(&leader) {
        set(
            &mut fixed,
            22,
            marc21_code(AUDIENCES, byte(general, 17)).unwrap_or(b' '),
        );
        let form = first_subfield(record, *b"106", b'a').and_then(|f| f.first().copied());
        set(
            &mut fixed,
            23,
            form.and_then(|f| marc21_code(FORMS, f)).unwrap_or(b' '),
        );
        if let Some(textual) = first_subfield(record, *b"105", b'a') {
            book_positions(&mut fixed, &textual);
        }
    }
    let language = first_subfield(record, *b"101", b'a').filter(|code| code.len() == 3);
    splice(&mut fixed, 35, language.as_deref().unwrap_or(b"   "));
    fixed
}

/// `008/18-34` of a book from `105 $a`.
fn book_positions(fixed: &mut [u8], textual: &[u8]) {
    let illustrations: Vec<u8> = (0..4)
        .filter_map(|p| marc21_code(ILLUSTRATIONS, byte(textual, p)))
        .collect();
    splice(fixed, 18, &illustrations);
    let contents: Vec<u8> = (4..8)
        .filter_map(|p| marc21_code(CONTENTS, byte(textual, p)))
        .collect();
    splice(fixed, 24, &contents);
    for (slot, position) in [(8, 29), (9, 30), (10, 31)] {
        set(
            fixed,
            position,
            if byte(textual, slot) == b'1' {
                b'1'
            } else {
                b'0'
            },
        );
    }
    set(
        fixed,
        33,
        marc21_code(LITERARY_FORMS, byte(textual, 11)).unwrap_or(b'u'),
    );
    set(
        fixed,
        34,
        marc21_code(BIOGRAPHIES, byte(textual, 12)).unwrap_or(b' '),
    );
}

/// What the `801`s say: the subfields of `040`, the assigner of `001` and AACR 2.
#[derive(Default)]
struct Source {
    cataloguing: Vec<(u8, Vec<u8>)>,
    assigner: Option<Vec<u8>>,
    aacr: bool,
}

fn marc21_source(record: &Record) -> Source {
    let mut source = Source::default();
    for field in &record.fields {
        let Field::Data {
            tag,
            indicators: [_, function],
            subfields,
        } = field
        else {
            continue;
        };
        if tag.as_bytes() != b"801" {
            continue;
        }
        for subfield in subfields {
            let value = subfield.value.clone();
            match (function, subfield.code) {
                (b'0', b'b') => source.cataloguing.push((b'a', value)),
                (b'0', b'g') if value == AACR2 => source.aacr = true,
                (b'0', b'g') => source.cataloguing.push((b'e', value)),
                (b'1', b'b') => source.cataloguing.push((b'c', value)),
                (b'2', b'b') => source.cataloguing.push((b'd', value)),
                (b'3', b'b') => source.assigner = Some(value),
                _ => {}
            }
        }
    }
    source
}

fn set(out: &mut [u8], at: usize, value: u8) {
    if let Some(slot) = out.get_mut(at) {
        *slot = value;
    }
}

fn splice(out: &mut [u8], at: usize, value: &[u8]) {
    if let Some(slot) = out.get_mut(at..at + value.len()) {
        slot.copy_from_slice(value);
    }
}

fn control(record: &Record, tag: [u8; 3]) -> Option<Vec<u8>> {
    record.fields.iter().find_map(|field| match field {
        Field::Control { tag: found, value } if *found.as_bytes() == tag => Some(value.clone()),
        _ => None,
    })
}

fn first_subfield(record: &Record, tag: [u8; 3], code: u8) -> Option<Vec<u8>> {
    record.fields.iter().find_map(|field| match field {
        Field::Data {
            tag: found,
            subfields,
            ..
        } if *found.as_bytes() == tag => subfields
            .iter()
            .find(|s| s.code == code)
            .map(|s| s.value.clone()),
        _ => None,
    })
}

fn data(tag: [u8; 3], indicators: [u8; 2], subfields: Vec<(u8, Vec<u8>)>) -> Field {
    Field::Data {
        tag: Tag::new(tag),
        indicators,
        subfields: subfields
            .into_iter()
            .map(|(code, value)| Subfield { code, value })
            .collect(),
    }
}
