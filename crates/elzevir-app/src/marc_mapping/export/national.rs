// SPDX-License-Identifier: GPL-3.0-or-later
//! The numbers of national bibliographic agencies back to `016` (bibframe-completo.md,
//! item 29, part 5f), after the way back of the Library of Congress (`bibframe2marc`:
//! `02-010-048.xml`): first indicator `7` and the agency in `$2`, or a blank first
//! indicator and no `$2` for Library and Archives Canada.

use elzevir_domain::statement::{Statement, Value};
use elzevir_marc::Field;

use super::super::national::{CANADA, TAGS, of_field};
use super::super::titles::Owner;
use super::described::StatementFields;
use super::fields::data_field;

/// The `016` fields.
pub(super) const NATIONAL: StatementFields = StatementFields {
    tags: &TAGS,
    read: of_field,
    write: field_of,
};

/// The field one number of the Instance is written as; `None` for any other statement.
fn field_of(owner: Owner, statement: &Statement) -> Option<Field> {
    let Value::Resource(local) = &statement.value else {
        return None;
    };
    if owner != Owner::Instance || statement.property != "identifiedBy" || local.class != "Local" {
        return None;
    }
    let number = local.label.as_deref()?;
    let source = local.statements.iter().find_map(|s| match &s.value {
        Value::Resource(source) if s.property == "source" => Some(source),
        _ => None,
    })?;
    let code = source.statements.iter().find_map(|s| match &s.value {
        Value::Literal { text, .. } if s.property == "code" => Some(text.as_str()),
        _ => None,
    });
    Some(match code {
        Some(code) => data_field(*b"016", *b"7 ", &[(b'a', number), (b'2', code)]),
        None if source.label.as_deref() == Some(CANADA) => {
            data_field(*b"016", *b"  ", &[(b'a', number)])
        }
        None => return None,
    })
}

#[cfg(test)]
mod tests {
    use elzevir_domain::statement::Description;
    use elzevir_marc::Record;

    use super::super::super::Marc;
    use super::super::super::national::of_instance;
    use super::super::described;
    use super::*;

    #[test]
    fn numbers_come_back_with_their_agency() {
        let source = Record {
            leader: *b"00000nam a2200000 i 4500",
            fields: vec![
                data_field(*b"016", *b"7 ", &[(b'a', "0000391964"), (b'2', "BR-RjBN")]),
                data_field(*b"016", *b"  ", &[(b'a', "20109876")]),
                data_field(*b"016", *b"7 ", &[(b'a', "123")]),
            ],
        };
        let instance = Description {
            classes: Vec::new(),
            statements: of_instance(&Marc(&source)),
        };
        // A number without an agency has no statement.
        assert_eq!(instance.statements.len(), 2);
        let work = Description {
            classes: Vec::new(),
            statements: Vec::new(),
        };
        let mut out = Record {
            leader: source.leader,
            fields: Vec::new(),
        };
        described::write(&mut out, &NATIONAL, &work, &instance, None);
        assert_eq!(out.fields, source.fields[..2]);
    }
}
