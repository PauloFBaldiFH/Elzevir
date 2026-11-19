// SPDX-License-Identifier: GPL-3.0-or-later
//! `circ.renew`: renewing a loan (docs/specs/circulacao/renovacao.md).

use elzevir_app::circulation_rules::{CirculationOutput, CirculationRefusal};
use elzevir_app::commands::{WriteOutput, WriteRefusal};
use elzevir_app::loans::{RenewRefusal, RenewRequest, Renewed, checked_barcode};
use elzevir_domain::catalog::{BranchId, PolicyId};
use elzevir_domain::checkout::loan_rule;
use elzevir_domain::circulation::{RenewalBase, renewal_due};
use elzevir_domain::circulation_rules::RuleScopeKey;
use elzevir_domain::due_date::{ConsiderLibraryHours, DueDateContext, LibraryHours};
use elzevir_domain::patron::PatronId;
use elzevir_domain::renewal::{RenewalFacts, RenewalSettings, can_renew};
use jiff::Zoned;
use rusqlite::{Connection, OptionalExtension, params};

use crate::calendar::read_calendar;
use crate::circulation_rules::read_rules;
use crate::loans::{USE_DAYS_MODE, find_patron};
use crate::{CommandFailed, StoreError};

type Applied = Result<WriteOutput, CommandFailed<WriteRefusal>>;

fn refused(refusal: RenewRefusal) -> CommandFailed<WriteRefusal> {
    CommandFailed::Rejected(WriteRefusal::Circulation(CirculationRefusal::Renew(
        refusal,
    )))
}

fn storage(error: rusqlite::Error) -> CommandFailed<WriteRefusal> {
    StoreError::from(error).into()
}

fn zoned(text: &str, column: usize) -> rusqlite::Result<Zoned> {
    text.parse().map_err(|error: jiff::Error| {
        rusqlite::Error::FromSqlConversionFailure(column, rusqlite::types::Type::Text, error.into())
    })
}

struct Loan {
    id: i64,
    patron: PatronId,
    due: Zoned,
    renewals: u32,
    on_site: bool,
    branch: BranchId,
    policy: PolicyId,
}

/// The loan of the Item with `barcode`, or why there is none.
fn find_loan(
    connection: &Connection,
    barcode: &str,
) -> rusqlite::Result<Result<Loan, RenewRefusal>> {
    let Some(item) = connection
        .query_row(
            "SELECT id FROM items WHERE barcode = ?1",
            [barcode],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
    else {
        return Ok(Err(RenewRefusal::UnknownBarcode));
    };
    connection
        .query_row(
            "SELECT l.id, l.patron_id, l.due, l.renewals, l.on_site, i.holding_branch, i.policy_id
             FROM loans l JOIN items i ON i.id = l.item_id WHERE l.item_id = ?1",
            [item],
            |row| {
                Ok(Loan {
                    id: row.get(0)?,
                    patron: PatronId(row.get(1)?),
                    due: zoned(&row.get::<_, String>(2)?, 2)?,
                    renewals: row.get(3)?,
                    on_site: row.get(4)?,
                    branch: BranchId(row.get(5)?),
                    policy: PolicyId(row.get(6)?),
                })
            },
        )
        .optional()
        .map(|loan| loan.ok_or(RenewRefusal::NotOnLoan))
}

/// Whether another loan of the patron was due before `now`.
fn other_overdue(connection: &Connection, loan: &Loan, now: &Zoned) -> rusqlite::Result<bool> {
    let mut statement =
        connection.prepare_cached("SELECT due FROM loans WHERE patron_id = ?1 AND id <> ?2")?;
    let dues = statement.query_map(params![loan.patron.0, loan.id], |row| {
        zoned(&row.get::<_, String>(0)?, 0)
    })?;
    for due in dues {
        if &due? < now {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Applies `circ.renew` inside the writer's transaction. The rules and the calendar are
/// those of the branch that holds the Item, the patron's category and the Item's policy;
/// the new period starts at the due date (`ManualRenewalPeriodBase`, the default).
///
/// Koha: `C4::Circulation::CanBookBeRenewed` and `C4::Circulation::AddRenewal`.
pub(crate) fn renew(connection: &Connection, request: &RenewRequest) -> Applied {
    let barcode =
        checked_barcode(&request.barcode).ok_or_else(|| refused(RenewRefusal::UnknownBarcode))?;
    let loan = find_loan(connection, barcode)
        .map_err(storage)?
        .map_err(refused)?;
    let (category, expires) = find_patron(connection, loan.patron)
        .map_err(storage)?
        .ok_or_else(|| refused(RenewRefusal::NotOnLoan))?;
    let rules = read_rules(connection).map_err(storage)?;
    let at = RuleScopeKey {
        branch: Some(loan.branch),
        category: Some(category),
        policy: Some(loan.policy),
    };
    can_renew(
        &RenewalFacts {
            rules: &rules,
            at,
            due: loan.due.clone(),
            renewals: loan.renewals,
            on_site: loan.on_site,
            other_overdue: other_overdue(connection, &loan, &request.at).map_err(storage)?,
            now: request.at.clone(),
        },
        &RenewalSettings::default(),
        request.override_limit,
    )
    .map_err(|refusal| refused(RenewRefusal::Refused(refusal)))?;
    let calendar = read_calendar(connection, loan.branch.0).map_err(storage)?;
    let rule = loan_rule(&rules, &at, USE_DAYS_MODE);
    let due = renewal_due(
        &loan.due,
        &request.at,
        RenewalBase::DueDate,
        &DueDateContext {
            rule: &rule,
            calendar: &calendar,
            hours: &LibraryHours::default(),
            consider_hours: ConsiderLibraryHours::Ignore,
            return_before_expiry: false,
            patron_expiry: Some(expires),
        },
    )
    .map_err(|_| refused(RenewRefusal::NoDueDate))?;
    let renewals = loan.renewals.saturating_add(1);
    connection
        .execute(
            "UPDATE loans SET due = ?2, renewals = ?3, renewed_at = ?4 WHERE id = ?1",
            params![loan.id, due.to_string(), renewals, request.at.to_string()],
        )
        .map_err(storage)?;
    Ok(WriteOutput::Circulation(CirculationOutput::Renewed(
        Renewed {
            patron: loan.patron,
            due,
            renewals,
        },
    )))
}
