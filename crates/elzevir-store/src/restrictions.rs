// SPDX-License-Identifier: GPL-3.0-or-later
//! `patron_restrictions`: suspensions of a patron and restrictions for overdue notices
//! (docs/specs/circulacao/suspensao.md, docs/specs/circulacao/avisos.md).

use elzevir_domain::patron::PatronId;
use elzevir_domain::suspension::{RestrictionKind, SuspensionEnd, restricted_until};
use jiff::civil::Date;
use rusqlite::{Connection, OptionalExtension, params};

/// A patron's suspension: how long it lasts, `None` until it is lifted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Suspension {
    id: i64,
    pub(crate) expires: Option<Date>,
}

impl Suspension {
    /// The suspension still holds on `day` (Koha: `Koha::Patron::is_debarred`).
    pub(crate) fn holds_on(self, day: Date) -> bool {
        self.expires.is_none_or(|expires| expires > day)
    }
}

/// The patron's suspension, whether it still holds or not.
pub(crate) fn suspension(
    connection: &Connection,
    patron: PatronId,
) -> rusqlite::Result<Option<Suspension>> {
    connection
        .query_row(
            "SELECT id, expires FROM patron_restrictions
             WHERE patron_id = ?1 AND kind = 'SUSPENSION'",
            [patron.0],
            |row| {
                let expires: Option<String> = row.get(1)?;
                let expires = expires
                    .map(|expires| expires.parse::<Date>())
                    .transpose()
                    .map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            1,
                            rusqlite::types::Type::Text,
                            error.into(),
                        )
                    })?;
                Ok(Suspension {
                    id: row.get(0)?,
                    expires,
                })
            },
        )
        .optional()
}

/// Suspends the patron until `until`, or keeps the current suspension when it ends later,
/// and returns when the suspension ends now.
///
/// A suspension without an end is replaced, as Koha does (it only compares two dates).
///
/// Koha: `Koha::Patron::Debarments::AddUniqueDebarment`.
pub(crate) fn suspend(
    connection: &Connection,
    patron: PatronId,
    until: Date,
    created_at: &str,
) -> rusqlite::Result<Date> {
    if let Some(current) = suspension(connection, patron)? {
        let until = current.expires.map_or(until, |expires| expires.max(until));
        connection.execute(
            "UPDATE patron_restrictions SET expires = ?2 WHERE id = ?1",
            params![current.id, until.to_string()],
        )?;
        return Ok(until);
    }
    connection.execute(
        "INSERT INTO patron_restrictions (patron_id, kind, expires, created_at)
         VALUES (?1, 'SUSPENSION', ?2, ?3)",
        params![patron.0, until.to_string(), created_at],
    )?;
    Ok(until)
}

/// The patron's restrictions that still hold on `day`, with their kinds and ends.
pub(crate) fn restrictions(
    connection: &Connection,
    patron: PatronId,
    day: Date,
) -> rusqlite::Result<Vec<(RestrictionKind, SuspensionEnd)>> {
    let mut statement = connection.prepare_cached(
        "SELECT kind, expires FROM patron_restrictions
         WHERE patron_id = ?1 AND (expires IS NULL OR expires > ?2)
         ORDER BY kind",
    )?;
    statement
        .query_map(params![patron.0, day.to_string()], |row| {
            let kind: String = row.get(0)?;
            let kind = RestrictionKind::from_name(&kind).ok_or_else(|| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    "unknown restriction".into(),
                )
            })?;
            let expires: Option<String> = row.get(1)?;
            let end = match expires {
                Some(expires) => {
                    SuspensionEnd::Day(expires.parse().map_err(|error: jiff::Error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            1,
                            rusqlite::types::Type::Text,
                            error.into(),
                        )
                    })?)
                }
                None => SuspensionEnd::WhenLifted,
            };
            Ok((kind, end))
        })?
        .collect()
}

/// Until when the patron may not borrow on `day`, all restrictions together; `None` when
/// none holds (Koha: `Koha::Patron::is_debarred`).
pub(crate) fn restricted(
    connection: &Connection,
    patron: PatronId,
    day: Date,
) -> rusqlite::Result<Option<SuspensionEnd>> {
    Ok(restricted_until(
        restrictions(connection, patron, day)?
            .into_iter()
            .map(|(_, end)| end),
    ))
}

/// Restricts the patron for overdue notices until the staff lift it; a patron already
/// restricted for them stays as is.
///
/// Koha: `AddUniqueDebarment` with type `OVERDUES` and no expiration, from
/// `overdue_notices.pl`.
pub(crate) fn restrict_for_overdues(
    connection: &Connection,
    patron: PatronId,
    created_at: &str,
) -> rusqlite::Result<()> {
    connection.execute(
        "INSERT OR IGNORE INTO patron_restrictions (patron_id, kind, expires, created_at)
         VALUES (?1, 'OVERDUES', NULL, ?2)",
        params![patron.0, created_at],
    )?;
    Ok(())
}

/// Lifts the patron's restriction of this kind; whether there was one.
///
/// Koha: `Koha::Patron::Debarments::DelUniqueDebarment`.
pub(crate) fn lift(
    connection: &Connection,
    patron: PatronId,
    kind: RestrictionKind,
) -> rusqlite::Result<bool> {
    Ok(connection.execute(
        "DELETE FROM patron_restrictions WHERE patron_id = ?1 AND kind = ?2",
        params![patron.0, kind.as_str()],
    )? > 0)
}
