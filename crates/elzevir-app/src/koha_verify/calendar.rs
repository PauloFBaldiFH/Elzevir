// SPDX-License-Identifier: GPL-3.0-or-later
//! Koha's calendar against each branch's calendar in the Elzevir
//! (docs/specs/migracao/verificacao-do-koha.md, item 19).

use std::collections::BTreeSet;

use crate::calendar::HolidayWhen;
use crate::koha_migration::{HolidayTable, KohaPlan, holiday_name};

use super::{Cause, Footprint, Table, TableCheck, Unexplained, sorted};

/// Whether an entry of the Elzevir is of the kind the Koha table holds.
const fn of_table(when: HolidayWhen, table: HolidayTable) -> bool {
    matches!(
        (when, table),
        (
            HolidayWhen::Weekly(_) | HolidayWhen::Annual(_),
            HolidayTable::Repeatable
        ) | (HolidayWhen::Dated(..), HolidayTable::Special)
    )
}

pub(super) fn holidays(plan: &KohaPlan, found: &Footprint, table: HolidayTable) -> TableCheck {
    let mut check = TableCheck::new(match table {
        HolidayTable::Repeatable => Table::RepeatableHolidays,
        HolidayTable::Special => Table::SpecialHolidays,
    });
    for (_, id, why) in plan.holidays_left.iter().filter(|left| left.0 == table) {
        check.in_dump += 1;
        check.explain(Cause::Holiday(*why), &id.to_string());
    }
    let mut seen = BTreeSet::new();
    for holiday in plan
        .holidays
        .iter()
        .filter(|holiday| holiday.table == table)
    {
        check.in_dump += 1;
        let number = holiday.id.to_string();
        let (Some(when), Some(text)) = (holiday.when(), holiday.text()) else {
            check.differ(&number, Unexplained::Missing);
            continue;
        };
        let name = holiday_name(&when);
        let stored = found.holidays.get(&holiday.branch).and_then(|entries| {
            entries
                .iter()
                .find(|entry| holiday_name(&entry.when) == name)
        });
        seen.insert((holiday.branch.clone(), name));
        let Some(stored) = stored else {
            if found.branches.contains_key(&holiday.branch) {
                check.differ(&number, Unexplained::Missing);
            } else {
                check.explain(Cause::UnknownBranch(holiday.branch.clone()), &number);
            }
            continue;
        };
        check.arrived += 1;
        if stored.when != when {
            check.differ(&number, Unexplained::Field("isexception"));
        }
        if stored.text.title != text.title {
            check.differ(&number, Unexplained::Field("title"));
        }
        if stored.text.description != text.description {
            check.differ(&number, Unexplained::Field("description"));
        }
    }
    for (branch, entries) in &found.holidays {
        for entry in entries.iter().filter(|entry| of_table(entry.when, table)) {
            let name = holiday_name(&entry.when);
            if !seen.contains(&(branch.clone(), name.clone())) {
                check.differ(&format!("{branch} {name}"), Unexplained::ExtraBy("entry"));
            }
        }
    }
    sorted(check)
}
