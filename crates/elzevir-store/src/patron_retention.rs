// SPDX-License-Identifier: GPL-3.0-or-later
//! `patrons.apply_retention` (docs/specs/circulacao/lgpd.md, items 12 to 16): the history
//! older than the library keeps it loses its patron, and patrons expired long ago are
//! anonymized.

use elzevir_app::commands::WriteRefusal;
use elzevir_app::patrons::{PatronOutput, Retention, RetentionApplied};
use elzevir_domain::patron::PatronId;
use jiff::Span;
use jiff::civil::Date;
use rusqlite::Connection;

use crate::{CommandFailed, StoreError};

/// The name of the run in `daily_runs`.
const RUN: &str = "patrons.retention";

/// Expired patrons, the day given or before, not yet anonymized; whether each can be (no
/// loan, no hold and no debt).
const EXPIRED: &str = "SELECT p.id,
            NOT EXISTS (SELECT 1 FROM loans WHERE patron_id = p.id)
        AND NOT EXISTS (SELECT 1 FROM holds WHERE patron_id = p.id)
        AND coalesce((SELECT sum(outstanding) FROM account_lines WHERE patron_id = p.id), 0) <= 0
     FROM patrons p
     WHERE p.anonymized_at IS NULL AND p.expires <= ?1
     ORDER BY p.id";

/// The day `days` before `today`; `None` when the period reaches before the calendar.
fn before(today: Date, days: u32) -> Option<String> {
    let span = Span::new().try_days(i64::from(days)).ok()?;
    today.checked_sub(span).ok().map(|day| day.to_string())
}

/// Item 13 to 15, once a day: the same day again does nothing.
pub(crate) fn apply(
    connection: &Connection,
    today: Date,
    retention: Retention,
) -> Result<PatronOutput, CommandFailed<WriteRefusal>> {
    let mut applied = RetentionApplied::default();
    if !crate::notices::first_run_today(connection, RUN, today).map_err(StoreError::from)? {
        return Ok(PatronOutput::RetentionApplied(applied));
    }
    applied.ran = true;
    if let Some(cutoff) = retention
        .reading_history_days
        .and_then(|days| before(today, days))
    {
        // Item 13: the stored moments start with the library's day, so the first ten
        // characters compare as dates.
        for sql in [
            "UPDATE returned_loans SET patron_id = NULL
             WHERE patron_id IS NOT NULL AND substr(returned_at, 1, 10) < ?1",
            "UPDATE ended_holds SET patron_id = NULL
             WHERE patron_id IS NOT NULL AND substr(ended_at, 1, 10) < ?1",
        ] {
            applied.history += connection
                .execute(sql, [&cutoff])
                .map_err(StoreError::from)?;
        }
    }
    if let Some(cutoff) = retention
        .anonymize_expired_days
        .and_then(|days| before(today, days))
    {
        let expired = {
            let mut statement = connection
                .prepare_cached(EXPIRED)
                .map_err(StoreError::from)?;
            statement
                .query_map([&cutoff], |row| Ok((PatronId(row.get(0)?), row.get(1)?)))
                .map_err(StoreError::from)?
                .collect::<rusqlite::Result<Vec<(PatronId, bool)>>>()
                .map_err(StoreError::from)?
        };
        for (patron, free) in expired {
            if free {
                crate::patron_privacy::anonymize(connection, patron)?;
                applied.anonymized += 1;
            } else {
                applied.kept += 1;
            }
        }
    }
    Ok(PatronOutput::RetentionApplied(applied))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_period_counts_back_from_the_day() {
        let today = jiff::civil::date(2026, 3, 1);
        assert_eq!(before(today, 1).as_deref(), Some("2026-02-28"));
        assert_eq!(before(today, 365).as_deref(), Some("2025-03-01"));
        assert_eq!(before(today, u32::MAX), None);
    }
}
