// SPDX-License-Identifier: GPL-3.0-or-later
//! The circulation desk port: a patron's loans and suspension, and the circulation
//! commands (interface §18.3).

use elzevir_app::accounts::AccountLine;
use elzevir_app::circulation_rules::{CirculationCommand, CirculationDeskError, CirculationOutput};
use elzevir_app::commands::{WriteCommand, WriteOutput, WriteRefusal};
use elzevir_app::holds::PatronHold;
use elzevir_app::loans::{LoanDesk, PatronLoan};
use elzevir_app::staff::StaffId;
use elzevir_domain::catalog::{BranchId, ItemId, Money};
use elzevir_domain::patron::PatronId;
use elzevir_domain::suspension::SuspensionEnd;
use jiff::civil::Date;
use rusqlite::OptionalExtension;

use crate::restrictions::suspension;
use crate::{CommandFailed, ElzevirStore, SubmitError};

fn unavailable(error: &dyn std::fmt::Display) -> CirculationDeskError {
    CirculationDeskError::Unavailable(error.to_string())
}

impl LoanDesk for ElzevirStore {
    fn loans(&self, patron: PatronId) -> Result<Vec<PatronLoan>, CirculationDeskError> {
        self.readers()
            .read(|c| {
                let mut statement = c.prepare_cached(
                    "SELECT coalesce(i.barcode, ''), n.title, l.due, l.renewals, l.on_site
                     FROM loans l
                     JOIN items i ON i.id = l.item_id
                     JOIN instances n ON n.id = i.instance_id
                     WHERE l.patron_id = ?1",
                )?;
                let rows = statement.query_map([patron.0], |row| {
                    let due: String = row.get(2)?;
                    let due = due.parse().map_err(|error: jiff::Error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            2,
                            rusqlite::types::Type::Text,
                            error.into(),
                        )
                    })?;
                    Ok(PatronLoan {
                        barcode: row.get(0)?,
                        title: row.get(1)?,
                        due,
                        renewals: row.get(3)?,
                        on_site: row.get(4)?,
                    })
                })?;
                let mut loans = rows.collect::<rusqlite::Result<Vec<_>>>()?;
                // The text sorts wrongly across time-zone offsets; the instants do not.
                loans.sort_by(|a, b| {
                    a.due
                        .timestamp()
                        .cmp(&b.due.timestamp())
                        .then_with(|| a.barcode.cmp(&b.barcode))
                });
                Ok::<_, rusqlite::Error>(loans)
            })
            .map_err(|error| unavailable(&error))
    }

    fn suspension(
        &self,
        patron: PatronId,
        today: Date,
    ) -> Result<Option<SuspensionEnd>, CirculationDeskError> {
        self.readers()
            .read(|c| suspension(c, patron))
            .map_err(|error| unavailable(&error))
            .map(|found| {
                found
                    .filter(|suspension| suspension.holds_on(today))
                    .map(crate::restrictions::Suspension::end)
            })
    }

    fn owed(&self, patron: PatronId) -> Result<Money, CirculationDeskError> {
        self.readers()
            .read(|c| crate::accounts::owed(c, patron))
            .map_err(|error| unavailable(&error))
    }

    fn account(&self, patron: PatronId) -> Result<Vec<AccountLine>, CirculationDeskError> {
        self.readers()
            .read(|c| crate::accounts::lines(c, patron))
            .map_err(|error| unavailable(&error))
    }

    fn holds(&self, patron: PatronId) -> Result<Vec<PatronHold>, CirculationDeskError> {
        self.readers()
            .read(|c| crate::holds::patron_holds(c, patron))
            .map_err(|error| unavailable(&error))
    }

    fn holding_branch(&self, item: ItemId) -> Result<Option<BranchId>, CirculationDeskError> {
        self.readers()
            .read(|c| {
                c.query_row(
                    "SELECT holding_branch FROM items WHERE id = ?1",
                    [item.0],
                    |row| row.get(0).map(BranchId),
                )
                .optional()
            })
            .map_err(|error| unavailable(&error))
    }

    fn borrower(&self, item: ItemId) -> Result<Option<PatronId>, CirculationDeskError> {
        self.readers()
            .read(|c| {
                c.query_row(
                    "SELECT patron_id FROM loans WHERE item_id = ?1",
                    [item.0],
                    |row| row.get(0).map(PatronId),
                )
                .optional()
            })
            .map_err(|error| unavailable(&error))
    }

    fn circulate(
        &self,
        actor: StaffId,
        command: CirculationCommand,
    ) -> Result<CirculationOutput, CirculationDeskError> {
        let pending = self
            .submit_as(actor.0, WriteCommand::Circulation(command))
            .map_err(|error| match error {
                SubmitError::Busy => CirculationDeskError::Busy,
                SubmitError::Closed => unavailable(&error),
            })?;
        match pending.wait() {
            Ok(WriteOutput::Circulation(output)) => Ok(output),
            Ok(_) => Err(unavailable(&"the writer answered with another result")),
            Err(CommandFailed::Rejected(WriteRefusal::Circulation(refusal))) => {
                Err(CirculationDeskError::Refused(refusal))
            }
            Err(failure) => Err(unavailable(&failure)),
        }
    }
}
