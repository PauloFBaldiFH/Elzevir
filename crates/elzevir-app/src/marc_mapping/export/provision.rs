// SPDX-License-Identifier: GPL-3.0-or-later
//! Production, distribution, manufacture and copyright date back to `260 $e $f $g` and
//! `264` (bibframe-completo.md, item 29, part 5c).

use elzevir_domain::statement::{Description, Statement, Value, is_other_provision};
use elzevir_marc::{Field, Record};

use super::super::fixed::is_coded_year;
use super::super::provision::of_264;
use super::fields::{data_field, first_index, set_subfield};
use super::{mapped, rebuild};

/// Rewrites the activities and copyright dates when they differ from those of `before`.
pub(super) fn write(out: &mut Record, instance: &Description, before: Option<&Description>) {
    let wanted = of(instance);
    if before.is_some_and(|before| of(before) == wanted) {
        return;
    }
    let rest = wanted.as_slice();
    // A record that publishes in `260` keeps its manufacture there, as `$e $f $g`.
    if first_index(out, *b"260").is_some() {
        let in_260 = rest.iter().position(|s| class_of(s) == Some("Manufacture"));
        let parts = in_260.and_then(|at| rest.get(at)).map(parts_of);
        let present: Vec<(u8, String)> = b"efg"
            .iter()
            .copied()
            .zip(parts.unwrap_or_default())
            .filter(|(_, values)| !values.is_empty())
            .map(|(code, values)| (code, values.join(" ")))
            .collect();
        let punctuated = manufacture_punctuation(present);
        for code in *b"efg" {
            let value = punctuated
                .iter()
                .find(|(c, _)| *c == code)
                .map(|(_, v)| v.as_str());
            set_subfield(out, *b"260", *b"  ", code, value);
        }
        if !punctuated.is_empty() {
            // The period closes the area after the parenthesis, not the date before it.
            open_date(out);
        }
        let others: Vec<Statement> = rest
            .iter()
            .enumerate()
            .filter(|(at, _)| Some(*at) != in_260)
            .map(|(_, s)| s.clone())
            .collect();
        write_264(out, &others);
        return;
    }
    write_264(out, rest);
}

/// The ISBD punctuation of the manufacture in `260`: in parentheses, ` :` before the
/// manufacturer and `,` before the date (`(Curitiba : Mileart)`).
fn manufacture_punctuation(parts: Vec<(u8, String)>) -> Vec<(u8, String)> {
    let next: Vec<Option<u8>> = parts
        .iter()
        .skip(1)
        .map(|(code, _)| Some(*code))
        .chain([None])
        .collect();
    parts
        .into_iter()
        .zip(next)
        .enumerate()
        .map(|(at, ((code, value), next))| {
            let opened = if at == 0 { format!("({value}") } else { value };
            let value = match next {
                Some(b'f') => format!("{opened} :"),
                Some(_) => format!("{opened},"),
                None => format!("{opened})"),
            };
            (code, value)
        })
        .collect()
}

/// Takes the closing period off the `260 $c` that a manufacture follows.
fn open_date(out: &mut Record) {
    let Some(Field::Data { subfields, .. }) =
        first_index(out, *b"260").and_then(|index| out.fields.get_mut(index))
    else {
        return;
    };
    for subfield in subfields.iter_mut().filter(|s| s.code == b'c') {
        if subfield.value.last() == Some(&b'.') {
            subfield.value.pop();
        }
    }
}

fn write_264(out: &mut Record, wanted: &[Statement]) {
    let current = mapped(out, &[b"264"], |_, field| {
        let read = of_264(field);
        (!read.is_empty()).then_some(read)
    });
    // One `264` holds every copyright date; each activity has its own field.
    let mut groups: Vec<Vec<Statement>> = wanted
        .iter()
        .filter(|s| is_other_provision(s))
        .map(|s| vec![s.clone()])
        .collect();
    let dates: Vec<Statement> = wanted
        .iter()
        // A coded year belongs to `008` (type `t`), which holds it.
        .filter(|s| s.property == "copyrightDate" && !is_coded_copyright(s))
        .cloned()
        .collect();
    if !dates.is_empty() {
        groups.push(dates);
    }
    rebuild(out, &current, &groups, |group| field_of(group), |_, _| None);
}

/// Whether `statement` is a copyright date as `008` codes it (four digits).
fn is_coded_copyright(statement: &Statement) -> bool {
    matches!(&statement.value, Value::Literal { text, .. } if is_coded_year(text))
}

/// The statements this module writes, in order.
fn of(description: &Description) -> Vec<Statement> {
    description
        .statements
        .iter()
        .filter(|s| {
            is_other_provision(s) || (s.property == "copyrightDate" && !is_coded_copyright(s))
        })
        .cloned()
        .collect()
}

fn class_of(statement: &Statement) -> Option<&str> {
    match &statement.value {
        Value::Resource(activity) if is_other_provision(statement) => Some(&activity.class),
        _ => None,
    }
}

/// Places, agents and dates of an activity.
fn parts_of(statement: &Statement) -> [Vec<String>; 3] {
    let mut parts: [Vec<String>; 3] = Default::default();
    if let Value::Resource(activity) = &statement.value {
        for part in &activity.statements {
            let (slot, text) = match (&*part.property, &part.value) {
                ("place", Value::Resource(r)) => (0, r.label.clone()),
                ("agent", Value::Resource(r)) => (1, r.label.clone()),
                ("date", Value::Literal { text, .. }) => (2, Some(text.clone())),
                _ => continue,
            };
            if let (Some(values), Some(text)) = (parts.get_mut(slot), text) {
                values.push(text);
            }
        }
    }
    parts
}

/// The `264` of one activity, or of the copyright dates.
fn field_of(group: &[Statement]) -> Field {
    let Some(first) = group.first() else {
        return data_field(*b"264", *b" 4", &[]);
    };
    if first.property == "copyrightDate" {
        let dates: Vec<(u8, &str)> = group
            .iter()
            .filter_map(|s| match &s.value {
                Value::Literal { text, .. } => Some((b'c', text.as_str())),
                _ => None,
            })
            .collect();
        return data_field(*b"264", *b" 4", &dates);
    }
    let second = match class_of(first) {
        Some("Production") => b'0',
        Some("Distribution") => b'2',
        _ => b'3',
    };
    let parts = parts_of(first);
    let mut subfields: Vec<(u8, &str)> = Vec::new();
    for (code, values) in b"abc".iter().copied().zip(&parts) {
        subfields.extend(values.iter().map(|v| (code, v.as_str())));
    }
    data_field(*b"264", [b' ', second], &subfields)
}

#[cfg(test)]
mod tests {
    use elzevir_marc::Record;

    use super::super::super::Marc;
    use super::super::super::provision::of_instance;
    use super::*;

    fn record(fields: Vec<Field>) -> Record {
        Record {
            leader: *b"00000nam a2200000 i 4500",
            fields,
        }
    }

    #[test]
    fn activities_and_copyright_come_back_from_their_fields() {
        let source = record(vec![
            data_field(
                *b"260",
                *b"  ",
                // The manufacture as ISBD writes it, in parentheses.
                &[
                    (b'a', "Campo Erê"),
                    (b'e', "(Chapecó :"),
                    (b'f', "Gráfica)"),
                ],
            ),
            data_field(
                *b"264",
                *b" 0",
                &[(b'a', "Curitiba"), (b'b', "Estúdio"), (b'c', "2001")],
            ),
            data_field(*b"264", *b" 4", &[(b'c', "©2014")]),
        ]);
        let instance = Description {
            classes: Vec::new(),
            statements: of_instance(&Marc(&source)),
        };
        assert_eq!(instance.statements.len(), 3);
        // Written into a record that only has the publication.
        let mut out = record(vec![data_field(*b"260", *b"  ", &[(b'a', "Campo Erê")])]);
        write(&mut out, &instance, None);
        assert_eq!(out, source);
        // Without the statements, the fields go.
        write(&mut out, &Description::default(), Some(&instance));
        assert_eq!(
            out,
            record(vec![data_field(*b"260", *b"  ", &[(b'a', "Campo Erê")])])
        );
    }
}
