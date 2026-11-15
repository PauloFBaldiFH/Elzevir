// SPDX-License-Identifier: GPL-3.0-or-later
//! Fields that hold statements of the Work and of the Instance, such as other titles and
//! typed notes (bibframe-completo.md, item 29).

use elzevir_domain::statement::{Description, Statement};
use elzevir_marc::{Field, Record};

use super::super::DataField;
use super::super::titles::Owner;
use super::fields::data_field;
use super::{mapped, rebuild};

/// How a group of fields maps to statements and back.
pub(super) struct StatementFields {
    /// The tags of the group, in tag order.
    pub(super) tags: &'static [&'static [u8; 3]],
    /// The statement one field gives, with its entity.
    pub(super) read: fn([u8; 3], DataField<'_>) -> Option<(Owner, Statement)>,
    /// The field a statement is written as; `None` for a statement of another group.
    pub(super) write: fn(Owner, &Statement) -> Option<Field>,
}

/// Rewrites the group's fields when its statements differ from those of `before`. A
/// statement that did not change keeps its original field.
pub(super) fn write(
    out: &mut Record,
    group: &StatementFields,
    work: &Description,
    instance: &Description,
    before: Option<(&Description, &Description)>,
) {
    let wanted = of_group(group, work, instance);
    if before.is_some_and(|(work, instance)| of_group(group, work, instance) == wanted) {
        return;
    }
    for tag in group.tags {
        let current = mapped(out, &[tag], group.read);
        let of_tag: Vec<(Owner, Statement)> = wanted
            .iter()
            .filter(|(owner, statement)| {
                (group.write)(*owner, statement).is_some_and(|f| f.tag().as_bytes() == *tag)
            })
            .cloned()
            .collect();
        rebuild(
            out,
            &current,
            &of_tag,
            |(owner, statement)| {
                (group.write)(*owner, statement).unwrap_or_else(|| data_field(**tag, *b"  ", &[]))
            },
            |_, _| None,
        );
    }
}

fn of_group(
    group: &StatementFields,
    work: &Description,
    instance: &Description,
) -> Vec<(Owner, Statement)> {
    [(Owner::Work, work), (Owner::Instance, instance)]
        .into_iter()
        .flat_map(|(owner, description)| {
            description
                .statements
                .iter()
                .filter(move |statement| (group.write)(owner, statement).is_some())
                .map(move |statement| (owner, statement.clone()))
        })
        .collect()
}
