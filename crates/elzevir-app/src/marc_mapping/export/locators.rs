// SPDX-License-Identifier: GPL-3.0-or-later
//! Electronic locations of the Instance back to `856` (bibframe-completo.md, item 29, part
//! 5e), after the way back of the Library of Congress (`bibframe2marc`: `10-8XX.xml`).

use elzevir_domain::statement::{Statement, Value};
use elzevir_marc::Field;

use super::super::locators::{TAGS, of_field};
use super::super::titles::Owner;
use super::described::StatementFields;
use super::fields::data_field;

/// The `856` fields.
pub(super) const LOCATORS: StatementFields = StatementFields {
    tags: &TAGS,
    read: of_field,
    write: field_of,
};

/// The field one location of the Instance is written as; `None` for any other statement.
///
/// The first indicator says the access method (`4`, HTTP). An address of the Instance gets
/// a blank second indicator: the record does not say whether it is the resource itself or
/// another version of it.
fn field_of(owner: Owner, statement: &Statement) -> Option<Field> {
    if owner != Owner::Instance {
        return None;
    }
    match (&*statement.property, &statement.value) {
        ("electronicLocator", Value::Iri(address)) => {
            Some(data_field(*b"856", *b"4 ", &[(b'u', address)]))
        }
        ("supplementaryContent", Value::Resource(content))
            if content.class == "SupplementaryContent" =>
        {
            // A related resource without an address has no field.
            let address = content.statements.iter().find_map(|s| match &s.value {
                Value::Iri(address) if s.property == "electronicLocator" => Some(address),
                _ => None,
            })?;
            let mut subfields: Vec<(u8, &str)> = vec![(b'u', address)];
            subfields.extend(content.statements.iter().filter_map(|s| match &s.value {
                Value::Resource(note) if s.property == "note" => {
                    note.label.as_deref().map(|label| (b'z', label))
                }
                _ => None,
            }));
            Some(data_field(*b"856", *b"42", &subfields))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use elzevir_domain::statement::Description;
    use elzevir_marc::Record;

    use super::super::super::Marc;
    use super::super::super::locators::of_instance;
    use super::super::described::write;
    use super::*;

    #[test]
    fn every_location_comes_back_from_the_field_it_is_written_to() {
        let fields = vec![
            data_field(*b"856", *b"4 ", &[(b'u', "https://x.org/capa.png")]),
            data_field(
                *b"856",
                *b"42",
                &[(b'u', "http://x.org/sumario"), (b'z', "Sumário")],
            ),
        ];
        let source = Record {
            leader: *b"00000nam a2200000 i 4500",
            fields: fields.clone(),
        };
        let instance = Description {
            classes: Vec::new(),
            statements: of_instance(&Marc(&source)),
        };
        assert_eq!(instance.statements.len(), 2);
        let mut out = Record {
            leader: source.leader,
            fields: Vec::new(),
        };
        write(
            &mut out,
            &LOCATORS,
            &Description::default(),
            &instance,
            None,
        );
        assert_eq!(out.fields, fields);
    }
}
