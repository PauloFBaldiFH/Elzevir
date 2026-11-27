// SPDX-License-Identifier: GPL-3.0-or-later
//! Overdue fines of loans still out and of late returns (docs/specs/circulacao/conta.md).

use elzevir_app::circulation_rules::{CirculationOutput, OverduesCharged};
use elzevir_app::commands::{WriteOutput, WriteRefusal};
use elzevir_domain::account::{DebtSettings, fine_rule};
use elzevir_domain::catalog::{BranchId, Money, PolicyId};
use elzevir_domain::checkout::loan_rule;
use elzevir_domain::circulation_rules::{CirculationRule, RuleScopeKey};
use elzevir_domain::fine::{FineContext, FineSettings, calc_fine};
use elzevir_domain::patron::PatronId;
use jiff::Zoned;
use rusqlite::Connection;
use rusqlite::types::Type;

use crate::accounts::{OverdueLoan, charge_overdue};
use crate::calendar::read_calendar;
use crate::circulation_rules::read_rules;
use crate::loans::{USE_DAYS_MODE, find_patron};
use crate::{CommandFailed, StoreError};

/// A loan whose fine is worked out.
pub(crate) struct LateLoan {
    /// `loans.id`.
    pub(crate) loan: i64,
    /// Who borrowed.
    pub(crate) patron: PatronId,
    /// The Item.
    pub(crate) item: i64,
    /// Where the Item was held while out: the branch whose rules and calendar count.
    pub(crate) holding: BranchId,
    /// The Item's circulation policy.
    pub(crate) policy: PolicyId,
    /// The Item's replacement price, in cents.
    pub(crate) replacement_price: Option<i64>,
    /// When it was due.
    pub(crate) due: Zoned,
}

/// When the fine is worked out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FineMoment {
    /// At a late return: always charged.
    Return,
    /// By the daily run: not on a day the branch is closed, unless
    /// `ChargeFinesOnClosedDays`.
    DailyRun,
}

/// Works out the loan's fine at `at` and charges it, or brings its growing fine to it;
/// returns the fine as it stands, or `None` when nothing was charged.
///
/// The rules, the calendar and the branch of the fine are those of the branch the Item is
/// held at (`CircControl` `ItemHomeLibrary` with `HomeOrHoldingBranch` `holdingbranch`,
/// Koha's defaults).
///
/// Koha: `C4::Circulation::_CalculateAndUpdateFine`, `misc/cronjobs/fines.pl`.
pub(crate) fn update_overdue_fine(
    connection: &Connection,
    rules: &[CirculationRule],
    late: &LateLoan,
    at: &Zoned,
    moment: FineMoment,
) -> rusqlite::Result<Option<Money>> {
    let settings = FineSettings::default();
    if moment == FineMoment::DailyRun
        && !settings.charge_on_closed_days
        && read_calendar(connection, late.holding.0)?.is_closed(at.date())
    {
        return Ok(None);
    }
    let Some(amount) = overdue_fine(connection, rules, late, at)? else {
        return Ok(None);
    };
    if amount.0 <= 0 {
        return Ok(None);
    }
    let overdue = OverdueLoan {
        loan: late.loan,
        patron: late.patron,
        item: late.item,
        branch: late.holding,
    };
    charge_overdue(
        connection,
        &overdue,
        amount,
        DebtSettings::default().max_fine,
        &at.to_string(),
    )
}

/// The loan's fine at `at`, before `MaxFine`, by the rules and the calendar of the branch
/// the Item is held at; `None` when the patron is gone.
///
/// Koha: `C4::Overdues::CalcFine`
pub(crate) fn overdue_fine(
    connection: &Connection,
    rules: &[CirculationRule],
    late: &LateLoan,
    at: &Zoned,
) -> rusqlite::Result<Option<Money>> {
    let settings = FineSettings::default();
    let Some((category, _)) = find_patron(connection, late.patron)? else {
        return Ok(None);
    };
    let scope = RuleScopeKey {
        branch: Some(late.holding),
        category: Some(category),
        policy: Some(late.policy),
    };
    let calendar = read_calendar(connection, late.holding.0)?;
    let rule = fine_rule(rules, &scope, loan_rule(rules, &scope, USE_DAYS_MODE).unit);
    let computed = calc_fine(
        &late.due,
        at,
        &FineContext {
            rule: &rule,
            calendar: &calendar,
            fines_calendar: settings.calendar,
            include_grace: settings.include_grace,
            replacement_price: late.replacement_price.map(Money),
            default_replacement_cost: None,
            overdue_recall: false,
        },
    );
    Ok(Some(computed.amount))
}

/// The loans out whose Item is not lost, by patron and then in the order they were made.
fn loans_out(connection: &Connection) -> rusqlite::Result<Vec<LateLoan>> {
    let mut statement = connection.prepare(
        "SELECT l.id, l.patron_id, l.item_id, i.holding_branch, i.policy_id,
                i.replacement_price_cents, l.due
         FROM loans l JOIN items i ON i.id = l.item_id
         WHERE i.lost = 0
         ORDER BY l.patron_id, l.id",
    )?;
    statement
        .query_map([], |row| {
            let due: String = row.get(6)?;
            let due = due.parse().map_err(|error: jiff::Error| {
                rusqlite::Error::FromSqlConversionFailure(6, Type::Text, error.into())
            })?;
            Ok(LateLoan {
                loan: row.get(0)?,
                patron: PatronId(row.get(1)?),
                item: row.get(2)?,
                holding: BranchId(row.get(3)?),
                policy: PolicyId(row.get(4)?),
                replacement_price: row.get(5)?,
                due,
            })
        })?
        .collect()
}

/// Applies `circ.charge_overdues`: brings the fine of every loan overdue at `at` to what
/// it is worth then.
///
/// Koha: `misc/cronjobs/fines.pl` (`Getoverdues`, `CalcFine`, `UpdateFine`), with the
/// patrons in order as `Getoverdues` sorts them, which matters under `MaxFine`.
pub(crate) fn charge_overdues(
    connection: &Connection,
    at: &Zoned,
) -> Result<WriteOutput, CommandFailed<WriteRefusal>> {
    let storage = |error: rusqlite::Error| CommandFailed::from(StoreError::from(error));
    let rules = read_rules(connection).map_err(storage)?;
    let mut charged = OverduesCharged::default();
    for late in loans_out(connection).map_err(storage)? {
        if late.due >= *at {
            continue;
        }
        charged.overdue += 1;
        if update_overdue_fine(connection, &rules, &late, at, FineMoment::DailyRun)
            .map_err(storage)?
            .is_some()
        {
            charged.charged += 1;
        }
    }
    Ok(WriteOutput::Circulation(
        CirculationOutput::OverduesCharged(charged),
    ))
}
