// SPDX-License-Identifier: GPL-3.0-or-later
//! `patrons.migrate_koha_calendar`: each row of Koha's calendar set in the same branch's
//! calendar as `circ.change_calendar` sets it (docs/specs/migracao/troca-do-koha.md,
//! item 7b).

use std::collections::BTreeMap;

use elzevir_app::calendar::CalendarChange;
use elzevir_app::commands::WriteRefusal;
use elzevir_app::koha_migration::{HolidayLeftover, KohaCalendarBatch, KohaCalendarMigrated};
use elzevir_app::patrons::PatronOutput;
use elzevir_domain::catalog::BranchId;
use rusqlite::Connection;

use crate::CommandFailed;
use crate::places::{Coded, by_code};

/// Sets every entry whose branch the Elzevir has; an entry it cannot take is left out alone.
pub(crate) fn apply(
    connection: &Connection,
    batch: &KohaCalendarBatch,
) -> Result<PatronOutput, CommandFailed<WriteRefusal>> {
    let mut done = KohaCalendarMigrated::default();
    let mut branches: BTreeMap<&str, Option<i64>> = BTreeMap::new();
    for holiday in &batch.holidays {
        let branch = if let Some(known) = branches.get(holiday.branch.as_str()) {
            *known
        } else {
            let found = by_code(connection, Coded::Branch, &holiday.branch)?;
            branches.insert(&holiday.branch, found);
            found
        };
        let Some(branch) = branch else {
            done.left
                .push((holiday.table, holiday.id, HolidayLeftover::UnknownBranch));
            continue;
        };
        let change = CalendarChange {
            branch: BranchId(branch),
            action: holiday.action.clone(),
        };
        match crate::calendar::change(connection, &change) {
            Ok(_) => done.holidays += 1,
            // The plan already checked the entry; a refusal here is the writer's own.
            Err(CommandFailed::Rejected(WriteRefusal::Circulation(_))) => {
                done.left
                    .push((holiday.table, holiday.id, HolidayLeftover::BadDate));
            }
            Err(other) => return Err(other),
        }
    }
    Ok(PatronOutput::KohaCalendarMigrated(done))
}
