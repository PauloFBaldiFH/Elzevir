// SPDX-License-Identifier: GPL-3.0-or-later
//! The coded publication back to `008/06-17` (bibframe-completo.md, item 29, part 5e), after
//! the way back of the Library of Congress (`bibframe2marc`: `01-001-008.xml`).

use elzevir_domain::statement::{Description, Statement, Value, is_coded_publication};
use elzevir_marc::Record;

use super::super::fixed::{coded_of, is_coded_year};
use super::fields::set_008;

/// The statements `008/06-17` holds, of the Work and of the Instance.
fn of_group<'a>(work: &'a Description, instance: &'a Description) -> Vec<&'a Statement> {
    work.statements
        .iter()
        .filter(|s| s.property == "originDate" && year(s).is_some())
        .chain(instance.statements.iter().filter(|s| {
            is_coded_publication(s) || (s.property == "copyrightDate" && year(s).is_some())
        }))
        .collect()
}

/// Writes `008/06-17` when the coded publication, the copyright date or the original date
/// changed from `before`; the rest of `008` stays as it is.
pub(super) fn write(
    out: &mut Record,
    work: &Description,
    instance: &Description,
    before: Option<(&Description, &Description)>,
) {
    let wanted = of_group(work, instance);
    if before.is_some_and(|(work, instance)| of_group(work, instance) == wanted) {
        return;
    }
    let publication = wanted.iter().find_map(|s| match &s.value {
        Value::Resource(publication) if is_coded_publication(s) => Some(publication),
        _ => None,
    });
    let first_year = |property: &str| {
        wanted
            .iter()
            .filter(|s| s.property == property)
            .find_map(|s| year(s))
    };
    if publication.is_none()
        && first_year("copyrightDate").is_none()
        && first_year("originDate").is_none()
    {
        // Nothing to code: a record without `008` gets none for it.
        if !out.fields.iter().any(|f| f.tag().as_bytes() == b"008") {
            return;
        }
    }
    let coded = coded_of(
        publication,
        first_year("copyrightDate"),
        first_year("originDate"),
    );
    let mut positions = vec![coded.kind];
    positions.extend(coded.first);
    positions.extend(coded.second);
    positions.extend(coded.country);
    set_008(out, 6, &positions);
}

fn year(statement: &Statement) -> Option<&str> {
    match &statement.value {
        Value::Literal { text, .. } if is_coded_year(text) => Some(text),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use elzevir_marc::{Field, Tag};

    use super::super::super::Marc;
    use super::super::super::fixed::{of_instance, of_work};
    use super::*;

    #[test]
    fn the_coded_publication_comes_back_to_its_positions() {
        let fixed = "250321t19951993bl            000 0 por d";
        let source = Record {
            leader: *b"00000nam a2200000 i 4500",
            fields: vec![Field::Control {
                tag: Tag::new(*b"008"),
                value: fixed.as_bytes().to_vec(),
            }],
        };
        let marc = Marc(&source);
        let work = Description {
            classes: Vec::new(),
            statements: of_work(&marc),
        };
        let instance = Description {
            classes: Vec::new(),
            statements: of_instance(&marc),
        };
        // Written over a blank 008, the positions come back.
        let mut out = source.clone();
        if let Some(Field::Control { value, .. }) = out.fields.first_mut() {
            value.splice(6..18, *b"||||||||||||");
            value.truncate(40);
        }
        write(&mut out, &work, &instance, None);
        assert_eq!(out, source);
        // Unchanged statements leave the record alone.
        let mut kept = out.clone();
        write(
            &mut kept,
            &Description::default(),
            &Description::default(),
            Some((&Description::default(), &Description::default())),
        );
        assert_eq!(kept, out);
    }
}
