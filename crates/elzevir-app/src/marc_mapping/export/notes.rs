// SPDX-License-Identifier: GPL-3.0-or-later
//! Typed notes, custodial history and frequency back to `310` and the `5XX` fields
//! (bibframe-completo.md, item 29, part 5b).

use elzevir_domain::statement::{Statement, Value, is_typed_note};
use elzevir_marc::Field;

use super::super::notes::{TAGS, note_code, of_field};
use super::super::titles::Owner;
use super::described::StatementFields;
use super::fields::data_field;

/// The fields of typed notes, custodial history and frequency.
pub(super) const NOTES: StatementFields = StatementFields {
    tags: &TAGS,
    read: of_field,
    write: field_of,
};

/// The field one statement is written as; `None` for any other statement.
fn field_of(owner: Owner, statement: &Statement) -> Option<Field> {
    match (owner, &*statement.property, &statement.value) {
        (Owner::Instance, "frequency", Value::Resource(frequency)) => {
            let mut subfields = vec![(b'a', frequency.label.as_deref()?)];
            subfields.extend(frequency.statements.iter().find_map(|s| match &s.value {
                Value::Literal { text, .. } if s.property == "date" => Some((b'b', text.as_str())),
                _ => None,
            }));
            Some(data_field(*b"310", *b"  ", &subfields))
        }
        (Owner::Instance, "custodialHistory", Value::Literal { text, .. }) => {
            Some(data_field(*b"561", *b"  ", &[(b'a', text)]))
        }
        (_, "note", Value::Resource(note)) if is_typed_note(statement) => {
            let label = note.statements.iter().find_map(|s| match &s.value {
                Value::Literal { text, .. } if s.property == "noteType" => Some(text.as_str()),
                _ => None,
            })?;
            let (tag, first) = field_for(note_code(label)?)?;
            let on_work = tag == *b"546";
            if on_work != (owner == Owner::Work) {
                return None;
            }
            Some(data_field(
                tag,
                [first, b' '],
                &[(b'a', note.label.as_deref()?)],
            ))
        }
        _ => None,
    }
}

/// The tag and first indicator of a note of `mnotetype` code `code`.
fn field_for(code: &str) -> Option<([u8; 3], u8)> {
    Some(match code {
        "with" => (*b"501", b' '),
        "biblio" => (*b"504", b' '),
        "report" => (*b"513", b' '),
        "issuance" => (*b"515", b' '),
        "computer" => (*b"516", b' '),
        "addphys" => (*b"530", b' '),
        "repro" => (*b"533", b' '),
        "orig" => (*b"534", b' '),
        "fundinfo" => (*b"536", b' '),
        "related" => (*b"544", b' '),
        "biogdata" => (*b"545", b'0'),
        "adminhist" => (*b"545", b'1'),
        "lang" => (*b"546", b' '),
        "issuing" => (*b"550", b' '),
        "finding" => (*b"555", b'0'),
        "index" => (*b"555", b' '),
        "doc" => (*b"556", b' '),
        "exhibit" => (*b"585", b' '),
        "descsource" => (*b"588", b' '),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use elzevir_domain::statement::Description;
    use elzevir_marc::Record;

    use super::super::super::Marc;
    use super::super::super::notes::{note, of_owner};
    use super::super::described::write;
    use super::*;

    #[test]
    fn every_typed_note_comes_back_from_the_field_it_is_written_to() {
        let codes = [
            "with",
            "biblio",
            "report",
            "issuance",
            "computer",
            "addphys",
            "repro",
            "orig",
            "fundinfo",
            "related",
            "biogdata",
            "adminhist",
            "issuing",
            "finding",
            "index",
            "doc",
            "exhibit",
            "descsource",
        ];
        let instance = Description {
            classes: Vec::new(),
            statements: codes
                .iter()
                .map(|code| note(code, format!("Nota {code}.")))
                .collect(),
        };
        let work = Description {
            classes: Vec::new(),
            statements: vec![note("lang", "Texto em português.".to_owned())],
        };
        let mut out = Record {
            leader: *b"00000nam a2200000 i 4500",
            fields: Vec::new(),
        };
        write(&mut out, &NOTES, &work, &instance, None);
        assert_eq!(out.fields.len(), codes.len() + 1);
        let marc = Marc(&out);
        let mut back = of_owner(&marc, Owner::Instance);
        back.sort_by_key(|statement| format!("{statement:?}"));
        let mut expected = instance.statements.clone();
        expected.sort_by_key(|statement| format!("{statement:?}"));
        assert_eq!(back, expected);
        assert_eq!(of_owner(&marc, Owner::Work), work.statements);
    }
}
