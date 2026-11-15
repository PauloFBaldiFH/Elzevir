// SPDX-License-Identifier: GPL-3.0-or-later
//! Geographic coverage, the languages of the parts and the scripts of the Work back to
//! `043 $a`, `041` and `546 $b` (bibframe-completo.md, item 29, part 5e).
//!
//! These values share their fields with others (`041 $a`, `546 $a`), so each is compared
//! with what the record being written holds now and only its own subfields are replaced
//! when they differ.

use elzevir_domain::statement::{Description, Statement, Value, is_language_part};
use elzevir_marc::{Field, Record, Subfield, Tag};

use super::super::Marc;
use super::super::content::{
    GEOGRAPHIC_AREAS, PARTS, geographic_coverage, language_parts, notations, part_code,
};
use super::fields::insert_in_tag_order;

/// Writes the three groups of the Work that differ from what `out` holds.
pub(super) fn write(out: &mut Record, work: &Description) {
    write_coverage(out, work);
    write_language_parts(out, work);
    write_notations(out, work);
}

fn write_coverage(out: &mut Record, work: &Description) {
    let wanted: Vec<&Statement> = work
        .statements
        .iter()
        .filter(|s| area(s).is_some())
        .collect();
    if differs(&wanted, &geographic_coverage(&Marc(out))) {
        // The list pads every code to seven characters.
        let subfields = wanted
            .iter()
            .filter_map(|s| area(s))
            .map(|code| subfield(b'a', &format!("{code:-<7}")))
            .collect();
        replace(out, *b"043", |_| true, b"a", subfields, *b"  ");
    }
}

/// The geographic area code of a `bf:geographicCoverage` from the MARC list.
fn area(statement: &Statement) -> Option<&str> {
    match &statement.value {
        Value::Iri(iri) if statement.property == "geographicCoverage" => {
            iri.strip_prefix(GEOGRAPHIC_AREAS)
        }
        _ => None,
    }
}

fn write_language_parts(out: &mut Record, work: &Description) {
    let wanted: Vec<&Statement> = work
        .statements
        .iter()
        .filter(|s| is_language_part(s))
        .collect();
    if !differs(&wanted, &language_parts(&Marc(out))) {
        return;
    }
    let mut subfields: Vec<Subfield> = wanted
        .iter()
        .filter_map(|s| {
            let Value::Resource(language) = &s.value else {
                return None;
            };
            let code = part_code(literal(&language.statements, "part")?)?;
            Some(subfield(code, literal(&language.statements, "code")?))
        })
        .collect();
    let has_field = out.fields.iter().any(|f| {
        matches!(f, Field::Data { tag, indicators, .. }
            if tag.as_bytes() == b"041" && indicators[1] == b' ')
    });
    // A single language is written only in `008/35-37`; a new `041` for the parts starts
    // with it, so that the field says the language of the resource too.
    if !has_field
        && !subfields.is_empty()
        && let Some(language) = language_of_008(out)
    {
        subfields.insert(0, subfield(b'a', &language));
    }
    let original = subfields.iter().any(|s| s.code == b'h');
    let codes: Vec<u8> = PARTS.iter().map(|(code, _)| *code).collect();
    // Only the fields with MARC codes hold the parts the model reads.
    replace(
        out,
        *b"041",
        |indicators| indicators[1] == b' ',
        &codes,
        subfields,
        *b"0 ",
    );
    if original
        && let Some(Field::Data { indicators, .. }) = out
            .fields
            .iter_mut()
            .find(|f| matches!(f, Field::Data { tag, indicators, .. } if tag.as_bytes() == b"041" && indicators[1] == b' '))
    {
        // A record with the language of the original is or includes a translation.
        indicators[0] = b'1';
    }
}

/// The language code of `008/35-37`, when it holds one.
fn language_of_008(out: &Record) -> Option<String> {
    out.fields.iter().find_map(|field| match field {
        Field::Control { tag, value } if tag.as_bytes() == b"008" => value
            .get(35..38)
            .filter(|code| code.iter().all(u8::is_ascii_alphabetic))
            .map(|code| String::from_utf8_lossy(code).into_owned()),
        _ => None,
    })
}

fn write_notations(out: &mut Record, work: &Description) {
    let wanted: Vec<&Statement> = work
        .statements
        .iter()
        .filter(|s| s.property == "notation")
        .collect();
    if !differs(&wanted, &notations(&Marc(out))) {
        return;
    }
    let subfields = wanted
        .iter()
        .filter_map(|s| match &s.value {
            Value::Resource(notation) => notation.label.as_deref(),
            _ => None,
        })
        .map(|label| subfield(b'b', label))
        .collect();
    replace(out, *b"546", |_| true, b"b", subfields, *b"  ");
}

fn differs(wanted: &[&Statement], current: &[Statement]) -> bool {
    wanted.len() != current.len() || wanted.iter().zip(current).any(|(w, c)| *w != c)
}

/// Removes `codes` from every `tag` field that `selected` takes, puts `subfields` at the
/// end of the first one (a new field with `indicators` when there is none) and drops the
/// fields left empty.
fn replace(
    out: &mut Record,
    tag: [u8; 3],
    selected: impl Fn([u8; 2]) -> bool,
    codes: &[u8],
    subfields: Vec<Subfield>,
    indicators: [u8; 2],
) {
    let mut pending = Some(subfields);
    for field in &mut out.fields {
        if let Field::Data {
            tag: t,
            indicators: i,
            subfields: present,
        } = field
            && *t.as_bytes() == tag
            && selected(*i)
        {
            present.retain(|s| !codes.contains(&s.code));
            if let Some(new) = pending.take() {
                present.extend(new);
            }
        }
    }
    out.fields.retain(|field| {
        !matches!(field, Field::Data { tag: t, subfields, .. }
            if *t.as_bytes() == tag && subfields.is_empty())
    });
    if let Some(new) = pending.filter(|new| !new.is_empty()) {
        insert_in_tag_order(
            out,
            vec![Field::Data {
                tag: Tag::new(tag),
                indicators,
                subfields: new,
            }],
        );
    }
}

fn literal<'a>(statements: &'a [Statement], property: &str) -> Option<&'a str> {
    statements.iter().find_map(|s| match &s.value {
        Value::Literal { text, .. } if s.property == property => Some(text.as_str()),
        _ => None,
    })
}

fn subfield(code: u8, value: &str) -> Subfield {
    Subfield {
        code,
        value: value.as_bytes().to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::content::{language_part, notation, of_work};
    use super::super::fields::data_field;
    use super::*;

    #[test]
    fn coverage_parts_and_scripts_come_back_and_keep_their_neighbours() {
        let source = Record {
            leader: *b"00000nam a2200000 i 4500",
            fields: vec![
                data_field(*b"041", *b"1 ", &[(b'a', "por"), (b'h', "fre")]),
                data_field(*b"043", *b"  ", &[(b'a', "s-bl---")]),
                data_field(
                    *b"546",
                    *b"  ",
                    &[(b'a', "Em português."), (b'b', "Latino")],
                ),
            ],
        };
        let mut work = Description {
            classes: Vec::new(),
            statements: of_work(&Marc(&source)),
        };
        // Unchanged values leave the record alone.
        let mut out = source.clone();
        write(&mut out, &work);
        assert_eq!(out, source);
        // Changed ones replace only their own subfields.
        work.statements = vec![
            Statement {
                property: "geographicCoverage".to_owned(),
                value: Value::Iri(format!("{GEOGRAPHIC_AREAS}n-us")),
            },
            language_part("eng", "original"),
            language_part("spa", "summary"),
            notation("Cirílico".to_owned()),
        ];
        write(&mut out, &work);
        assert_eq!(
            out.fields,
            [
                data_field(
                    *b"041",
                    *b"1 ",
                    &[(b'a', "por"), (b'h', "eng"), (b'b', "spa")]
                ),
                data_field(*b"043", *b"  ", &[(b'a', "n-us---")]),
                data_field(
                    *b"546",
                    *b"  ",
                    &[(b'a', "Em português."), (b'b', "Cirílico")]
                ),
            ]
        );
        assert_eq!(of_work(&Marc(&out)), work.statements);
        // Removed ones take their fields with them when nothing else is left.
        work.statements.clear();
        write(&mut out, &work);
        assert_eq!(
            out.fields,
            [
                data_field(*b"041", *b"1 ", &[(b'a', "por")]),
                data_field(*b"546", *b"  ", &[(b'a', "Em português.")]),
            ]
        );
    }

    #[test]
    fn a_new_041_for_the_parts_starts_with_the_language_of_the_008() {
        let mut out = Record {
            leader: *b"00000nam a2200000 i 4500",
            fields: vec![Field::Control {
                tag: Tag::new(*b"008"),
                value: b"250321s1995    bl            000 0 por d".to_vec(),
            }],
        };
        let work = Description {
            classes: Vec::new(),
            statements: vec![language_part("eng", "original")],
        };
        write(&mut out, &work);
        assert_eq!(
            out.fields.last(),
            Some(&data_field(
                *b"041",
                *b"1 ",
                &[(b'a', "por"), (b'h', "eng")]
            ))
        );
    }
}
