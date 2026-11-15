// SPDX-License-Identifier: GPL-3.0-or-later
//! `bibframe_to_marc`: the model back to MARC21 (docs/specs/catalogo/mapeamento-marc21.md, §7).
//!
//! The original record is the starting point. Only the parts of the model that differ from
//! what the original maps to are rewritten, and a rewritten list reuses every original field
//! whose mapping did not change. An unedited record therefore comes back byte for byte.

use elzevir_domain::catalog::{
    AgentIdentifierKind, AgentKind, IdentifierKind, MaterialType, NoteKind, SubjectKind,
};
use elzevir_marc::{Field, Record, Subfield, Tag};
use std::borrow::Cow;

use super::instance::{IDENTIFIER_TAGS, NOTE_TAGS, identifier_of, note_of};
use super::items::{defaults, item_of};
use super::work::{CONTRIBUTION_TAGS, SUBJECT_TAGS, contribution_of, name_part_code, subject_of};
use super::{DataField, Marc, marc_to_bibframe, responsibility};
use crate::catalog::{ItemData, NewContribution, NewIdentifier, NewNote, NewRecord, NewSubject};

mod admin;
mod coded;
mod content;
mod described;
mod fields;
mod fixed;
mod locators;
mod notes;
mod physical;
mod provision;
mod relations;
mod titles;

use fields::{data_field, first_index, insert_in_tag_order, set_subfield};

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
    if changed(&|b| b.material_type == model.material_type) {
        set_material_type(&mut out, model.material_type);
    }
    if changed(&|b| b.instance.material_type == model.instance.material_type) {
        set_instance_material_type(&mut out, model.instance.material_type);
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
    provision::write(
        &mut out,
        &model.instance.description,
        before.as_ref().map(|b| &b.instance.description),
    );
    write_titles(&mut out, model, before.as_ref());
    write_publication(&mut out, model, before.as_ref());
    write_lists(&mut out, model, before.as_ref());
    content::write(&mut out, &model.work_description);
    physical::write(&mut out, &model.instance.description);
    admin::write(&mut out, &model.instance.description);
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

fn set_material_type(out: &mut Record, material_type: MaterialType) {
    let (record_type, level) = match material_type {
        MaterialType::Book => (b'a', b'm'),
        MaterialType::Periodical => (b'a', b's'),
        MaterialType::Map => (b'e', b'm'),
        MaterialType::Score => (b'c', b'm'),
        MaterialType::SoundRecording => (b'j', b'm'),
        MaterialType::Video => (b'g', b'm'),
        MaterialType::Digital => (b'm', b'm'),
        MaterialType::VisualMaterial => (b'k', b'm'),
        MaterialType::Mixed => (b'p', b'c'),
    };
    if let Some([six, seven]) = out
        .leader
        .get_mut(6..8)
        .and_then(|s| <&mut [u8; 2]>::try_from(s).ok())
    {
        *six = record_type;
        *seven = level;
    }
}

/// An electronic Instance gets a `007` for a remote electronic resource when it has none.
fn set_instance_material_type(out: &mut Record, material_type: Option<MaterialType>) {
    let has_007 = out.fields.iter().any(|f| f.tag().as_bytes() == b"007");
    if material_type == Some(MaterialType::Digital) && !has_007 {
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
        write_245(out, model, statement);
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
    if before.is_none_or(|b| {
        (&b.classification, &b.classification_scheme)
            != (&model.classification, &model.classification_scheme)
    }) {
        let tag = match model.classification_scheme.as_deref() {
            Some("udc") => *b"080",
            Some("lcc") => *b"050",
            _ => *b"082",
        };
        set_subfield(out, tag, *b"04", b'a', model.classification.as_deref());
    }
}

/// The `245` with the ISBD punctuation of the LC way back: ` :` before the subtitle, ` /`
/// before the statement of responsibility and a period at the end of `$b` or `$c`. The
/// first indicator says whether the title is an added entry: it is when the Work has a main
/// entry or a preferred title of its own; the second (characters to ignore) has no source
/// in the model and is `0`.
fn write_245(out: &mut Record, model: &NewRecord, statement: Option<&str>) {
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
    set_subfield(out, *b"245", indicators, b'a', Some(&main));
    set_subfield(out, *b"245", indicators, b'b', rest.as_deref());
    set_subfield(
        out,
        *b"245",
        indicators,
        b'c',
        statement.map(closed).as_deref(),
    );
    set_subfield(out, *b"245", indicators, b'n', None);
    set_subfield(out, *b"245", indicators, b'p', None);
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
            insert_in_tag_order(out, vec![data_field(*b"264", *b" 1", &subfields)]);
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
            &model.items,
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
    if languages.is_empty() {
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
    let mut name: Vec<(u8, Cow<'_, str>)> = name_subfields(contribution)
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
    subfields.extend(contribution.identifiers.iter().map(|identifier| {
        let code = match identifier.kind {
            AgentIdentifierKind::AuthorityRecord => b'0',
            AgentIdentifierKind::RealWorldObject => b'1',
        };
        (code, identifier.value.as_str())
    }));
    subfields.extend(code.map(|code| (b'4', code)));
    data_field(tag, [first_indicator, b' '], &subfields)
}

/// The heading as `$a` and a subfield for each name part, cut where the part starts; a
/// heading whose parts do not fit it goes out whole in `$a`.
fn name_subfields(contribution: &NewContribution) -> Vec<(u8, &str)> {
    let heading = contribution.heading.as_str();
    let mut cuts = Vec::new();
    let mut previous = 0;
    for part in &contribution.name_parts {
        // Characters to bytes; a part outside the heading or out of order is no cut.
        let Some((at, _)) = heading.char_indices().nth(part.start) else {
            return vec![(b'a', heading)];
        };
        if at <= previous {
            return vec![(b'a', heading)];
        }
        cuts.push((at, name_part_code(contribution.kind, part.kind)));
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

fn subject_field(subject: &NewSubject) -> Field {
    let (tag, first_indicator) = match subject.kind {
        SubjectKind::Agent => (*b"600", b'1'),
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
    let mut subfields: Vec<(u8, &str)> = subject_parts(subject)
        .into_iter()
        .map(|(kind, part)| (kind.map_or(b'a', subdivision_code), part))
        .collect();
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
        SubjectKind::Topic | SubjectKind::Agent | SubjectKind::Title => b'x',
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
    let values: [(u8, Option<String>); 17] = [
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
