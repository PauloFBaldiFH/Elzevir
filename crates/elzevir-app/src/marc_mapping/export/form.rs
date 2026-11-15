// SPDX-License-Identifier: GPL-3.0-or-later
//! The forms of the Work back to `380` (bibframe-completo.md, item 29, part 5f).

use elzevir_domain::statement::{Statement, Value};
use elzevir_marc::Field;

use super::super::form::{TAGS, of_field};
use super::super::titles::Owner;
use super::described::StatementFields;
use super::fields::data_field;

/// The `380` fields.
pub(super) const FORMS: StatementFields = StatementFields {
    tags: &TAGS,
    read: of_field,
    write: field_of,
};

/// The field one form of the Work is written as: a `bf:GenreForm` named only by its
/// label. The terms of a list (an IRI) go to `008` and the subjects to `655`.
fn field_of(owner: Owner, statement: &Statement) -> Option<Field> {
    match &statement.value {
        Value::Resource(form)
            if owner == Owner::Work
                && statement.property == "genreForm"
                && form.class == "GenreForm"
                && form.statements.is_empty() =>
        {
            let term = form.label.as_deref()?;
            Some(data_field(*b"380", *b"  ", &[(b'a', term)]))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use elzevir_domain::statement::Description;
    use elzevir_marc::Record;

    use super::super::super::Marc;
    use super::super::super::form::of_work;
    use super::super::described;
    use super::*;

    #[test]
    fn forms_come_back_to_their_field() {
        let source = Record {
            leader: *b"00000nam a2200000 i 4500",
            fields: vec![data_field(*b"380", *b"  ", &[(b'a', "Ficção")])],
        };
        let work = Description {
            classes: Vec::new(),
            statements: of_work(&Marc(&source)),
        };
        let instance = Description {
            classes: Vec::new(),
            statements: Vec::new(),
        };
        let mut out = Record {
            leader: source.leader,
            fields: Vec::new(),
        };
        described::write(&mut out, &FORMS, &work, &instance, None);
        assert_eq!(out, source);
    }
}
