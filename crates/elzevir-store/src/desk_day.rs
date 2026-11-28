// SPDX-License-Identifier: GPL-3.0-or-later
//! What `elzevir desk-day` reads between the steps of a day: Koha's numbers found through
//! what the switch kept, and what a patron owes (docs/specs/migracao/desk-day.md, item 8).

use elzevir_app::desk_day::{DeskItem, account_line};
use elzevir_app::holds::HoldId;
use elzevir_domain::catalog::{BranchId, Money};
use elzevir_domain::patron::PatronId;
use rusqlite::{OptionalExtension, params};

use crate::StoreError;
use crate::koha_migration::Keyed;

impl crate::ElzevirStore {
    /// The Item of an `itemnumber`, with its barcode, where it is and who has it; none when
    /// it did not come or has no barcode.
    ///
    /// # Errors
    /// The database could not be read.
    pub fn desk_item(&self, itemnumber: u64) -> Result<Option<DeskItem>, StoreError> {
        self.readers().read(|connection| {
            Ok(connection
                .query_row(
                    "SELECT i.barcode, i.holding_branch, l.patron_id, i.lost
                     FROM item_origins o
                     JOIN items i ON i.id = o.item_id
                     LEFT JOIN loans l ON l.item_id = i.id
                     WHERE o.system = 'koha.itemnumber' AND o.external_id = ?1
                       AND i.barcode IS NOT NULL
                     ORDER BY i.id LIMIT 1",
                    [itemnumber.to_string()],
                    |row| {
                        Ok(DeskItem {
                            barcode: row.get(0)?,
                            holding: BranchId(row.get(1)?),
                            borrower: row.get::<_, Option<i64>>(2)?.map(PatronId),
                            lost: row.get(3)?,
                        })
                    },
                )
                .optional()?)
        })
    }

    /// The patron of a `borrowernumber` and their branch; none when they did not come.
    ///
    /// # Errors
    /// The database could not be read.
    pub fn desk_patron(
        &self,
        borrowernumber: u64,
    ) -> Result<Option<(PatronId, BranchId)>, StoreError> {
        self.readers().read(|connection| {
            Ok(connection
                .query_row(
                    "SELECT p.id, p.branch_id FROM koha_keys k
                     JOIN patrons p ON p.id = k.local_id
                     WHERE k.kind = ?1 AND k.koha_key = ?2",
                    params![Keyed::Patron as i64, borrowernumber.to_string()],
                    |row| Ok((PatronId(row.get(0)?), BranchId(row.get(1)?))),
                )
                .optional()?)
        })
    }

    /// The `borrowernumber` the switch kept for the patron of a hold.
    ///
    /// # Errors
    /// The database could not be read.
    pub fn hold_borrowernumber(&self, hold: HoldId) -> Result<Option<String>, StoreError> {
        self.readers().read(|connection| {
            Ok(connection
                .query_row(
                    "SELECT k.koha_key FROM holds h
                     JOIN koha_keys k ON k.kind = ?2 AND k.local_id = h.patron_id
                     WHERE h.id = ?1",
                    params![hold.0, Keyed::Patron as i64],
                    |row| row.get(0),
                )
                .optional()?)
        })
    }

    /// What the patron owes: the outstanding debits, as the desk counts them.
    ///
    /// # Errors
    /// The database could not be read.
    pub fn owed_by(&self, patron: PatronId) -> Result<Money, StoreError> {
        self.readers()
            .read(|connection| Ok(crate::accounts::owed(connection, patron)?))
    }

    /// The patron's account lines, oldest first, as the day compares them.
    ///
    /// # Errors
    /// The database could not be read.
    pub fn desk_lines(&self, patron: PatronId) -> Result<Vec<String>, StoreError> {
        self.readers().read(|connection| {
            let mut statement = connection.prepare(
                "SELECT coalesce(debit_type, credit_type, '-'), status, amount, outstanding
                 FROM account_lines WHERE patron_id = ?1 ORDER BY id",
            )?;
            let rows = statement.query_map([patron.0], |row| {
                Ok(account_line(
                    &row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?.as_deref(),
                    row.get(2)?,
                    row.get(3)?,
                ))
            })?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }
}
