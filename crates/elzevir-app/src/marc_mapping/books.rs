// SPDX-License-Identifier: GPL-3.0-or-later
//! The positions of a book in `008/18-34` (docs/specs/catalogo/bibframe-completo.md, item
//! 29, part 5f), after the Library of Congress conversions (`marc2bibframe2`:
//! `ConvSpec-006,008.xsl` with the code tables of `conf/codeMaps.xml`; `bibframe2marc`:
//! `01-001-008.xml`).
//!
//! Each coded value becomes a statement of the Work: the illustrations (`18-21`) a
//! `bf:illustrativeContent` of the `millus` list, the target audience (`22`) a
//! `bf:intendedAudience` of `maudience`, the nature of the contents (`24-27`) a
//! `bf:genreForm` of the LCGFT or a `bf:supplementaryContent` of `msupplcont`, a conference
//! (`29`) or a festschrift (`30`) a `bf:genreForm`, an index (`31`) a
//! `bf:supplementaryContent`, and the literary form (`33`) and biography (`34`) a
//! `bf:genreForm`. The government publication (`28`) has only an LC extension
//! (`bflc:governmentPubType`) and the form of the item (`23`) is the carrier, so both stay
//! in the source record.

use elzevir_domain::statement::{Statement, Value};

use super::Marc;

/// The LCGFT terms (`id.loc.gov/authorities/genreForms`).
pub(super) const GENRE_FORMS: &str = "http://id.loc.gov/authorities/genreForms/";

/// `008/18-21`: each code and its concept of `millus`.
pub(super) const ILLUSTRATIONS: [(u8, &str); 15] = [
    (b'a', "ill"),
    (b'b', "map"),
    (b'c', "por"),
    (b'd', "chr"),
    (b'e', "pln"),
    (b'f', "plt"),
    (b'g', "mus"),
    (b'h', "fac"),
    (b'i', "coa"),
    (b'j', "gnt"),
    (b'k', "for"),
    (b'l', "sam"),
    (b'm', "pho"),
    (b'o', "pht"),
    (b'p', "ilm"),
];

/// `008/22`: each code and its concept of `maudience`, in the order the LC way back tries
/// them.
pub(super) const AUDIENCES: [(u8, &str); 8] = [
    (b'a', "pre"),
    (b'b', "pri"),
    (b'c', "pad"),
    (b'd', "ado"),
    (b'e', "adu"),
    (b'f', "spe"),
    (b'g', "gen"),
    (b'j', "juv"),
];

/// What a code of the nature of the contents names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Content {
    /// An LCGFT term, by its identifier.
    Genre(&'static str),
    /// A concept of `msupplcont`.
    Supplement(&'static str),
}

/// `008/24-27`: each code and what it names; the way back writes the first code of each.
pub(super) const CONTENTS: [(u8, Content); 22] = [
    (b'b', Content::Supplement("bibliography")),
    (b'c', Content::Genre("gf2014026057")),
    (b'd', Content::Genre("gf2014026086")),
    (b'e', Content::Genre("gf2014026092")),
    (b'f', Content::Genre("gf2014026109")),
    (b'g', Content::Genre("gf2011026351")),
    (b'h', Content::Genre("gf2014026049")),
    (b'i', Content::Genre("gf2014026112")),
    (b'j', Content::Genre("gf2011026438")),
    (b'k', Content::Supplement("discography")),
    (b'l', Content::Genre("gf2011026351")),
    (b'm', Content::Genre("gf2014026039")),
    (b'q', Content::Supplement("film")),
    (b'r', Content::Genre("gf2014026087")),
    (b's', Content::Genre("gf2014026181")),
    (b't', Content::Genre("gf2015026093")),
    (b'v', Content::Genre("gf2011026351")),
    (b'w', Content::Genre("gf2011026351")),
    (b'y', Content::Genre("gf2014026208")),
    (b'z', Content::Genre("gf2011026707")),
    (b'5', Content::Genre("gf2014026055")),
    (b'6', Content::Genre("gf2014026266")),
];

/// `008/29` `1`: conference papers and proceedings.
pub(super) const CONFERENCE: &str = "gf2014026068";
/// `008/30` `1`: festschriften.
pub(super) const FESTSCHRIFT: &str = "gf2016026082";
/// `008/31` `1`: an index, of `msupplcont`.
pub(super) const INDEX: &str = "index";

/// `008/33`: each literary form and its LCGFT term (`0`, not fiction, names none).
const LITERARY_FORMS: [(u8, &str); 10] = [
    (b'1', "gf2014026339"),
    (b'd', "gf2014026297"),
    (b'e', "gf2014026094"),
    (b'f', "gf2015026020"),
    (b'h', "gf2014026110"),
    (b'i', "gf2014026141"),
    (b'j', "gf2014026542"),
    (b'm', "gf2014026339"),
    (b'p', "gf2014026481"),
    (b's', "gf2011026363"),
];

/// `008/34`: autobiography and biography.
const BIOGRAPHIES: [(u8, &str); 3] = [
    (b'a', "gf2014026047"),
    (b'b', "gf2014026049"),
    (b'c', "gf2014026049"),
];

/// Whether the Leader describes a book (`008` "BK"): language material or a manuscript
/// that is not a continuing resource.
pub(super) fn is_book(record_type: u8, level: u8) -> bool {
    matches!(record_type, b'a' | b't') && !matches!(level, b'b' | b'i' | b's')
}

/// The statements of the Work from `008/18-34` of a book, each once, in position order.
pub(super) fn of_work(marc: &Marc<'_>) -> Vec<Statement> {
    if !is_book(marc.leader_byte(6), marc.leader_byte(7)) {
        return Vec::new();
    }
    let Some(fixed) = marc.control(*b"008").and_then(|fixed| fixed.get(18..35)) else {
        return Vec::new();
    };
    let at = |position: usize| fixed.get(position - 18).copied().unwrap_or(b' ');
    let mut statements = Vec::new();
    let mut push = |statement: Statement| {
        if !statements.contains(&statement) {
            statements.push(statement);
        }
    };
    for code in (18..22).map(at) {
        if let Some((_, illustration)) = ILLUSTRATIONS.iter().find(|(c, _)| *c == code) {
            push(concept("illustrativeContent", "millus", illustration));
        }
    }
    if let Some((_, audience)) = AUDIENCES.iter().find(|(c, _)| *c == at(22)) {
        push(concept("intendedAudience", "maudience", audience));
    }
    for code in (24..28).map(at) {
        match CONTENTS.iter().find(|(c, _)| *c == code).map(|(_, c)| *c) {
            Some(Content::Genre(id)) => push(genre(id)),
            Some(Content::Supplement(code)) => {
                push(concept("supplementaryContent", "msupplcont", code));
            }
            None => {}
        }
    }
    if at(29) == b'1' {
        push(genre(CONFERENCE));
    }
    if at(30) == b'1' {
        push(genre(FESTSCHRIFT));
    }
    if at(31) == b'1' {
        push(concept("supplementaryContent", "msupplcont", INDEX));
    }
    for (position, table) in [(33, &LITERARY_FORMS[..]), (34, &BIOGRAPHIES[..])] {
        if let Some((_, id)) = table.iter().find(|(c, _)| *c == at(position)) {
            push(genre(id));
        }
    }
    statements
}

fn concept(property: &str, scheme: &str, code: &str) -> Statement {
    Statement {
        property: property.to_owned(),
        value: Value::Concept {
            scheme: scheme.to_owned(),
            code: code.to_owned(),
        },
    }
}

fn genre(id: &str) -> Statement {
    Statement {
        property: "genreForm".to_owned(),
        value: Value::Iri(format!("{GENRE_FORMS}{id}")),
    }
}

#[cfg(test)]
mod tests {
    use elzevir_marc::{Field, Record, Tag};

    use super::*;

    fn record(leader: &[u8; 24], fixed: &str) -> Record {
        Record {
            leader: *leader,
            fields: vec![Field::Control {
                tag: Tag::new(*b"008"),
                value: fixed.as_bytes().to_vec(),
            }],
        }
    }

    #[test]
    fn the_positions_of_a_book_become_statements_of_the_work() {
        let book = record(
            b"00000nam a2200000 i 4500",
            "250321s1995    bl ac  j bd   111 1bpor d",
        );
        let statements = of_work(&Marc(&book));
        assert_eq!(
            statements,
            [
                concept("illustrativeContent", "millus", "ill"),
                concept("illustrativeContent", "millus", "por"),
                concept("intendedAudience", "maudience", "juv"),
                concept("supplementaryContent", "msupplcont", "bibliography"),
                genre("gf2014026086"),
                genre(CONFERENCE),
                genre(FESTSCHRIFT),
                concept("supplementaryContent", "msupplcont", INDEX),
                genre("gf2014026339"),
                genre("gf2014026049"),
            ]
        );
        // Fill characters and zeros name nothing; a serial has other positions.
        let filled = record(
            b"00000nam a2200000 i 4500",
            "250321s1995||||xx |||||||||||||| ||por|||",
        );
        assert!(of_work(&Marc(&filled)).is_empty());
        let serial = record(
            b"00000nas a2200000 i 4500",
            "250321c19959999bl ac  j bd   111 1bpor d",
        );
        assert!(of_work(&Marc(&serial)).is_empty());
    }
}
