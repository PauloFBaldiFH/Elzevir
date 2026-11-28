// SPDX-License-Identifier: GPL-3.0-or-later
//! `patrons.migrate_koha_statuses`: the names of Koha's status numbers become status
//! names, and the Items get the moment they took their statuses
//! (docs/specs/circulacao/lost-items.md, item 17).

use elzevir_app::commands::WriteRefusal;
use elzevir_app::koha_migration::{
    KohaStatusDates, KohaStatusName, KohaStatusesBatch, KohaStatusesMigrated, numbered_label,
};
use elzevir_app::patrons::PatronOutput;
use rusqlite::{Connection, OptionalExtension, params};

use crate::{CommandFailed, StoreError};

/// Applies one write: the names, then the dates.
pub(crate) fn apply(
    connection: &Connection,
    batch: &KohaStatusesBatch,
) -> Result<PatronOutput, CommandFailed<WriteRefusal>> {
    let mut done = KohaStatusesMigrated::default();
    for name in &batch.names {
        save_name(connection, name, &mut done)?;
    }
    for dates in &batch.dates {
        match crate::koha_circulation::item(connection, &dates.item)? {
            Ok(item) => {
                save_dates(connection, item, dates)?;
                done.dates += 1;
            }
            Err(_) => done.items_left += 1,
        }
    }
    Ok(PatronOutput::KohaStatusesMigrated(done))
}

/// Names the number, unless it is named already; a name another number of the kind has
/// comes with the number.
fn save_name(
    connection: &Connection,
    name: &KohaStatusName,
    done: &mut KohaStatusesMigrated,
) -> Result<(), StoreError> {
    let kind = name.kind.code();
    let named = connection
        .query_row(
            "SELECT 1 FROM item_status_names WHERE kind = ?1 AND number = ?2",
            params![kind, name.number],
            |_| Ok(()),
        )
        .optional()?;
    if named.is_some() {
        done.names_existing += 1;
        return Ok(());
    }
    let free = |label: &str| -> Result<bool, StoreError> {
        Ok(connection
            .query_row(
                "SELECT 1 FROM item_status_names WHERE kind = ?1 AND label = ?2",
                params![kind, label],
                |_| Ok(()),
            )
            .optional()?
            .is_none())
    };
    let label = if free(&name.label)? {
        name.label.clone()
    } else {
        let numbered = numbered_label(&name.label, name.number);
        if !free(&numbered)? {
            done.names_taken += 1;
            return Ok(());
        }
        done.names_numbered += 1;
        numbered
    };
    connection.execute(
        "INSERT INTO item_status_names (kind, number, label) VALUES (?1, ?2, ?3)",
        params![kind, name.number, label],
    )?;
    done.names += 1;
    Ok(())
}

/// Gives the Item each date of a status it has; a status it does not have keeps no date,
/// as in Koha, which clears the date with the status.
fn save_dates(
    connection: &Connection,
    item: i64,
    dates: &KohaStatusDates,
) -> Result<(), StoreError> {
    connection.execute(
        "UPDATE items
         SET lost_on = CASE WHEN lost <> 0 AND ?2 IS NOT NULL THEN ?2 ELSE lost_on END,
             damaged_on = CASE WHEN damaged <> 0 AND ?3 IS NOT NULL THEN ?3 ELSE damaged_on END,
             withdrawn_on = CASE WHEN withdrawn <> 0 AND ?4 IS NOT NULL THEN ?4
                                 ELSE withdrawn_on END
         WHERE id = ?1",
        params![item, dates.lost_on, dates.damaged_on, dates.withdrawn_on],
    )?;
    Ok(())
}
