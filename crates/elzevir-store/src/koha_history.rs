// SPDX-License-Identifier: GPL-3.0-or-later
//! `patrons.migrate_koha_history`: one write of Koha's returned loans
//! (docs/specs/migracao/circulacao-do-koha.md, items 14 to 17).

use elzevir_app::commands::WriteRefusal;
use elzevir_app::koha_migration::{KohaHistoryBatch, KohaHistoryMigrated, KohaReturnedLoan};
use elzevir_app::patrons::PatronOutput;
use rusqlite::{Connection, OptionalExtension, params};

use crate::koha_circulation::{Found, branch, item, patron};
use crate::koha_migration::{Keyed, key, set_key};
use crate::{CommandFailed, StoreError};

/// Applies one write.
pub(crate) fn apply(
    connection: &Connection,
    batch: &KohaHistoryBatch,
) -> Result<PatronOutput, CommandFailed<WriteRefusal>> {
    let mut done = KohaHistoryMigrated::default();
    for loan in &batch.returned {
        let number = loan.issue_id.to_string();
        if key(connection, Keyed::ReturnedLoan, &number)?.is_some() {
            done.already_migrated += 1;
            continue;
        }
        match add_returned(connection, loan)? {
            Ok((id, with_patron)) => {
                set_key(connection, Keyed::ReturnedLoan, &number, id)?;
                done.returned += 1;
                done.with_patron += usize::from(with_patron);
            }
            Err(why) => done.left.push((loan.issue_id, why)),
        }
    }
    Ok(PatronOutput::KohaHistoryMigrated(done))
}

/// Item 15: the returned loan, with its patron only when the patron came and keeps a
/// reading history, as a return in the Elzevir does.
fn add_returned(
    connection: &Connection,
    loan: &KohaReturnedLoan,
) -> Result<Found<(i64, bool)>, StoreError> {
    let item = match item(connection, &loan.item)? {
        Ok(id) => id,
        Err(why) => return Ok(Err(why)),
    };
    let lent_at = match branch(connection, &loan.branch)? {
        Ok(id) => id,
        Err(why) => return Ok(Err(why)),
    };
    let returned_at = match branch(connection, &loan.return_branch)? {
        Ok(id) => id,
        Err(why) => return Ok(Err(why)),
    };
    let reader = match loan.borrowernumber {
        Some(number) => match patron(connection, number)? {
            Ok(id) => connection
                .query_row(
                    "SELECT id FROM patrons WHERE id = ?1 AND reading_history = 1",
                    [id],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?,
            Err(_) => None,
        },
        None => None,
    };
    connection.execute(
        "INSERT INTO returned_loans (item_id, patron_id, branch_id, issued_at, due, on_site,
                                     returned_at, return_branch_id, renewals)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            item,
            reader,
            lent_at,
            loan.issued_at,
            loan.due,
            loan.on_site,
            loan.returned_at,
            returned_at,
            loan.renewals
        ],
    )?;
    Ok(Ok((connection.last_insert_rowid(), reader.is_some())))
}
