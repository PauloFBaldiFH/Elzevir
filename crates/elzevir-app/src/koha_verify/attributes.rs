// SPDX-License-Identifier: GPL-3.0-or-later
//! The patron attributes: each type of the dump is a field, holding as many values as the
//! migration's rules let come (docs/specs/circulacao/patron-attributes.md, item 19).

use std::collections::{BTreeMap, BTreeSet};

use super::{Cause, Footprint, Table, TableCheck, Unexplained, extras, sorted};
use crate::koha_migration::{KohaPlan, ValueLeftover};
use crate::patron_fields::{FieldKind, MAX_TEXT_VALUE, MAX_VALUES_PER_FIELD};

/// Compares the types by `code`, and the number of values of each.
pub(super) fn attributes(plan: &KohaPlan, found: &Footprint) -> TableCheck {
    let mut check = TableCheck::new(Table::AttributeTypes);
    let mut seen = BTreeSet::new();
    let patrons: BTreeSet<u64> = plan
        .patrons
        .iter()
        .map(|patron| patron.borrowernumber)
        .collect();
    for field_type in &plan.attribute_types {
        let code = field_type.code.as_str();
        check.in_dump += 1;
        seen.insert(code);
        let Some(stored) = found.attributes.get(code) else {
            check.differ(code, Unexplained::Missing);
            continue;
        };
        check.arrived += 1;
        let mut held: BTreeMap<u64, usize> = BTreeMap::new();
        let mut taken: BTreeSet<&str> = BTreeSet::new();
        let mut given: BTreeSet<(u64, &str)> = BTreeSet::new();
        let mut expected = 0;
        for value in plan
            .attribute_values
            .iter()
            .filter(|value| value.code == code)
        {
            // The same value twice for a patron comes once, as the writer gives it.
            if given.contains(&(value.borrowernumber, value.value.as_str())) {
                continue;
            }
            let option = field_type
                .option_codes
                .iter()
                .find(|(option, _)| *option == value.value)
                .map(|(_, index)| index);
            let why = if !patrons.contains(&value.borrowernumber) {
                Some(ValueLeftover::Patron)
            } else if match field_type.spec.kind {
                FieldKind::List(_) => option.is_none(),
                FieldKind::Date => value
                    .value
                    .get(..10)
                    .and_then(|day| day.parse::<jiff::civil::Date>().ok())
                    .is_none(),
                FieldKind::Text => value.value.chars().count() > MAX_TEXT_VALUE,
            } {
                Some(ValueLeftover::Invalid)
            } else if held.get(&value.borrowernumber).copied().unwrap_or(0)
                >= if field_type.spec.repeatable {
                    MAX_VALUES_PER_FIELD
                } else {
                    1
                }
            {
                Some(ValueLeftover::Repeated)
            } else if field_type.spec.unique && !taken.insert(value.value.as_str()) {
                Some(ValueLeftover::Taken)
            } else {
                None
            };
            if let Some(why) = why {
                check.explain(Cause::Attribute(why), code);
            } else {
                given.insert((value.borrowernumber, value.value.as_str()));
                *held.entry(value.borrowernumber).or_default() += 1;
                expected += 1;
            }
        }
        if *stored != expected {
            check.differ(code, Unexplained::Field("values"));
        }
    }
    extras(&mut check, found.attributes.iter(), &seen);
    sorted(check)
}
