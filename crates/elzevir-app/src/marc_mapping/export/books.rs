// SPDX-License-Identifier: GPL-3.0-or-later
//! The positions of a book back to `008/18-34` (bibframe-completo.md, item 29, part 5f),
//! after the way back of the Library of Congress (`bibframe2marc`: `01-001-008.xml`).
//!
//! The way back reads more LCGFT terms than the way in writes: every term of fiction gives
//! `33` `1`, and every catalog or comics term its code of the nature of the contents. Codes
//! that name the same term come back as the first of them: `m` (mixed forms) as `1`, `c`
//! (biography with some biographical information) as `b`, and `l`, `v` and `w` (law
//! materials) as `g`. The positions this module does not read (`23`, the form of
//! the item; `28`, the government publication; `32`) stay as they are.

use elzevir_domain::statement::{Description, Statement, Value};
use elzevir_marc::Record;

use super::super::books::{
    AUDIENCES, CONFERENCE, CONTENTS, Content, FESTSCHRIFT, GENRE_FORMS, ILLUSTRATIONS, INDEX,
    is_book,
};
use super::fields::set_008;

/// More LCGFT terms the way back reads for the nature of the contents (`24-27`), beside
/// those of [`CONTENTS`]: bibliographies, catalogs and comics.
const MORE_CONTENTS: [(&str, u8); 10] = [
    ("gf2014026048", b'b'),
    ("gf2014026058", b'c'),
    ("gf2014026088", b'c'),
    ("gf2015026050", b'c'),
    ("gf2014026572", b'6'),
    ("gf2022026036", b'6'),
    ("gf2023026096", b'6'),
    ("gf2014026362", b'6'),
    ("gf2016026005", b'6'),
    ("gf2017026130", b'6'),
];

/// `008/33`: the LCGFT terms the way back reads; `0` without any. The LC way back tries
/// the terms of fiction first, so that novels (`f`) and short stories (`j`) come back as
/// `1`; here the specific forms come first, so that a code survives the way in and back.
const LITERARY_FORMS: [(&str, u8); 17] = [
    ("gf2014026297", b'd'),
    ("gf2014026094", b'e'),
    ("gf2015026020", b'f'),
    ("gf2014026110", b'h'),
    ("gf2014026141", b'i'),
    ("gf2014026054", b'i'),
    ("gf2014026542", b'j'),
    ("gf2014026481", b'p'),
    ("gf2011026363", b's'),
    ("gf2014026339", b'1'),
    ("gf2014026243", b'1'),
    ("gf2014026259", b'1'),
    ("gf2018026099", b'1'),
    ("gf2015026019", b'1'),
    ("gf2014026456", b'1'),
    ("gf2014026518", b'1'),
    ("gf2014026559", b'1'),
];

/// `008/34`: the LCGFT terms the way back reads, in its order; blank without any.
const BIOGRAPHIES: [(&str, u8); 5] = [
    ("gf2014026047", b'a'),
    ("gf2014026085", b'a'),
    ("gf2014026049", b'b'),
    ("gf2014026229", b'a'),
    ("gf2014026244", b'b'),
];

/// The statements of the Work that `008/18-34` codes.
fn of_group(work: &Description) -> Vec<&Statement> {
    work.statements
        .iter()
        .filter(|s| {
            genre(s).is_some()
                || concept(s, "illustrativeContent", "millus").is_some()
                || concept(s, "intendedAudience", "maudience").is_some()
                || concept(s, "supplementaryContent", "msupplcont").is_some()
        })
        .collect()
}

/// Writes `008/18-22`, `24-27`, `29-31` and `33-34` of a book when the statements they code
/// changed from `before`.
pub(super) fn write(out: &mut Record, work: &Description, before: Option<&Description>) {
    let Some([record_type, level]) = out.leader.get(6..8) else {
        return;
    };
    if !is_book(*record_type, *level) {
        return;
    }
    let wanted = of_group(work);
    if before.is_some_and(|before| of_group(before) == wanted) {
        return;
    }
    let genres: Vec<&str> = wanted.iter().filter_map(|s| genre(s)).collect();
    let has_genre = |id: &str| genres.contains(&id);
    let illustrations = codes(wanted.iter().filter_map(|s| {
        let code = concept(s, "illustrativeContent", "millus")?;
        ILLUSTRATIONS
            .iter()
            .find(|(_, c)| *c == code)
            .map(|(m, _)| *m)
    }));
    let audience = AUDIENCES
        .iter()
        .find(|(_, code)| {
            wanted
                .iter()
                .any(|s| concept(s, "intendedAudience", "maudience") == Some(code))
        })
        .map_or(b' ', |(marc, _)| *marc);
    let mut first = illustrations.to_vec();
    first.push(audience);
    set_008(out, 18, &first);
    set_008(
        out,
        24,
        &codes(wanted.iter().filter_map(|s| content_code(s))),
    );
    let index = wanted
        .iter()
        .any(|s| concept(s, "supplementaryContent", "msupplcont") == Some(INDEX));
    let flag = |yes: bool| if yes { b'1' } else { b'0' };
    set_008(
        out,
        29,
        &[
            flag(has_genre(CONFERENCE)),
            flag(has_genre(FESTSCHRIFT)),
            flag(index),
        ],
    );
    let first_of = |table: &[(&str, u8)], none: u8| {
        table
            .iter()
            .find(|(id, _)| has_genre(id))
            .map_or(none, |(_, code)| *code)
    };
    set_008(
        out,
        33,
        &[
            first_of(&LITERARY_FORMS, b'0'),
            first_of(&BIOGRAPHIES, b' '),
        ],
    );
}

/// The first four distinct codes, padded with blanks.
fn codes(found: impl Iterator<Item = u8>) -> [u8; 4] {
    let mut distinct: Vec<u8> = Vec::new();
    for code in found {
        if !distinct.contains(&code) {
            distinct.push(code);
        }
    }
    let mut positions = [b' '; 4];
    for (position, code) in positions.iter_mut().zip(distinct) {
        *position = code;
    }
    positions
}

/// The code of the nature of the contents a statement gives.
fn content_code(statement: &Statement) -> Option<u8> {
    if let Some(id) = genre(statement) {
        return CONTENTS
            .iter()
            .find(|(_, content)| matches!(content, Content::Genre(known) if *known == id))
            .map(|(code, _)| *code)
            .or_else(|| {
                MORE_CONTENTS
                    .iter()
                    .find(|(more, _)| *more == id)
                    .map(|(_, code)| *code)
            });
    }
    let supplement = concept(statement, "supplementaryContent", "msupplcont")?;
    CONTENTS
        .iter()
        .find(|(_, content)| matches!(content, Content::Supplement(known) if *known == supplement))
        .map(|(code, _)| *code)
}

/// The LCGFT identifier of a `bf:genreForm`.
fn genre(statement: &Statement) -> Option<&str> {
    match &statement.value {
        Value::Iri(iri) if statement.property == "genreForm" => iri.strip_prefix(GENRE_FORMS),
        _ => None,
    }
}

/// The code of a concept of `scheme` held by `property`.
fn concept<'a>(statement: &'a Statement, property: &str, scheme: &str) -> Option<&'a str> {
    match &statement.value {
        Value::Concept { scheme: s, code } if statement.property == property && s == scheme => {
            Some(code)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use elzevir_marc::{Field, Tag};

    use super::super::super::{Marc, books};
    use super::*;

    /// Every term the way in writes or the way back reads is a current LCGFT term, so
    /// the OPAC shows its label (bibframe-completo.md, item 12).
    #[test]
    fn every_genre_of_the_book_positions_is_a_current_lcgft_term() {
        let ways_in = CONTENTS.iter().filter_map(|(_, content)| match content {
            Content::Genre(id) => Some(*id),
            Content::Supplement(_) => None,
        });
        let ways_back = MORE_CONTENTS
            .iter()
            .chain(&LITERARY_FORMS)
            .chain(&BIOGRAPHIES)
            .map(|(id, _)| *id);
        for id in ways_in.chain(ways_back).chain([CONFERENCE, FESTSCHRIFT]) {
            let term = elzevir_data::vocabulary::genre_form(&format!("{GENRE_FORMS}{id}"));
            assert!(term.is_some_and(|term| !term.deprecated), "{id}");
        }
    }

    fn fixed(record: &Record) -> String {
        match record.fields.first() {
            Some(Field::Control { value, .. }) => String::from_utf8_lossy(value).into_owned(),
            _ => String::new(),
        }
    }

    #[test]
    fn the_positions_of_a_book_come_back_as_the_lc_writes_them() {
        let source = Record {
            leader: *b"00000nam a2200000 i 4500",
            fields: vec![Field::Control {
                tag: Tag::new(*b"008"),
                value: b"250321s1995    bl ac  j bd   111 1bpor d".to_vec(),
            }],
        };
        let work = Description {
            classes: Vec::new(),
            statements: books::of_work(&Marc(&source)),
        };
        // Unchanged statements leave the record alone.
        let mut out = source.clone();
        write(&mut out, &work, Some(&work));
        assert_eq!(out, source);
        // Written anew, the same codes come back; Biographies also gives `h` in `24-27`.
        let mut empty = Record {
            leader: source.leader,
            fields: Vec::new(),
        };
        write(&mut empty, &work, None);
        assert_eq!(
            fixed(&empty),
            format!("{}ac  j bdh  111 1b{}", " ".repeat(18), " ".repeat(5))
        );
        // Without statements, the way back writes the zeros of a book.
        let mut plain = source.clone();
        write(
            &mut plain,
            &Description {
                classes: Vec::new(),
                statements: Vec::new(),
            },
            Some(&work),
        );
        assert_eq!(fixed(&plain), "250321s1995    bl            000 0 por d");
    }
}
