// SPDX-License-Identifier: GPL-3.0-or-later
//! The patron port on SQLite: commands through the writer, reads through the pool
//! (docs/specs/circulacao/leitores.md, items 10 to 14).

use elzevir_app::commands::{WriteCommand, WriteOutput, WriteRefusal};
use elzevir_app::patron_account::{
    PatronAccount, PatronAccountError, PatronAccounts, PatronCredentials,
};
use elzevir_app::patrons::{
    MAX_SEARCH_ROWS, Patron, PatronCategory, PatronCommand, PatronDesk, PatronDeskError,
    PatronFields, PatronOutput, PatronQuery, PatronRow, ReadingHistory,
};
use elzevir_app::staff::StaffId;
use elzevir_domain::catalog::{BranchId, Money};
use elzevir_domain::patron::{Cpf, Enrolment, PatronCategoryId, PatronId};
use jiff::Zoned;
use jiff::civil::Date;
use rusqlite::{OptionalExtension, Row, params};

use crate::{CommandFailed, ElzevirStore, SubmitError};

fn unavailable(error: impl std::fmt::Display) -> PatronDeskError {
    PatronDeskError::Unavailable(error.to_string())
}

/// A stored date; a malformed one is a storage failure, never a guess.
fn day(text: &str) -> rusqlite::Result<Date> {
    text.parse().map_err(|error: jiff::Error| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, error.into())
    })
}

fn optional_day(text: Option<&str>) -> rusqlite::Result<Option<Date>> {
    text.map(day).transpose()
}

const PATRON: &str = "SELECT p.id, p.row_version, p.cardnumber, p.surname, p.first_name, p.cpf,
                             p.date_of_birth, p.email, p.phone, p.address, p.branch_id,
                             p.category_id, p.enrolled, p.expires, p.staff_note,
                             p.reading_history, b.name, c.name,
                             p.password_hash IS NOT NULL
                      FROM patrons p
                      JOIN branches b ON b.id = p.branch_id
                      JOIN patron_categories c ON c.id = p.category_id
                      WHERE p.id = ?1";

fn patron_of(row: &Row<'_>) -> rusqlite::Result<Patron> {
    let cpf: Option<String> = row.get(5)?;
    Ok(Patron {
        id: PatronId(row.get(0)?),
        row_version: row.get(1)?,
        fields: PatronFields {
            cardnumber: row.get(2)?,
            surname: row.get(3)?,
            first_name: row.get(4)?,
            // A CPF that no longer parses is shown as absent rather than failing the record.
            cpf: cpf.as_deref().and_then(|digits| Cpf::parse(digits).ok()),
            date_of_birth: optional_day(row.get::<_, Option<String>>(6)?.as_deref())?,
            email: row.get(7)?,
            phone: row.get(8)?,
            address: row.get(9)?,
            branch: BranchId(row.get(10)?),
            category: PatronCategoryId(row.get(11)?),
            enrolled: day(&row.get::<_, String>(12)?)?,
            expires: Some(day(&row.get::<_, String>(13)?)?),
            staff_note: row.get(14)?,
            reading_history: if row.get::<_, i64>(15)? == 1 {
                ReadingHistory::Kept
            } else {
                ReadingHistory::NotKept
            },
        },
        branch_name: row.get(16)?,
        category_name: row.get(17)?,
        has_password: row.get(18)?,
    })
}

/// `?1` the card number, `?2` the CPF digits, `?3` a `LIKE` pattern for names (or
/// `NULL`), `?4` the most rows.
// Unit separator between due moments: a zoned date-time never holds it.
const SEARCH: &str = "SELECT p.id, p.cardnumber, p.surname, p.first_name, c.name, p.expires,
                             b.name,
                             (SELECT group_concat(l.due, char(31)) FROM loans l
                              WHERE l.patron_id = p.id),
                             (SELECT count(*) FROM holds h WHERE h.patron_id = p.id),
                             (SELECT coalesce(sum(a.outstanding), 0) FROM account_lines a
                              WHERE a.patron_id = p.id AND a.debit_type IS NOT NULL)
                      FROM patrons p
                      JOIN patron_categories c ON c.id = p.category_id
                      JOIN branches b ON b.id = p.branch_id
                      WHERE p.cardnumber = ?1
                         OR p.cpf = ?2
                         OR (?3 IS NOT NULL
                             AND (p.surname || ' ' || coalesce(p.first_name, '') LIKE ?3 ESCAPE '\\'
                                  OR coalesce(p.first_name, '') || ' ' || p.surname LIKE ?3 ESCAPE '\\'))
                      ORDER BY p.surname, p.first_name, p.id
                      LIMIT ?4";

/// The due moments `SEARCH` joins with the unit separator.
fn dues(joined: Option<&str>) -> rusqlite::Result<Vec<Zoned>> {
    joined
        .into_iter()
        .flat_map(|text| text.split('\u{1f}'))
        .map(|text| {
            text.parse::<Zoned>().map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    7,
                    rusqlite::types::Type::Text,
                    error.into(),
                )
            })
        })
        .collect()
}

/// `%text%` with the `LIKE` wildcards of `text` escaped.
fn contains(text: &str) -> String {
    let mut pattern = String::with_capacity(text.len() + 2);
    pattern.push('%');
    for c in text.chars() {
        if matches!(c, '%' | '_' | '\\') {
            pattern.push('\\');
        }
        pattern.push(c);
    }
    pattern.push('%');
    pattern
}

impl PatronDesk for ElzevirStore {
    fn apply(
        &self,
        actor: StaffId,
        command: PatronCommand,
    ) -> Result<PatronOutput, PatronDeskError> {
        let pending = self
            .submit_as(actor.0, WriteCommand::Patrons(command))
            .map_err(|error| match error {
                SubmitError::Busy => PatronDeskError::Busy,
                SubmitError::Closed => unavailable(error),
            })?;
        match pending.wait() {
            Ok(WriteOutput::Patrons(output)) => Ok(output),
            Ok(_) => Err(unavailable("the writer answered with another result")),
            Err(CommandFailed::Rejected(WriteRefusal::Patrons(refusal))) => {
                Err(PatronDeskError::Refused(refusal))
            }
            Err(failure) => Err(unavailable(failure)),
        }
    }

    fn patron(&self, id: PatronId) -> Result<Option<Patron>, PatronDeskError> {
        self.readers()
            .read(|c| c.query_row(PATRON, [id.0], patron_of).optional())
            .map_err(unavailable)
    }

    fn search(&self, query: &PatronQuery) -> Result<Vec<PatronRow>, PatronDeskError> {
        let cpf = query.cpf.as_ref().map(|cpf| cpf.digits().to_owned());
        let name = query.name.as_deref().map(contains);
        let limit = i64::try_from(MAX_SEARCH_ROWS).unwrap_or(i64::MAX);
        self.readers()
            .read(|c| {
                let mut statement = c.prepare_cached(SEARCH)?;
                let rows =
                    statement.query_map(params![query.cardnumber, cpf, name, limit], |row| {
                        Ok(PatronRow {
                            id: PatronId(row.get(0)?),
                            cardnumber: row.get(1)?,
                            surname: row.get(2)?,
                            first_name: row.get(3)?,
                            category_name: row.get(4)?,
                            expires: day(&row.get::<_, String>(5)?)?,
                            branch_name: row.get(6)?,
                            loans_due: dues(row.get::<_, Option<String>>(7)?.as_deref())?,
                            holds: row.get(8)?,
                            owed: Money(row.get(9)?),
                        })
                    })?;
                rows.collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(unavailable)
    }

    fn categories(&self) -> Result<Vec<PatronCategory>, PatronDeskError> {
        self.readers()
            .read(|c| {
                let mut statement = c.prepare_cached(
                    "SELECT c.id, c.name, c.enrolment_months, c.enrolment_until,
                            (SELECT count(*) FROM patrons p WHERE p.category_id = c.id)
                     FROM patron_categories c ORDER BY c.name, c.id",
                )?;
                let rows = statement.query_map([], |row| {
                    let months: Option<u16> = row.get(2)?;
                    let until: Option<String> = row.get(3)?;
                    let enrolment = match (months, until) {
                        (Some(months), _) => Enrolment::Months(months),
                        (None, Some(until)) => Enrolment::Until(day(&until)?),
                        // The table's CHECK keeps one of the two; read a broken row as
                        // a failure rather than inventing a period.
                        (None, None) => return Err(rusqlite::Error::InvalidQuery),
                    };
                    Ok(PatronCategory {
                        id: PatronCategoryId(row.get(0)?),
                        name: row.get(1)?,
                        enrolment,
                        patrons: u64::try_from(row.get::<_, i64>(4)?).unwrap_or(0),
                    })
                })?;
                rows.collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(unavailable)
    }
}

impl PatronAccounts for ElzevirStore {
    fn credentials(
        &self,
        cardnumber: &str,
    ) -> Result<Option<PatronCredentials>, PatronAccountError> {
        self.readers()
            .read(|c| {
                c.query_row(
                    "SELECT id, coalesce(nullif(first_name, ''), surname), password_hash
                     FROM patrons WHERE cardnumber = ?1",
                    params![cardnumber],
                    |row| {
                        Ok(PatronCredentials {
                            id: PatronId(row.get(0)?),
                            greeting: row.get(1)?,
                            password_hash: row.get(2)?,
                        })
                    },
                )
                .optional()
            })
            .map_err(|error: rusqlite::Error| PatronAccountError(error.to_string()))
    }

    fn account(&self, id: PatronId) -> Result<Option<PatronAccount>, PatronAccountError> {
        self.readers()
            .read(|c| c.query_row(PATRON, params![id.0], patron_of).optional())
            .map_err(|error: rusqlite::Error| PatronAccountError(error.to_string()))
            .map(|patron| {
                patron.map(|patron| PatronAccount {
                    name: patron.fields.first_name.as_deref().map_or_else(
                        || patron.fields.surname.clone(),
                        |first| format!("{first} {}", patron.fields.surname),
                    ),
                    cardnumber: patron.fields.cardnumber,
                    branch_name: patron.branch_name,
                    category_name: patron.category_name,
                    // A stored patron always has its expiry (`patrons.expires` is NOT NULL).
                    expires: patron.fields.expires.unwrap_or(patron.fields.enrolled),
                    reading_history: patron.fields.reading_history,
                })
            })
    }

    fn set_reading_history(
        &self,
        id: PatronId,
        choice: ReadingHistory,
    ) -> Result<(), PatronAccountError> {
        let command = WriteCommand::Patrons(PatronCommand::SetReadingHistory {
            patron: id,
            reading_history: choice,
        });
        // No staff member acts: the audit log keeps the patron inside the command.
        let pending = self
            .submit(command)
            .map_err(|error| PatronAccountError(error.to_string()))?;
        match pending.wait() {
            Ok(_) => Ok(()),
            Err(failure) => Err(PatronAccountError(failure.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::contains;

    #[test]
    fn like_wildcards_are_escaped() {
        assert_eq!(contains("a_b%c\\"), "%a\\_b\\%c\\\\%");
    }
}
