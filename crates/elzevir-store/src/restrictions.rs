// SPDX-License-Identifier: GPL-3.0-or-later
//! `patron_restrictions`: suspensions of a patron (docs/specs/circulacao/suspensao.md).

use elzevir_domain::patron::PatronId;
use elzevir_domain::suspension::SuspensionEnd;
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

    /// When the suspension ends.
    pub(crate) fn end(self) -> SuspensionEnd {
        self.expires
            .map_or(SuspensionEnd::WhenLifted, SuspensionEnd::Day)
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
