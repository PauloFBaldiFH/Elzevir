// SPDX-License-Identifier: GPL-3.0-or-later
//! The circulation desk port: a patron's loans and restrictions, and the circulation
//! commands (interface §18.3).

use elzevir_app::accounts::AccountLine;
use elzevir_app::circulation_rules::{CirculationCommand, CirculationDeskError, CirculationOutput};
use elzevir_app::commands::{WriteCommand, WriteOutput, WriteRefusal};
use elzevir_app::holds::PatronHold;
use elzevir_app::loans::{LoanDesk, PatronLoan, RenewRequest, Renewed};
use elzevir_app::staff::StaffId;
use elzevir_domain::catalog::{BranchId, ItemId, Money, PolicyId};
use elzevir_domain::circulation_rules::RuleScopeKey;
use elzevir_domain::patron::{PatronCategoryId, PatronId};
use elzevir_domain::renewal::renewals_allowed;
use elzevir_domain::suspension::{RestrictionKind, SuspensionEnd};
use jiff::Zoned;
use jiff::civil::Date;
use rusqlite::OptionalExtension;

use crate::circulation_rules::read_rules;
use crate::{CommandFailed, ElzevirStore, SubmitError};

fn unavailable(error: &dyn std::fmt::Display) -> CirculationDeskError {
    CirculationDeskError::Unavailable(error.to_string())
}

impl LoanDesk for ElzevirStore {
    fn loans(&self, patron: PatronId) -> Result<Vec<PatronLoan>, CirculationDeskError> {
        self.readers()
            .read(|c| {
                let rules = read_rules(c)?;
                let mut statement = c.prepare_cached(
                    "SELECT coalesce(i.barcode, ''), n.title, l.due, l.renewals, l.on_site,
                            l.issued_at, l.renewed_at, i.holding_branch, p.category_id,
                            i.policy_id
                     FROM loans l
                     JOIN items i ON i.id = l.item_id
                     JOIN instances n ON n.id = i.instance_id
                     JOIN patrons p ON p.id = l.patron_id
                     WHERE l.patron_id = ?1",
                )?;
                let rows = statement.query_map([patron.0], |row| {
                    let moment = |index: usize, text: String| {
                        text.parse::<Zoned>().map_err(|error| {
                            rusqlite::Error::FromSqlConversionFailure(
                                index,
                                rusqlite::types::Type::Text,
                                error.into(),
                            )
                        })
                    };
                    let renewals: u32 = row.get(3)?;
                    // The same rules as the renewal: the branch holding the Item, the
                    // patron's category and the Item's policy (Koha: `GetRenewCount`).
                    let at = RuleScopeKey {
                        branch: Some(BranchId(row.get(7)?)),
                        category: Some(PatronCategoryId(row.get(8)?)),
                        policy: Some(PolicyId(row.get(9)?)),
                    };
                    let allowed = renewals_allowed(&rules, &at);
                    Ok(PatronLoan {
                        barcode: row.get(0)?,
                        title: row.get(1)?,
                        due: moment(2, row.get(2)?)?,
                        issued: moment(5, row.get(5)?)?,
                        renewed: row
                            .get::<_, Option<String>>(6)?
                            .map(|text| moment(6, text))
                            .transpose()?,
                        renewals,
                        renewals_left: u32::try_from(allowed.saturating_sub(u64::from(renewals)))
                            .unwrap_or(u32::MAX),
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

    fn restrictions(
        &self,
        patron: PatronId,
        today: Date,
    ) -> Result<Vec<(RestrictionKind, SuspensionEnd)>, CirculationDeskError> {
        self.readers()
            .read(|c| crate::restrictions::restrictions(c, patron, today))
            .map_err(|error| unavailable(&error))
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
        wait(self.submit_as(actor.0, WriteCommand::Circulation(command)))
    }

    fn renew_own(
        &self,
        patron: PatronId,
        request: RenewRequest,
    ) -> Result<Renewed, CirculationDeskError> {
        let request = RenewRequest {
            by_patron: Some(patron),
            ..request
        };
        // No staff member acts: the audit log keeps the patron inside the command.
        match self.patron_command(CirculationCommand::Renew(request))? {
            CirculationOutput::Renewed(renewed) => Ok(renewed),
            _ => Err(unavailable(&"the writer answered with another result")),
        }
    }

    fn patron_hold(
        &self,
        command: CirculationCommand,
    ) -> Result<CirculationOutput, CirculationDeskError> {
        // Only what a patron may ask for passes without a staff member behind it.
        match command {
            CirculationCommand::PlaceHold(_) | CirculationCommand::CancelHold(_) => {
                self.patron_command(command)
            }
            _ => Err(unavailable(&"not a hold command")),
        }
    }
}

impl ElzevirStore {
    fn patron_command(
        &self,
        command: CirculationCommand,
    ) -> Result<CirculationOutput, CirculationDeskError> {
        wait(self.submit(WriteCommand::Circulation(command)))
    }
}

fn wait(
    pending: Result<crate::Pending<crate::ElzevirHandler>, SubmitError>,
) -> Result<CirculationOutput, CirculationDeskError> {
    let pending = pending.map_err(|error| match error {
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
