// SPDX-License-Identifier: GPL-3.0-or-later
//! Classes, content, media, carrier and issuance (docs/specs/catalogo/bibframe-completo.md,
//! item 10h), after the Library of Congress conversion (`marc2bibframe2`:
//! `ConvSpec-LDR.xsl` and `conf/codeMaps.xml`, `contentTypes`).

use elzevir_data::vocabulary;
use elzevir_domain::statement::{Description, Statement};

use elzevir_marc::Field;

use super::Marc;
use super::admin;
use super::books;
use super::classification;
use super::content;
use super::fixed;
use super::form;
use super::locators;
use super::national;
use super::notes;
use super::physical;
use super::provision;
use super::relations;
use super::responsibility;
use super::temporal;
use super::titles::{self, Owner};
use crate::description::concept;

/// The Work: classes from Leader/06 and /07, content from `336 $b` or else Leader/06.
pub(super) fn work(marc: &Marc<'_>) -> Description {
    let (mut classes, content) = record_type(marc.leader_byte(6));
    if let Some(class) = level_class(marc.leader_byte(7)) {
        classes.push(class);
    }
    let mut statements = coded(marc, *b"336", "content", "contentTypes");
    if marc.first(b"336").is_none()
        && let Some(code) = content
    {
        statements.push(concept("content", "contentTypes", code));
    }
    statements.extend(titles::of_owner(marc, Owner::Work));
    statements.extend(notes::of_owner(marc, Owner::Work));
    statements.extend(relations::of_work(marc));
    statements.extend(content::of_work(marc));
    statements.extend(fixed::of_work(marc));
    statements.extend(books::of_work(marc));
    statements.extend(temporal::of_work(marc));
    statements.extend(form::of_work(marc));
    statements.extend(classification::of_work(marc));
    Description {
        classes: classes.into_iter().map(str::to_owned).collect(),
        statements,
    }
}

/// The Instance: issuance from `334 $b` or else Leader/07, media `337 $b`, carrier `338 $b`.
/// The conversion gives an Instance no class of its own; an electronic edition gets
/// `bf:Electronic` here (bibframe-completo.md, item 10j).
pub(super) fn instance(marc: &Marc<'_>) -> Description {
    let mut statements = coded(marc, *b"334", "issuance", "issuance");
    if marc.first(b"334").is_none()
        && let Some(code) = issuance(marc.leader_byte(7))
    {
        statements.push(concept("issuance", "issuance", code));
    }
    statements.extend(coded(marc, *b"337", "media", "mediaTypes"));
    statements.extend(coded(marc, *b"338", "carrier", "carriers"));
    statements.extend(titles::of_owner(marc, Owner::Instance));
    statements.extend(responsibility::of_instance(marc));
    statements.extend(notes::of_owner(marc, Owner::Instance));
    statements.extend(provision::of_instance(marc));
    // A copyright date that `264` already gave is not repeated from `008`.
    for statement in fixed::of_instance(marc) {
        if !statements.contains(&statement) {
            statements.push(statement);
        }
    }
    statements.extend(locators::of_instance(marc));
    statements.extend(national::of_instance(marc));
    statements.extend(physical::of_instance(marc));
    statements.extend(admin::of_instance(marc));
    Description {
        classes: if super::instance::electronic(marc) {
            vec!["Electronic".to_owned()]
        } else {
            Vec::new()
        },
        statements,
    }
}

/// Leader/06: the Work's classes and its content code.
fn record_type(byte: u8) -> (Vec<&'static str>, Option<&'static str>) {
    match byte {
        b'a' => (vec!["Text"], Some("txt")),
        b'c' => (vec!["NotatedMusic"], Some("ntm")),
        b'd' => (vec!["NotatedMusic", "Manuscript"], Some("ntm")),
        b'e' => (vec!["Cartography"], Some("cri")),
        b'f' => (vec!["Cartography", "Manuscript"], Some("cri")),
        b'g' => (vec!["MovingImage"], Some("tdi")),
        b'i' => (vec!["NonMusicAudio"], Some("spw")),
        b'j' => (vec!["MusicAudio"], Some("prm")),
        b'k' => (vec!["StillImage"], Some("sti")),
        b'm' => (vec!["Multimedia"], Some("cop")),
        b'o' | b'p' => (vec!["MixedMaterial"], Some("txt")),
        b'r' => (vec!["Object"], Some("tdf")),
        // `t` (manuscript text) and anything else: the conversion gives no class.
        _ => (Vec::new(), None),
    }
}

/// Leader/07: the Work's class for its bibliographic level.
fn level_class(byte: u8) -> Option<&'static str> {
    match byte {
        b'a' | b'm' => Some("Monograph"),
        b'b' | b's' => Some("Serial"),
        b'c' | b'd' => Some("Collection"),
        b'i' => Some("Integrating"),
        _ => None,
    }
}

/// Leader/07: the Instance's mode of issuance.
fn issuance(byte: u8) -> Option<&'static str> {
    match byte {
        b'a' | b'd' | b'm' => Some("mono"),
        b'b' | b's' => Some("serl"),
        b'c' => Some("mulm"),
        b'i' => Some("intg"),
        _ => None,
    }
}

/// Every `$b` of `tag` that is a code of `scheme`, once each, in order.
fn coded(marc: &Marc<'_>, tag: [u8; 3], property: &str, scheme: &str) -> Vec<Statement> {
    let Some(known) = vocabulary::scheme(scheme) else {
        return Vec::new();
    };
    let mut codes: Vec<String> = Vec::new();
    let subfields = marc.0.fields.iter().filter_map(|field| match field {
        Field::Data {
            tag: t, subfields, ..
        } if *t.as_bytes() == tag => Some(subfields),
        _ => None,
    });
    for field in subfields {
        for value in field
            .iter()
            .filter(|subfield| subfield.code == b'b')
            .map(|subfield| subfield.value.as_slice())
        {
            if let Some(code) = super::text::trimmed(value)
                && known.concept(&code).is_some()
                && !codes.contains(&code)
            {
                codes.push(code);
            }
        }
    }
    codes
        .iter()
        .map(|code| concept(property, scheme, code))
        .collect()
}

#[cfg(test)]
mod tests {
    use elzevir_marc::{Record, Subfield, Tag};

    use super::*;

    fn record(leader: [u8; 2], fields: &[(&[u8; 3], &[&str])]) -> Record {
        // Leader/18 blank: no description conventions, so no administrative metadata.
        let mut full = *b"00000nam a2200000   4500";
        full[6] = leader[0];
        full[7] = leader[1];
        Record {
            leader: full,
            fields: fields
                .iter()
                .map(|(tag, codes)| Field::Data {
                    tag: Tag::new(**tag),
                    indicators: *b"  ",
                    subfields: codes
                        .iter()
                        .map(|code| Subfield {
                            code: b'b',
                            value: code.as_bytes().to_vec(),
                        })
                        .collect(),
                })
                .collect(),
        }
    }

    #[test]
    fn the_leader_gives_classes_content_and_issuance() {
        let book = record(*b"am", &[]);
        let work = work(&Marc(&book));
        assert_eq!(work.classes, ["Text", "Monograph"]);
        assert_eq!(work.statements, [concept("content", "contentTypes", "txt")]);
        assert_eq!(
            instance(&Marc(&book)).statements,
            [concept("issuance", "issuance", "mono")]
        );
        let manuscript_map = record(*b"fc", &[]);
        assert_eq!(
            super::work(&Marc(&manuscript_map)).classes,
            ["Cartography", "Manuscript", "Collection"]
        );
        assert!(super::work(&Marc(&record(*b"t ", &[]))).classes.is_empty());
    }

    #[test]
    fn fields_334_to_338_win_over_the_leader_and_unknown_codes_are_dropped() {
        let disc = record(
            *b"jm",
            &[
                (b"336", &["prm", "txt"]),
                (b"337", &["s"]),
                (b"338", &["sd", "XX"]),
                (b"334", &["mono"]),
            ],
        );
        let marc = Marc(&disc);
        assert_eq!(work(&marc).classes, ["MusicAudio", "Monograph"]);
        assert_eq!(
            work(&marc).statements,
            [
                concept("content", "contentTypes", "prm"),
                concept("content", "contentTypes", "txt")
            ]
        );
        assert_eq!(
            instance(&marc).statements,
            [
                concept("issuance", "issuance", "mono"),
                concept("media", "mediaTypes", "s"),
                concept("carrier", "carriers", "sd")
            ]
        );
    }
}
