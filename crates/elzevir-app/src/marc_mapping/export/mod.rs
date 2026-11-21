// SPDX-License-Identifier: GPL-3.0-or-later
//! `bibframe_to_marc`: the model back to MARC21 (docs/specs/catalogo/mapeamento-marc21.md, §7).
//!
//! The original record is the starting point. Only the parts of the model that differ from
//! what the original maps to are rewritten, and a rewritten list reuses every original field
//! whose mapping did not change. An unedited record therefore comes back byte for byte.

use elzevir_domain::catalog::{
    AgentKind, AuthorityLinkKind, IdentifierKind, NoteKind, SubjectKind,
};
use elzevir_marc::{Field, Record, Subfield, Tag};
use std::borrow::Cow;

use super::instance::{IDENTIFIER_TAGS, NOTE_TAGS, identifier_of, note_of};
use super::items::{defaults, item_of};
use super::work::{CONTRIBUTION_TAGS, SUBJECT_TAGS, contribution_of, name_part_code, subject_of};
use super::{DataField, Marc, marc_to_bibframe, responsibility};
use crate::catalog::{
    AuthorityLink, ItemData, NamePart, NewContribution, NewIdentifier, NewNote, NewRecord,
    NewSubject,
};

mod admin;
mod books;
mod classification;
mod coded;
mod content;
mod described;
mod fields;
mod fixed;
mod form;
mod locators;
mod national;
mod notes;
mod physical;
mod provision;
mod relations;
mod temporal;
mod titles;

use fields::{data_field, first_index, insert_in_tag_order, set_008, set_subfield};

/// Writes a model record as MARC21, starting from `original` when there is one.
///
/// Hubs as records of their own are not written in this version (spec §6).
#[must_use]
pub fn bibframe_to_marc(model: &NewRecord, original: Option<&Record>) -> Record {
    let before = original.and_then(|record| marc_to_bibframe(record, None).ok());
    let mut out = original.cloned().unwrap_or_else(|| Record {
        leader: *b"00000nam a2200000 i 4500",
        fields: Vec::new(),
    });
    let changed = |part: &dyn Fn(&NewRecord) -> bool| before.as_ref().is_none_or(|b| !part(b));
    if changed(&|b| b.work_description.classes == model.work_description.classes) {
        set_leader_type(&mut out, &model.work_description.classes);
    }
    if changed(&|b| b.instance.description.classes == model.instance.description.classes) {
        set_electronic(&mut out, &model.instance.description.classes);
    }
    if changed(&|b| b.instance.opac_hidden == model.instance.opac_hidden) {
        set_opac_hidden(&mut out, model.instance.opac_hidden);
    }
    coded::write(
        &mut out,
        &model.work_description,
        &model.instance.description,
        before
            .as_ref()
            .map(|b| (&b.work_description, &b.instance.description)),
    );
    for group in [
        &titles::TITLES,
        &notes::NOTES,
        &relations::RELATIONS,
        &locators::LOCATORS,
        &national::NATIONAL,
        &form::FORMS,
    ] {
        described::write(
            &mut out,
            group,
            &model.work_description,
            &model.instance.description,
            before
                .as_ref()
                .map(|b| (&b.work_description, &b.instance.description)),
        );
    }
    write_titles(&mut out, model, before.as_ref());
    write_publication(&mut out, model, before.as_ref());
    provision::write(
        &mut out,
        &model.instance.description,
        before.as_ref().map(|b| &b.instance.description),
    );
    write_lists(&mut out, model, before.as_ref());
    content::write(&mut out, &model.work_description);
    temporal::write(&mut out, &model.work_description);
    physical::write(&mut out, &model.instance.description);
    admin::write(&mut out, &model.instance.description);
    books::write(
        &mut out,
        &model.work_description,
        before.as_ref().map(|b| &b.work_description),
    );
    fixed::write(
        &mut out,
        &model.work_description,
        &model.instance.description,
        before
            .as_ref()
            .map(|b| (&b.work_description, &b.instance.description)),
    );
    out
}

/// Koha's `942 $n`, the reverse of the import: `1` when hidden; when shown, `0` only where
/// the record already has the subfield, so that a record that never had it gains none.
fn set_opac_hidden(out: &mut Record, hidden: bool) {
    let has_subfield = first_index(out, *b"942")
        .and_then(|index| out.fields.get(index))
        .is_some_and(|field| match field {
            Field::Data { subfields, .. } => subfields.iter().any(|subfield| subfield.code == b'n'),
            Field::Control { .. } => false,
        });
    if hidden || has_subfield {
        set_subfield(
            out,
            *b"942",
            *b"  ",
            b'n',
            Some(if hidden { "1" } else { "0" }),
        );
    }
}

/// Leader/06 and /07 from the Work's classes, the reverse of the import (bibframe-
/// completo.md, item 10j). A position no class speaks for keeps its byte.
fn set_leader_type(out: &mut Record, classes: &[String]) {
    let has = |class: &str| classes.iter().any(|name| name == class);
    let manuscript = has("Manuscript");
    let record_type = classes.iter().find_map(|class| match class.as_str() {
        "Text" if manuscript => Some(b't'),
        "Text" => Some(b'a'),
        "NotatedMusic" if manuscript => Some(b'd'),
        "NotatedMusic" => Some(b'c'),
        "Cartography" if manuscript => Some(b'f'),
        "Cartography" => Some(b'e'),
        "MovingImage" => Some(b'g'),
        "NonMusicAudio" => Some(b'i'),
        "MusicAudio" | "Audio" => Some(b'j'),
        "StillImage" => Some(b'k'),
        "Multimedia" | "Kit" => Some(b'm'),
        "MixedMaterial" => Some(b'p'),
        "Object" => Some(b'r'),
        _ => None,
    });
    let level = classes.iter().find_map(|class| match class.as_str() {
        "Monograph" => Some(b'm'),
        "Serial" => Some(b's'),
        "Collection" => Some(b'c'),
        "Integrating" => Some(b'i'),
        _ => None,
    });
    for (position, byte) in [(6, record_type), (7, level)] {
        if let (Some(slot), Some(byte)) = (out.leader.get_mut(position), byte) {
            *slot = byte;
        }
    }
}

/// An electronic Instance gets a `007` for a remote electronic resource when it has none.
fn set_electronic(out: &mut Record, classes: &[String]) {
    let has_007 = out.fields.iter().any(|f| f.tag().as_bytes() == b"007");
    if classes.iter().any(|class| class == "Electronic") && !has_007 {
        insert_in_tag_order(
            out,
            vec![Field::Control {
                tag: Tag::new(*b"007"),
                value: b"cr".to_vec(),
            }],
        );
    }
}

fn write_titles(out: &mut Record, model: &NewRecord, before: Option<&NewRecord>) {
    let instance = &model.instance;
    let statement = responsibility::of_description(&instance.description);
    if before.is_none_or(|b| {
        b.instance.title != instance.title
            || responsibility::of_description(&b.instance.description) != statement
    }) {
        write_245(out, model, before, &statement);
    }
    if before.is_none_or(|b| b.work_title != model.work_title) {
        let main_title = instance.title.split(" : ").next().unwrap_or_default();
        let tag = if first_index(out, *b"130").is_some() {
            Some(*b"130")
        } else if first_index(out, *b"240").is_some() || model.work_title != main_title {
            Some(*b"240")
        } else {
            None
        };
        if let Some(tag) = tag {
            set_subfield(out, tag, *b"10", b'a', Some(&model.work_title));
        }
    }
    let scalar =
        |part: fn(&NewRecord) -> &Option<String>| before.is_none_or(|b| part(b) != part(model));
    if scalar(|r| &r.instance.edition) {
        set_subfield(out, *b"250", *b"  ", b'a', instance.edition.as_deref());
    }
    if scalar(|r| &r.summary) {
        set_subfield(out, *b"520", *b"  ", b'a', model.summary.as_deref());
    }
    if scalar(|r| &r.audience) {
        set_subfield(out, *b"521", *b"  ", b'a', model.audience.as_deref());
    }
    if scalar(|r| &r.instance.extent) {
        set_subfield(out, *b"300", *b"  ", b'a', instance.extent.as_deref());
        // The extent is `$a $f $g`; `$b`, `$c` and `$e` are statements.
        set_subfield(out, *b"300", *b"  ", b'f', None);
        set_subfield(out, *b"300", *b"  ", b'g', None);
    }
    if scalar(|r| &r.instance.series) {
        let (name, number) = match instance
            .series
            .as_deref()
            .map(|s| s.rsplit_once(" ; ").unwrap_or((s, "")))
        {
            Some((name, number)) => (Some(name), (!number.is_empty()).then_some(number)),
            None => (None, None),
        };
        set_subfield(out, *b"490", *b"0 ", b'a', name);
        set_subfield(out, *b"490", *b"0 ", b'v', number);
    }
    classification::write(out, model, before);
}

/// The `245` with the ISBD punctuation of the LC way back: ` :` before the subtitle, ` /`
/// before the statement of responsibility and a period at the end of `$b` or `$c`. The
/// first indicator says whether the title is an added entry: it is when the Work has a main
/// entry or a preferred title of its own; the second (characters to ignore) has no source
/// in the model and is `0`.
fn write_245(out: &mut Record, model: &NewRecord, before: Option<&NewRecord>, statements: &[&str]) {
    let statement = statements.first();
    let title = model.instance.title.as_str();
    let (main, rest) = match title.split_once(" : ") {
        Some((main, rest)) => (main, Some(rest)),
        None => (title, None),
    };
    let added = model.contributions.iter().any(|c| c.primary) || model.work_title != main;
    let indicators = [if added { b'1' } else { b'0' }, b'0'];
    let main = match (rest, statement) {
        (Some(_), _) => format!("{main} :"),
        (None, Some(_)) => format!("{main} /"),
        (None, None) => main.to_owned(),
    };
    let rest = rest.map(|rest| match statement {
        Some(_) => format!("{rest} /"),
        None => closed(rest),
    });
    if let Some(previous) = before
        && let Some(index) = first_index(out, *b"245")
        && let Some(Field::Data { subfields, .. }) = out.fields.get_mut(index)
    {
        let (old_main, old_rest) = previous
            .instance
            .title
            .split_once(" : ")
            .map_or((previous.instance.title.as_str(), None), |(a, b)| {
                (a, Some(b))
            });
        let (new_main, new_rest) = title
            .split_once(" : ")
            .map_or((title, None), |(a, b)| (a, Some(b)));
        if old_main != new_main {
            // The display title includes subsequent $n/$p and repeated $a. Leave those
            // occurrences in place when the main title is edited in the catalog.
            let suffix = subfields
                .iter()
                .filter(|s| b"anp".contains(&s.code))
                .skip(1)
                .filter_map(|s| super::text::clean(&s.value))
                .collect::<Vec<_>>()
                .join(" ");
            let value = new_main
                .strip_suffix(&format!(" {suffix}"))
                .unwrap_or(new_main);
            edit_first(subfields, b'a', Some(value));
        }
        if old_rest != new_rest {
            edit_first(subfields, b'b', rest.as_deref());
        }
        let prior = responsibility::of_description(&previous.instance.description);
        if prior != statements {
            edit_responsibilities(subfields, &prior, statements);
        }
        return;
    }
    set_subfield(out, *b"245", indicators, b'a', Some(&main));
    set_subfield(out, *b"245", indicators, b'b', rest.as_deref());
    if let Some(index) = first_index(out, *b"245")
        && let Some(Field::Data { subfields, .. }) = out.fields.get_mut(index)
    {
        edit_responsibilities(subfields, &[], statements);
    }
}

fn edit_responsibilities(fields: &mut Vec<Subfield>, prior: &[&str], edited: &[&str]) {
    let mut values = edited.iter();
    let mut old = prior.iter();
    fields.retain_mut(|s| {
        if s.code != b'c' {
            return true;
        }
        let Some(value) = values.next() else {
            return false;
        };
        if old.next() != Some(value) {
            s.value = closed(value).into_bytes();
        }
        true
    });
    fields.extend(values.map(|value| Subfield {
        code: b'c',
        value: closed(value).into_bytes(),
    }));
}

fn edit_first(subfields: &mut Vec<Subfield>, code: u8, value: Option<&str>) {
    let at = subfields.iter().position(|s| s.code == code);
    match (at, value) {
        (Some(i), Some(text)) => {
            if let Some(s) = subfields.get_mut(i) {
                s.value = text.as_bytes().to_vec();
            }
        }
        (Some(i), None) => {
            subfields.remove(i);
        }
        (None, Some(text)) => subfields.push(Subfield {
            code,
            value: text.as_bytes().to_vec(),
        }),
        (None, None) => {}
    }
}

/// The ISBD punctuation of the publication area, as the LC way back writes it: ` ;`
/// between places, ` :` before a publisher, `,` before the date and a period at the end.
fn with_publication_punctuation(subfields: Vec<(u8, String)>) -> Vec<(u8, String)> {
    let next_codes: Vec<Option<u8>> = subfields
        .iter()
        .skip(1)
        .map(|(code, _)| Some(*code))
        .chain([None])
        .collect();
    subfields
        .into_iter()
        .zip(next_codes)
        .map(|((code, value), next)| {
            let value = match next {
                Some(b'a') => format!("{value} ;"),
                Some(b'b') => format!("{value} :"),
                Some(_) => format!("{value},"),
                None => closed(&value),
            };
            (code, value)
        })
        .collect()
}

/// `text` ending with a period, unless it already ends with one.
fn closed(text: &str) -> String {
    if text.ends_with('.') {
        text.to_owned()
    } else {
        format!("{text}.")
    }
}

fn write_publication(out: &mut Record, model: &NewRecord, before: Option<&NewRecord>) {
    let instance = &model.instance;
    let parts = |r: &NewRecord| {
        (
            r.instance.place.clone(),
            r.instance.publisher.clone(),
            r.instance.date.clone(),
        )
    };
    if before.is_some_and(|b| parts(b) == parts(model)) {
        return;
    }
    // Several places and publishers were joined as ISBD does; each goes back to its own
    // subfield, in the order `$a`, `$b`, `$c`, before the subfields the model does not hold.
    let split = |text: &Option<String>, separator: &str| -> Vec<String> {
        text.as_deref()
            .map(|t| t.split(separator).map(str::to_owned).collect())
            .unwrap_or_default()
    };
    let mut wanted: Vec<(u8, String)> = Vec::new();
    wanted.extend(split(&instance.place, " ; ").into_iter().map(|v| (b'a', v)));
    wanted.extend(
        split(&instance.publisher, " : ")
            .into_iter()
            .map(|v| (b'b', v)),
    );
    wanted.extend(instance.date.iter().map(|v| (b'c', v.clone())));
    let wanted = with_publication_punctuation(wanted);
    let index = out
        .fields
        .iter()
        .position(|f| {
            matches!(f, Field::Data { tag, indicators, .. }
                if tag.as_bytes() == b"264" && indicators[1] == b'1')
        })
        .or_else(|| first_index(out, *b"260"));
    let Some(index) = index else {
        if !wanted.is_empty() {
            let subfields: Vec<(u8, &str)> = wanted.iter().map(|(c, v)| (*c, v.as_str())).collect();
            // AACR 2 publishes in `260`; RDA, and the LC way back, in `264`.
            let field = if admin::description_follows(&instance.description, "aacr") {
                data_field(*b"260", *b"  ", &subfields)
            } else {
                data_field(*b"264", *b" 1", &subfields)
            };
            insert_in_tag_order(out, vec![field]);
        }
        return;
    };
    let Some(Field::Data { subfields, .. }) = out.fields.get_mut(index) else {
        return;
    };
    let kept: Vec<Subfield> = subfields
        .iter()
        .filter(|s| !matches!(s.code, b'a' | b'b' | b'c'))
        .cloned()
        .collect();
    *subfields = wanted
        .into_iter()
        .map(|(code, value)| Subfield {
            code,
            value: value.into_bytes(),
        })
        .chain(kept)
        .collect();
    if subfields.is_empty() {
        out.fields.remove(index);
    }
}

fn write_lists(out: &mut Record, model: &NewRecord, before: Option<&NewRecord>) {
    if before.is_none_or(|b| b.languages != model.languages) {
        write_languages(out, &model.languages);
    }
    if before.is_none_or(|b| b.contributions != model.contributions) {
        let current = mapped(out, &CONTRIBUTION_TAGS, |tag, field| {
            contribution_of(tag, field)
        });
        rebuild(
            out,
            &current,
            &model.contributions,
            contribution_field,
            |_, _| None,
        );
    }
    if before.is_none_or(|b| b.subjects != model.subjects) {
        let current = mapped(out, &SUBJECT_TAGS, subject_of);
        rebuild(out, &current, &model.subjects, subject_field, |_, _| None);
    }
    if before.is_none_or(|b| b.instance.identifiers != model.instance.identifiers) {
        let current = mapped(out, &IDENTIFIER_TAGS, |tag, field| {
            identifier_of(tag, field)
        });
        rebuild(
            out,
            &current,
            &model.instance.identifiers,
            identifier_field,
            |_, _| None,
        );
    }
    if before.is_none_or(|b| b.instance.notes != model.instance.notes) {
        let current = mapped(out, &NOTE_TAGS, note_of);
        rebuild(out, &current, &model.instance.notes, note_field, |_, _| {
            None
        });
    }
    if before.is_none_or(|b| b.items != model.items) {
        let defaults = defaults(&Marc(out));
        let mut position = 0;
        let current = mapped(out, &[b"952"], |_, field| {
            let item = item_of(position, field, &defaults).ok();
            position += 1;
            item
        });
        rebuild(
            out,
            &current,
            model.items.marc(),
            |item| item_field(item, None),
            |item, current| {
                // A changed Item keeps the subfields Koha maintains (`$9`, `$6`...) of the
                // original field with the same barcode.
                item.barcode.as_ref().and_then(|barcode| {
                    current
                        .iter()
                        .find(|(_, original, _)| original.barcode.as_ref() == Some(barcode))
                        .map(|(_, _, field)| item_field(item, Some(field)))
                })
            },
        );
    }
}

/// The languages of the Work to `008/35-37` and `041 $a`.
fn write_languages(out: &mut Record, languages: &[String]) {
    let first = first_index(out, *b"041");
    let kept: Vec<Subfield> = first
        .and_then(|index| match out.fields.get(index) {
            Some(Field::Data { subfields, .. }) => Some(
                subfields
                    .iter()
                    .filter(|s| s.code != b'a')
                    .cloned()
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default();
    out.fields.retain(|field| field.tag().as_bytes() != b"041");
    // `008/35-37` holds the first language, which a three-letter code fills.
    let first_code = languages
        .first()
        .filter(|code| code.len() == 3)
        .map_or("   ", String::as_str);
    if !languages.is_empty() || first_index(out, *b"008").is_some() {
        set_008(out, 35, first_code.as_bytes());
    }
    // As the LC way back, `041` is written only for more than one language; a single one
    // is already in `008`. Other subfields of the field (the language of the original...)
    // keep it.
    if languages.len() < 2 && kept.is_empty() {
        return;
    }
    let mut subfields: Vec<Subfield> = languages
        .iter()
        .map(|code| Subfield {
            code: b'a',
            value: code.as_bytes().to_vec(),
        })
        .collect();
    subfields.extend(kept);
    let field = Field::Data {
        tag: Tag::new(*b"041"),
        indicators: *b"0 ",
        subfields,
    };
    match first {
        Some(index) if index <= out.fields.len() => out.fields.insert(index, field),
        _ => insert_in_tag_order(out, vec![field]),
    }
}

/// The fields with one of `tags` that map to a value, with their index and a copy.
fn mapped<T>(
    out: &Record,
    tags: &[&[u8; 3]],
    mut map: impl FnMut([u8; 3], DataField<'_>) -> Option<T>,
) -> Vec<(usize, T, Field)> {
    out.fields
        .iter()
        .enumerate()
        .filter_map(|(index, field)| {
            let Field::Data {
                tag,
                indicators,
                subfields,
            } = field
            else {
                return None;
            };
            if !tags.contains(&tag.as_bytes()) {
                return None;
            }
            map(
                *tag.as_bytes(),
                DataField {
                    indicators: *indicators,
                    subfields,
                },
            )
            .map(|value| (index, value, field.clone()))
        })
        .collect()
}

/// Replaces the mapped fields with one field per wanted value: an unchanged original field
/// when one maps to exactly that value, else `changed` (from a related original), else `new`.
fn rebuild<T: PartialEq>(
    out: &mut Record,
    current: &[(usize, T, Field)],
    wanted: &[T],
    new: impl Fn(&T) -> Field,
    changed: impl Fn(&T, &[(usize, T, Field)]) -> Option<Field>,
) {
    let mut used = vec![false; current.len()];
    let fields: Vec<Field> = wanted
        .iter()
        .map(|value| {
            let reusable = current
                .iter()
                .zip(used.iter_mut())
                .find(|((_, original, _), used)| !**used && original == value);
            match reusable {
                Some(((_, _, field), used)) => {
                    *used = true;
                    field.clone()
                }
                None => changed(value, current).unwrap_or_else(|| new(value)),
            }
        })
        .collect();
    let insert_at = current.first().map(|(index, _, _)| *index);
    for (index, _, _) in current.iter().rev() {
        if *index < out.fields.len() {
            out.fields.remove(*index);
        }
    }
    match insert_at {
        Some(index) if index <= out.fields.len() => {
            out.fields.splice(index..index, fields);
        }
        _ => insert_in_tag_order(out, fields),
    }
}

fn contribution_field(contribution: &NewContribution) -> Field {
    let (main, added, first_indicator) = match contribution.kind {
        AgentKind::Person => (*b"100", *b"700", b'1'),
        AgentKind::Family => (*b"100", *b"700", b'3'),
        AgentKind::Organization => (*b"110", *b"710", b'2'),
        AgentKind::Meeting => (*b"111", *b"711", b'2'),
    };
    let tag = if contribution.primary { main } else { added };
    // `$e` the term, the identifiers, then `$4` the code, as the records write them.
    let role = contribution.role.as_deref();
    let term = contribution.role_term.as_deref();
    let code = role.filter(|role| {
        role.len() == 3
            && role.bytes().all(|b| b.is_ascii_lowercase())
            && term.is_none_or(|term| term.to_lowercase() != *role)
    });
    let term = term.or(role.filter(|_| code.is_none()));
    let mut name: Vec<(u8, Cow<'_, str>)> = name_subfields(
        contribution.kind,
        &contribution.heading,
        &contribution.name_parts,
    )
    .into_iter()
    .map(|(code, text)| (code, Cow::Borrowed(text)))
    .collect();
    // ISBD puts a comma between the name and the relator term.
    if term.is_some()
        && let Some((_, last)) = name.last_mut()
        && !last.ends_with([',', '-'])
    {
        last.to_mut().push(',');
    }
    let mut subfields: Vec<(u8, &str)> = name.iter().map(|(code, text)| (*code, &**text)).collect();
    subfields.extend(term.map(|term| (b'e', term)));
    subfields.extend(contribution.identifiers.iter().map(link_subfield));
    subfields.extend(code.map(|code| (b'4', code)));
    data_field(tag, [first_indicator, b' '], &subfields)
}

/// The heading as `$a` and a subfield for each name part, cut where the part starts; a
/// heading whose parts do not fit it goes out whole in `$a`.
fn name_subfields<'a>(kind: AgentKind, heading: &'a str, parts: &[NamePart]) -> Vec<(u8, &'a str)> {
    let mut cuts = Vec::new();
    let mut previous = 0;
    for part in parts {
        // Characters to bytes; a part outside the heading or out of order is no cut.
        let Some((at, _)) = heading.char_indices().nth(part.start) else {
            return vec![(b'a', heading)];
        };
        if at <= previous {
            return vec![(b'a', heading)];
        }
        cuts.push((at, name_part_code(kind, part.kind)));
        previous = at;
    }
    let mut subfields = Vec::new();
    let mut from = 0;
    let mut code = b'a';
    for (at, next) in cuts {
        subfields.push((code, heading.get(from..at).unwrap_or_default().trim_end()));
        from = at;
        code = next;
    }
    subfields.push((code, heading.get(from..).unwrap_or_default()));
    subfields
}

/// A link as `$0` (an authority record) or `$1` (the entity itself).
fn link_subfield(link: &AuthorityLink) -> (u8, &str) {
    let code = match link.kind {
        AuthorityLinkKind::AuthorityRecord => b'0',
        AuthorityLinkKind::RealWorldObject => b'1',
    };
    (code, link.value.as_str())
}

fn subject_field(subject: &NewSubject) -> Field {
    let (tag, first_indicator) = match subject.kind {
        SubjectKind::Person => (*b"600", b'1'),
        SubjectKind::Family => (*b"600", b'3'),
        SubjectKind::Organization => (*b"610", b'2'),
        SubjectKind::Meeting => (*b"611", b'2'),
        SubjectKind::Title => (*b"630", b'0'),
        SubjectKind::Temporal => (*b"648", b' '),
        SubjectKind::Topic => (*b"650", b' '),
        SubjectKind::Place => (*b"651", b' '),
        SubjectKind::GenreForm => (*b"655", b' '),
    };
    let (second_indicator, source) = match subject.thesaurus.as_deref() {
        None => (b'4', None),
        Some("lcsh") => (b'0', None),
        Some("lcac") => (b'1', None),
        Some("mesh") => (b'2', None),
        Some("nal") => (b'3', None),
        Some("cash") => (b'5', None),
        Some("rvm") => (b'6', None),
        Some(other) => (b'7', Some(other)),
    };
    // The name cut into its parts, the subdivisions, the links, then the thesaurus.
    let mut subfields: Vec<(u8, &str)> = Vec::new();
    for (kind, part) in subject_parts(subject) {
        match (kind, subject.kind.agent()) {
            (Some(kind), _) => subfields.push((subdivision_code(kind), part)),
            (None, Some(agent)) => {
                subfields.extend(name_subfields(agent, part, &subject.name_parts));
            }
            (None, None) => subfields.push((b'a', part)),
        }
    }
    subfields.extend(subject.identifiers.iter().map(link_subfield));
    subfields.extend(source.map(|source| (b'2', source)));
    data_field(tag, [first_indicator, second_indicator], &subfields)
}

/// The main heading (`None`) and each subdivision with its kind. The typed subdivisions are
/// the last parts of the heading; the parts before them, after the first, are topics.
fn subject_parts(subject: &NewSubject) -> Vec<(Option<SubjectKind>, &str)> {
    let parts: Vec<&str> = subject.heading.split(" -- ").collect();
    let typed = subject
        .subdivisions
        .len()
        .min(parts.len().saturating_sub(1));
    let untyped = parts.len() - typed;
    let kinds = subject
        .subdivisions
        .iter()
        .skip(subject.subdivisions.len() - typed);
    parts
        .iter()
        .take(untyped)
        .enumerate()
        .map(|(position, part)| ((position > 0).then_some(SubjectKind::Topic), *part))
        .chain(
            parts
                .iter()
                .skip(untyped)
                .zip(kinds)
                .map(|(part, kind)| (Some(*kind), *part)),
        )
        .collect()
}

/// The subfield of a subdivision: `$v` genre or form, `$y` period, `$z` place, `$x` the rest.
const fn subdivision_code(kind: SubjectKind) -> u8 {
    match kind {
        SubjectKind::GenreForm => b'v',
        SubjectKind::Temporal => b'y',
        SubjectKind::Place => b'z',
        SubjectKind::Topic
        | SubjectKind::Title
        | SubjectKind::Person
        | SubjectKind::Organization
        | SubjectKind::Meeting
        | SubjectKind::Family => b'x',
    }
}

fn identifier_field(identifier: &NewIdentifier) -> Field {
    let (tag, indicators, source) = match identifier.kind {
        IdentifierKind::Isbn => (*b"020", *b"  ", None),
        IdentifierKind::Issn => (*b"022", *b"  ", None),
        IdentifierKind::Lccn => (*b"010", *b"  ", None),
        IdentifierKind::Ean => (*b"024", *b"3 ", None),
        IdentifierKind::Ismn => (*b"024", *b"2 ", None),
        IdentifierKind::Doi => (*b"024", *b"7 ", Some("doi")),
        IdentifierKind::SystemControlNumber => (*b"035", *b"  ", None),
    };
    let mut subfields = vec![(b'a', identifier.value.as_str())];
    subfields.extend(identifier.qualifier.as_deref().map(|q| (b'q', q)));
    subfields.extend(source.map(|s| (b'2', s)));
    data_field(tag, indicators, &subfields)
}

fn note_field(note: &NewNote) -> Field {
    let (tag, indicators) = match note.kind {
        NoteKind::General => (*b"500", *b"  "),
        // First indicator 0: the contents are complete.
        NoteKind::Contents => (*b"505", *b"0 "),
        NoteKind::Local => (*b"590", *b"  "),
    };
    data_field(tag, indicators, &[(b'a', note.text.as_str())])
}

/// A `952` for `item`: the mapped subfields set on `base` (or on a new field).
fn item_field(item: &ItemData, base: Option<&Field>) -> Field {
    let money = |m: Option<elzevir_domain::catalog::Money>| {
        m.map(|m| format!("{}.{:02}", m.0 / 100, m.0 % 100))
    };
    let number = |n: i64| (n != 0).then(|| n.to_string());
    let values: [(u8, Option<String>); 18] = [
        (b'0', number(item.status.withdrawn)),
        (b'1', number(item.status.lost)),
        (b'4', number(item.status.damaged)),
        (b'5', number(item.status.restricted)),
        (b'7', number(item.status.not_for_loan)),
        (b'8', item.collection.clone()),
        (b'a', Some(item.home_branch.clone())),
        (b'b', Some(item.holding_branch.clone())),
        (b'c', item.location.clone()),
        (b'd', item.acquired_on.clone()),
        (b'g', money(item.price)),
        (b'i', item.inventory_number.clone()),
        (b'o', item.call_number.clone()),
        (b'p', item.barcode.clone()),
        (b't', item.copy_number.clone()),
        (b'h', item.issue_statement.clone()),
        (b'v', money(item.replacement_price)),
        (b'y', Some(item.item_type.clone())),
    ];
    let mut record = Record {
        leader: [b' '; 24],
        fields: vec![
            base.cloned()
                .unwrap_or_else(|| data_field(*b"952", *b"  ", &[])),
        ],
    };
    for (code, value) in &values {
        let keep_zero = base.is_some() && matches!(code, b'0' | b'1' | b'4' | b'5' | b'7');
        let value = match value {
            None if keep_zero => Some("0".to_owned()),
            other => other.clone(),
        };
        set_subfield(&mut record, *b"952", *b"  ", *code, value.as_deref());
    }
    record
        .fields
        .pop()
        .unwrap_or_else(|| data_field(*b"952", *b"  ", &[]))
}
