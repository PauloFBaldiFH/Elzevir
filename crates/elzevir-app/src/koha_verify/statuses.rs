// SPDX-License-Identifier: GPL-3.0-or-later
//! The names of the status numbers and the Items' status dates
//! (docs/specs/circulacao/lost-items.md, item 18).

use std::collections::BTreeSet;

use super::{Cause, Footprint, Table, TableCheck, Unexplained, sorted};
use crate::koha_migration::{KohaPlan, numbered_label};

/// Compares the names by kind and number, written as the kind's code and the number.
pub(super) fn names(plan: &KohaPlan, found: &Footprint) -> TableCheck {
    let mut check = TableCheck::new(Table::StatusNames);
    let mut seen = BTreeSet::new();
    for name in &plan.status_names {
        let number = format!("{} {}", name.kind.code(), name.number);
        check.in_dump += 1;
        seen.insert((name.kind, name.number));
        match found.status_names.get(&(name.kind, name.number)) {
            Some(label) => {
                check.arrived += 1;
                if *label != name.label && *label != numbered_label(&name.label, name.number) {
                    check.differ(&number, Unexplained::Field("lib"));
                }
            }
            None if found
                .status_names
                .iter()
                .any(|((kind, _), label)| *kind == name.kind && *label == name.label) =>
            {
                check.explain(Cause::StatusNameTaken, &number);
            }
            None => check.differ(&number, Unexplained::Missing),
        }
    }
    for (kind, number) in found.status_names.keys() {
        if !seen.contains(&(*kind, *number)) {
            check.differ(&format!("{} {number}", kind.code()), Unexplained::Extra);
        }
    }
    sorted(check)
}

/// Compares the dates of the Items the dump gives one, by `itemnumber`.
pub(super) fn dates(plan: &KohaPlan, found: &Footprint) -> TableCheck {
    let mut check = TableCheck::new(Table::StatusDates);
    for dates in &plan.status_dates {
        let number = dates.item.itemnumber.to_string();
        check.in_dump += 1;
        match found.items.get(&number).map_or(&[][..], Vec::as_slice) {
            [] => check.explain(Cause::ItemLeftOut, &number),
            [stored] => {
                check.arrived += 1;
                let columns = [
                    ("itemlost_on", &stored.lost_on, &dates.lost_on),
                    ("damaged_on", &stored.damaged_on, &dates.damaged_on),
                    ("withdrawn_on", &stored.withdrawn_on, &dates.withdrawn_on),
                ];
                for (column, stored, dumped) in columns {
                    if stored != dumped {
                        check.differ(&number, Unexplained::Field(column));
                    }
                }
            }
            _ => {
                check.arrived += 1;
                check.differ(&number, Unexplained::Repeated);
            }
        }
    }
    sorted(check)
}
