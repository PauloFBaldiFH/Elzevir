// SPDX-License-Identifier: GPL-3.0-or-later
//! The desk's pending work counted on SQLite (`docs/UI_BIBFRAME_INTERACTION_SPEC.md`,
//! §22 item 14).

use elzevir_app::pending::{PendingCounts, PendingError, PendingReader};
use jiff::{Timestamp, Zoned};
use rusqlite::Connection;

use crate::ElzevirStore;

fn count(connection: &Connection, sql: &str) -> rusqlite::Result<u64> {
    let counted: i64 = connection.query_row(sql, [], |row| row.get(0))?;
    Ok(u64::try_from(counted).unwrap_or(0))
}

/// Loans past due at `now`. The due time is stored with its zone, which text order does
/// not follow, so each one is read and compared as a time.
fn overdue(connection: &Connection, now: Timestamp) -> rusqlite::Result<u64> {
    let mut statement = connection.prepare_cached(
        "SELECT l.due FROM loans l JOIN items i ON i.id = l.item_id WHERE i.lost = 0",
    )?;
    let mut late = 0;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let due: String = row.get(0)?;
        let due: Zoned = due.parse().map_err(|error: jiff::Error| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, error.into())
        })?;
        if due.timestamp() < now {
            late += 1;
        }
    }
    Ok(late)
}

impl PendingReader for ElzevirStore {
    fn pending(&self, now: Timestamp) -> Result<PendingCounts, PendingError> {
        self.readers()
            .read(|c| {
                Ok(PendingCounts {
                    overdue: overdue(c, now)?,
                    waiting_holds: count(c, "SELECT count(*) FROM holds WHERE found = 'W'")?,
                    notices_to_print: count(
                        c,
                        "SELECT count(*) FROM notices
                         WHERE status = 'pending' AND transport = 'print'",
                    )?,
                })
            })
            .map_err(|error: rusqlite::Error| PendingError(error.to_string()))
    }
}
