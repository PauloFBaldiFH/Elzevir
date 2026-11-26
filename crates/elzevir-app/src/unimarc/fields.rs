// SPDX-License-Identifier: GPL-3.0-or-later
//! The data fields (docs/specs/catalogo/unimarc.md, items 6 to 14): for each pair of
//! tags, one list of subfield correspondences, read one way to write UNIMARC and the
//! other way to read it.

use elzevir_marc::{Field, Subfield, Tag};

/// Subfield correspondences, `(MARC21 code, UNIMARC code)`, in the order a code is
/// looked up: the first pair with the code wins.
type Pairs = &'static [(u8, u8)];

/// Where one field goes: its tag, its indicators and its subfield correspondences.
struct Target {
    tag: [u8; 3],
    indicators: [u8; 2],
    pairs: Pairs,
    kind: Kind,
}

/// What a field needs besides its subfield correspondences.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Plain,
    /// A personal name: MARC21 `$a` "Surname, Forename" is UNIMARC `$a` and `$b`.
    Person,
    /// A heading with subject subdivisions and a thesaurus.
    Subject,
    /// A personal name heading of a subject.
    PersonSubject,
}

const TERM: Pairs = &[(b'a', b'a')];
const NOTE: Pairs = &[(b'a', b'a'), (b'5', b'5')];
const PERSON: Pairs = &[
    (b'a', b'a'),
    (b'b', b'd'),
    (b'c', b'c'),
    (b'd', b'f'),
    (b'q', b'g'),
    (b'4', b'4'),
    (b'0', b'3'),
    (b'1', b'1'),
    // Read only: some agencies write the MARC21 subfields as they are.
    (b'0', b'0'),
];
const CORPORATE: Pairs = &[
    (b'a', b'a'),
    (b'b', b'b'),
    (b'c', b'c'),
    (b'4', b'4'),
    (b'0', b'3'),
    (b'1', b'1'),
    // Read only: some agencies write the MARC21 subfields as they are.
    (b'0', b'0'),
];
const MEETING: Pairs = &[
    (b'a', b'a'),
    (b'n', b'd'),
    (b'c', b'e'),
    (b'd', b'f'),
    (b'4', b'4'),
    (b'0', b'3'),
    (b'1', b'1'),
    // Read only: some agencies write the MARC21 subfields as they are.
    (b'0', b'0'),
];
/// Subject subdivisions: form, general, chronological and geographic.
const SUBDIVISIONS: Pairs = &[
    (b'v', b'j'),
    (b'x', b'x'),
    (b'y', b'z'),
    (b'z', b'y'),
    (b'2', b'2'),
    (b'0', b'3'),
    (b'1', b'1'),
    // Read only: some agencies write the MARC21 subfields as they are.
    (b'0', b'0'),
];
const PUBLICATION: Pairs = &[
    (b'a', b'a'),
    (b'b', b'c'),
    (b'c', b'd'),
    (b'e', b'e'),
    (b'f', b'g'),
];
/// Koha's `952` and `995`, subfield by subfield after the `kohafield` of each in the
/// default frameworks of the two families.
const ITEM: Pairs = &[
    (b'0', b'0'),
    (b'1', b'2'),
    (b'2', b'4'),
    (b'3', b'l'),
    (b'4', b'1'),
    (b'5', b'3'),
    (b'6', b's'),
    (b'7', b'o'),
    (b'8', b'h'),
    (b'9', b'9'),
    (b'a', b'b'),
    (b'b', b'c'),
    (b'c', b'e'),
    (b'd', b'5'),
    (b'e', b'a'),
    (b'f', b'8'),
    (b'g', b'p'),
    (b'h', b'v'),
    (b'i', b'j'),
    (b'j', b'q'),
    (b'l', b'w'),
    (b'm', b'x'),
    (b'n', b't'),
    (b'o', b'k'),
    (b'p', b'f'),
    (b'q', b'n'),
    (b'r', b'y'),
    (b's', b'm'),
    (b't', b'6'),
    (b'u', b'7'),
    (b'v', b'g'),
    (b'w', b'i'),
    (b'x', b'z'),
    (b'y', b'r'),
    (b'z', b'u'),
];

const fn target(tag: [u8; 3], indicators: [u8; 2], pairs: Pairs) -> Target {
    Target {
        tag,
        indicators,
        pairs,
        kind: Kind::Plain,
    }
}

const fn with(mut target: Target, kind: Kind) -> Target {
    target.kind = kind;
    target
}

/// Where a MARC21 data field goes in UNIMARC; `None` for a field UNIMARC has no place
/// for (item 14).
fn unimarc_target(tag: [u8; 3], indicators: [u8; 2]) -> Option<Target> {
    let [first, second] = indicators;
    let surname = if first == b'0' { b'0' } else { b'1' };
    Some(match &tag {
        b"016" => target(*b"020", *b"  ", &[(b'a', b'b'), (b'2', b'a')]),
        b"020" => target(
            *b"010",
            *b"  ",
            &[(b'a', b'a'), (b'q', b'b'), (b'c', b'd'), (b'z', b'z')],
        ),
        b"022" => target(*b"011", *b"  ", &[(b'a', b'a'), (b'z', b'z')]),
        b"035" => target(*b"035", *b"  ", &[(b'a', b'a'), (b'z', b'z')]),
        b"041" => target(
            *b"101",
            [first, b' '],
            &[(b'a', b'a'), (b'h', b'c'), (b'b', b'd')],
        ),
        b"043" => target(*b"660", *b"  ", TERM),
        b"045" => target(*b"661", *b"  ", TERM),
        b"050" => target(*b"680", [b' ', second], &[(b'a', b'a'), (b'b', b'b')]),
        b"080" => target(*b"675", *b"  ", &[(b'a', b'a'), (b'2', b'v')]),
        b"082" => target(*b"676", [first, second], &[(b'a', b'a'), (b'2', b'v')]),
        b"084" => target(*b"686", *b"  ", &[(b'a', b'a'), (b'2', b'2')]),
        b"100" if first == b'3' => with(target(*b"720", *b"  ", PERSON), Kind::Person),
        b"100" => with(target(*b"700", [b' ', surname], PERSON), Kind::Person),
        b"110" => target(*b"710", *b"02", CORPORATE),
        b"111" => target(*b"710", *b"12", MEETING),
        b"130" => target(*b"500", *b"10", TERM),
        b"210" => target(*b"531", *b"  ", TERM),
        b"240" => target(*b"500", *b"10", &[(b'a', b'a'), (b'l', b'm')]),
        b"243" => target(*b"501", *b"10", TERM),
        b"245" => target(
            *b"200",
            [if first == b'0' { b'0' } else { b'1' }, b' '],
            &[
                (b'a', b'a'),
                (b'b', b'e'),
                (b'n', b'h'),
                (b'p', b'i'),
                (b'c', b'f'),
            ],
        ),
        b"246" => target(*variant_tag(second), *b"1 ", TERM),
        b"250" => target(*b"205", *b"  ", &[(b'a', b'a'), (b'b', b'f')]),
        b"260" => target(*b"210", *b"  ", PUBLICATION),
        b"264" => target(*b"214", [b' ', second], PUBLICATION),
        b"300" => target(
            *b"215",
            *b"  ",
            &[(b'a', b'a'), (b'b', b'c'), (b'c', b'd'), (b'e', b'e')],
        ),
        b"310" => target(*b"326", *b"  ", TERM),
        b"336" => target(*b"181", *b"  ", &[(b'b', b'c'), (b'2', b'2')]),
        b"337" => target(*b"182", *b"  ", &[(b'b', b'c'), (b'2', b'2')]),
        b"338" => target(*b"183", *b"  ", &[(b'b', b'a'), (b'2', b'2')]),
        b"440" => target(*b"225", *b"2 ", &[(b'a', b'a'), (b'v', b'v')]),
        b"490" => target(
            *b"225",
            if first == b'1' { *b"0 " } else { *b"1 " },
            &[(b'a', b'a'), (b'v', b'v')],
        ),
        b"500" => target(*b"300", *b"  ", NOTE),
        b"504" => target(*b"320", *b"  ", NOTE),
        b"505" => target(*b"327", [first, b' '], NOTE),
        b"520" => target(*b"330", *b"  ", NOTE),
        b"534" => target(*b"324", *b"  ", NOTE),
        b"546" => target(*b"302", *b"  ", NOTE),
        b"555" => target(*b"321", *b"  ", NOTE),
        b"561" => target(*b"317", *b"  ", NOTE),
        b"590" => target(*b"316", *b"  ", NOTE),
        b"600" if first == b'3' => with(target(*b"602", *b"  ", PERSON), Kind::Subject),
        b"600" => with(
            target(*b"600", [b' ', surname], PERSON),
            Kind::PersonSubject,
        ),
        b"610" => with(target(*b"601", *b"02", CORPORATE), Kind::Subject),
        b"611" => with(target(*b"601", *b"12", MEETING), Kind::Subject),
        b"630" => with(target(*b"605", *b"  ", TERM), Kind::Subject),
        b"650" => with(target(*b"606", *b"  ", TERM), Kind::Subject),
        b"651" => with(target(*b"607", *b"  ", TERM), Kind::Subject),
        b"655" => with(target(*b"608", *b"  ", TERM), Kind::Subject),
        b"700" if first == b'3' => with(target(*b"721", *b"  ", PERSON), Kind::Person),
        b"700" => with(target(*b"701", [b' ', surname], PERSON), Kind::Person),
        b"710" => target(*b"711", *b"02", CORPORATE),
        b"711" => target(*b"711", *b"12", MEETING),
        b"740" if second == b'2' => target(*b"464", *b"  ", &[(b'a', b't')]),
        b"740" => target(*b"488", *b"  ", &[(b'a', b't')]),
        b"775" => target(
            *b"451",
            *b" 1",
            &[(b'a', b'a'), (b't', b't'), (b'b', b'e'), (b'w', b'3')],
        ),
        b"830" => target(
            *b"410",
            *b" 1",
            &[(b'a', b't'), (b'p', b'i'), (b'v', b'v'), (b'0', b'3')],
        ),
        b"856" => target(*b"856", [first, second], &[(b'u', b'u'), (b'z', b'z')]),
        b"952" => target(*b"995", *b"  ", ITEM),
        _ => return None,
    })
}

/// Where a UNIMARC data field goes in MARC21; `None` for a field MARC21 has no place for.
/// Fields of other agencies' practice that Elzevir does not write are read too (`702`,
/// `712`, `722`).
fn marc21_target(tag: [u8; 3], indicators: [u8; 2]) -> Option<Target> {
    let [first, second] = indicators;
    let surname = if second == b'0' { b'0' } else { b'1' };
    let meeting = first == b'1';
    Some(match &tag {
        b"010" => target(*b"020", *b"  ", unimarc_pairs(*b"020")),
        b"011" => target(*b"022", *b"  ", unimarc_pairs(*b"022")),
        b"020" => target(*b"016", *b"7 ", unimarc_pairs(*b"016")),
        b"035" => target(*b"035", *b"  ", unimarc_pairs(*b"035")),
        b"101" => target(*b"041", [first, b' '], unimarc_pairs(*b"041")),
        b"181" => target(*b"336", *b"  ", unimarc_pairs(*b"336")),
        b"182" => target(*b"337", *b"  ", unimarc_pairs(*b"337")),
        b"183" => target(*b"338", *b"  ", unimarc_pairs(*b"338")),
        b"200" => target(
            *b"245",
            [if first == b'0' { b'0' } else { b'1' }, b'0'],
            unimarc_pairs(*b"245"),
        ),
        b"205" => target(*b"250", *b"  ", unimarc_pairs(*b"250")),
        b"210" => target(*b"260", *b"  ", PUBLICATION),
        b"214" => target(*b"264", [b' ', second], PUBLICATION),
        b"215" => target(*b"300", *b"  ", unimarc_pairs(*b"300")),
        b"225" if first == b'2' => target(*b"440", *b" 0", unimarc_pairs(*b"440")),
        b"225" => target(
            *b"490",
            if first == b'0' { *b"1 " } else { *b"0 " },
            unimarc_pairs(*b"490"),
        ),
        b"300" => target(*b"500", *b"  ", NOTE),
        b"302" => target(*b"546", *b"  ", NOTE),
        b"316" => target(*b"590", *b"  ", NOTE),
        b"317" => target(*b"561", *b"  ", NOTE),
        b"320" => target(*b"504", *b"  ", NOTE),
        b"321" => target(*b"555", *b"  ", NOTE),
        b"324" => target(*b"534", *b"  ", NOTE),
        b"326" => target(*b"310", *b"  ", TERM),
        b"327" => target(*b"505", [first, b' '], NOTE),
        b"330" => target(*b"520", *b"  ", NOTE),
        b"410" => target(*b"830", *b" 0", unimarc_pairs(*b"830")),
        b"451" => target(*b"775", *b"0 ", unimarc_pairs(*b"775")),
        b"500" => target(*b"240", *b"10", unimarc_pairs(*b"240")),
        b"501" => target(*b"243", *b"10", TERM),
        b"510" | b"512" | b"513" | b"514" | b"515" | b"516" | b"517" => {
            target(*b"246", [b'3', variant_indicator(tag)], TERM)
        }
        b"531" => target(*b"210", *b"0 ", TERM),
        b"464" => target(*b"740", *b"02", &[(b'a', b't')]),
        b"488" => target(*b"740", *b"0 ", &[(b'a', b't')]),
        b"600" => with(
            target(*b"600", [surname, b' '], PERSON),
            Kind::PersonSubject,
        ),
        b"601" if meeting => with(target(*b"611", *b"2 ", MEETING), Kind::Subject),
        b"601" => with(target(*b"610", *b"2 ", CORPORATE), Kind::Subject),
        b"602" => with(target(*b"600", *b"3 ", PERSON), Kind::Subject),
        b"605" => with(target(*b"630", *b"0 ", TERM), Kind::Subject),
        b"606" => with(target(*b"650", *b"  ", TERM), Kind::Subject),
        b"607" => with(target(*b"651", *b"  ", TERM), Kind::Subject),
        b"608" => with(target(*b"655", *b"  ", TERM), Kind::Subject),
        b"660" => target(*b"043", *b"  ", TERM),
        b"661" => target(*b"045", *b"  ", TERM),
        b"675" => target(*b"080", *b"  ", unimarc_pairs(*b"080")),
        b"676" => target(*b"082", [first, second], unimarc_pairs(*b"082")),
        b"680" => target(*b"050", [b' ', second], unimarc_pairs(*b"050")),
        b"686" => target(*b"084", *b"  ", unimarc_pairs(*b"084")),
        b"700" => with(target(*b"100", [surname, b' '], PERSON), Kind::Person),
        b"720" => with(target(*b"100", *b"3 ", PERSON), Kind::Person),
        b"701" | b"702" => with(target(*b"700", [surname, b' '], PERSON), Kind::Person),
        b"721" | b"722" => with(target(*b"700", *b"3 ", PERSON), Kind::Person),
        b"710" if meeting => target(*b"111", *b"2 ", MEETING),
        b"710" => target(*b"110", *b"2 ", CORPORATE),
        b"711" | b"712" if meeting => target(*b"711", *b"2 ", MEETING),
        b"711" | b"712" => target(*b"710", *b"2 ", CORPORATE),
        b"856" => target(*b"856", [first, second], unimarc_pairs(*b"856")),
        b"995" => target(*b"952", *b"  ", ITEM),
        _ => return None,
    })
}

/// Variant titles by type: the second indicator of `246` against the UNIMARC tag. Portion
/// (`0`), distinctive (`2`) and other (`3`) titles have no tag of their own (item 14).
const VARIANTS: [(u8, &[u8; 3]); 7] = [
    (b'1', b"510"),
    (b'4', b"512"),
    (b'5', b"513"),
    (b'6', b"514"),
    (b'7', b"515"),
    (b'8', b"516"),
    (b' ', b"517"),
];

fn variant_tag(indicator: u8) -> &'static [u8; 3] {
    VARIANTS
        .iter()
        .find(|(found, _)| *found == indicator)
        .map_or(b"517", |(_, tag)| tag)
}

fn variant_indicator(tag: [u8; 3]) -> u8 {
    VARIANTS
        .iter()
        .find(|(_, found)| **found == tag)
        .map_or(b' ', |(indicator, _)| *indicator)
}

/// The subfield correspondences of the MARC21 field `tag`, for reading UNIMARC with the
/// pairs that write it.
fn unimarc_pairs(tag: [u8; 3]) -> Pairs {
    unimarc_target(tag, *b"  ").map_or(&[], |target| target.pairs)
}

/// `field`, a MARC21 data field, as UNIMARC.
pub(super) fn to_unimarc(tag: Tag, indicators: [u8; 2], subfields: &[Subfield]) -> Option<Field> {
    let target = unimarc_target(*tag.as_bytes(), indicators)?;
    let mut out = Vec::with_capacity(subfields.len() + 1);
    let person = matches!(target.kind, Kind::Person | Kind::PersonSubject);
    for subfield in subfields {
        // A name in inverted order is UNIMARC `$a` (surname) and `$b` (the rest).
        if person
            && subfield.code == b'a'
            && target.indicators[1] == b'1'
            && let Some((surname, forename)) = split_name(&subfield.value)
        {
            out.push(sub(b'a', surname));
            out.push(sub(b'b', forename));
            continue;
        }
        if let Some(code) = convert(
            subfield.code,
            target.pairs,
            target.kind,
            Direction::ToUnimarc,
        ) {
            out.push(sub(code, &subfield.value));
        }
    }
    let subject = matches!(target.kind, Kind::Subject | Kind::PersonSubject);
    if subject
        && !out.iter().any(|s| s.code == b'2')
        && let Some(code) = thesaurus_code(indicators[1])
    {
        out.push(sub(b'2', code.as_bytes()));
    }
    Some(Field::Data {
        tag: Tag::new(target.tag),
        indicators: target.indicators,
        subfields: out,
    })
}

/// `field`, a UNIMARC data field, as MARC21.
pub(super) fn to_marc21(tag: Tag, indicators: [u8; 2], subfields: &[Subfield]) -> Option<Field> {
    let mut target = marc21_target(*tag.as_bytes(), indicators)?;
    let mut out: Vec<Subfield> = Vec::with_capacity(subfields.len());
    let mut thesaurus = None;
    for subfield in subfields {
        // An inverted personal name carries the rest of the name in UNIMARC `$b`.
        if matches!(
            target.kind,
            Kind::Person | Kind::PersonSubject | Kind::Subject
        ) && subfield.code == b'b'
            && target.pairs == PERSON
            && let Some(name) = out.iter_mut().rev().find(|s| s.code == b'a')
        {
            name.value.extend_from_slice(b", ");
            name.value.extend_from_slice(&subfield.value);
            continue;
        }
        let Some(code) = convert(
            subfield.code,
            target.pairs,
            target.kind,
            Direction::ToMarc21,
        ) else {
            continue;
        };
        if code == b'2' && matches!(target.kind, Kind::Subject | Kind::PersonSubject) {
            if let Some(indicator) = thesaurus_indicator(&subfield.value) {
                thesaurus = Some(indicator);
                continue;
            }
            thesaurus = Some(b'7');
        }
        out.push(sub(code, &without_sorting_marks(&subfield.value)));
    }
    if matches!(target.kind, Kind::Subject | Kind::PersonSubject) {
        target.indicators[1] = thesaurus.unwrap_or(b'4');
    }
    Some(Field::Data {
        tag: Tag::new(target.tag),
        indicators: target.indicators,
        subfields: out,
    })
}

#[derive(Clone, Copy)]
enum Direction {
    ToUnimarc,
    ToMarc21,
}

/// The code of `code` on the other side: the field's own pairs first, then the subject
/// subdivisions for a subject heading.
fn convert(code: u8, pairs: Pairs, kind: Kind, direction: Direction) -> Option<u8> {
    let subdivisions = matches!(kind, Kind::Subject | Kind::PersonSubject);
    let lists: &[Pairs] = if subdivisions {
        &[pairs, SUBDIVISIONS]
    } else {
        &[pairs]
    };
    lists
        .iter()
        .flat_map(|list| list.iter())
        .find_map(|&(marc21, unimarc)| match direction {
            Direction::ToUnimarc => (marc21 == code).then_some(unimarc),
            Direction::ToMarc21 => (unimarc == code).then_some(marc21),
        })
}

/// "Surname, Forename" split at its first comma.
fn split_name(name: &[u8]) -> Option<(&[u8], &[u8])> {
    let comma = name.windows(2).position(|pair| pair == b", ")?;
    Some((name.get(..comma)?, name.get(comma + 2..)?))
}

/// The UNIMARC system code of a MARC21 subject thesaurus (second indicator); `None` for
/// `4` (unspecified) and `7` (named in `$2`).
fn thesaurus_code(indicator: u8) -> Option<&'static str> {
    Some(match indicator {
        b'0' => "lcsh",
        b'1' => "lcshac",
        b'2' => "mesh",
        b'3' => "nal",
        b'5' => "cash",
        b'6' => "rvm",
        _ => return None,
    })
}

/// The MARC21 second indicator a UNIMARC system code stands for, the reverse of
/// [`thesaurus_code`].
fn thesaurus_indicator(code: &[u8]) -> Option<u8> {
    Some(match code {
        b"lcsh" => b'0',
        b"lcshac" => b'1',
        b"mesh" => b'2',
        b"nal" => b'3',
        b"cash" => b'5',
        b"rvm" => b'6',
        _ => return None,
    })
}

/// `value` without the marks around words left out of sorting: NSB and NSE, as
/// U+0088 and U+0089 or as U+0098 and U+009C (item 11).
fn without_sorting_marks(value: &[u8]) -> Vec<u8> {
    const MARKS: [[u8; 2]; 4] = [[0xc2, 0x88], [0xc2, 0x89], [0xc2, 0x98], [0xc2, 0x9c]];
    let mut out = Vec::with_capacity(value.len());
    let mut rest = value;
    while let Some((&first, tail)) = rest.split_first() {
        if let Some(second) = tail.first()
            && MARKS.contains(&[first, *second])
        {
            rest = tail.get(1..).unwrap_or_default();
            continue;
        }
        out.push(first);
        rest = tail;
    }
    out
}

fn sub(code: u8, value: &[u8]) -> Subfield {
    Subfield {
        code,
        value: value.to_vec(),
    }
}
