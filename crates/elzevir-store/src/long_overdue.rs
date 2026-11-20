// SPDX-License-Identifier: GPL-3.0-or-later
//! Items on loan long past their due date, declared lost (docs/specs/circulacao/conta.md,
//! items 20 to 26).

use elzevir_app::circulation_rules::{CirculationOutput, LongOverdueMarked, RuleKind};
use elzevir_app::commands::{WriteOutput, WriteRefusal};
use elzevir_domain::account::{DebitType, OverdueStatus, rule_money};
use elzevir_domain::catalog::{BranchId, Money, PolicyId};
use elzevir_domain::circulation_rules::{CirculationRule, RuleScopeKey, effective_rule};
use elzevir_domain::long_overdue::{LongOverdueSettings, is_long_overdue, lost_charges};
use elzevir_domain::patron::PatronId;
use jiff::Zoned;
use rusqlite::types::Type;
use rusqlite::{Connection, params};

use crate::accounts::{OverdueLoan, add_debit, close_overdue};
use crate::circulation_rules::read_rules;
use crate::returns::close_loan;
use crate::{CommandFailed, StoreError};

/// A loan whose Item may be declared lost.
struct LoanOut {
    /// What the debits are about: the loan, the patron, the Item, and the branch it is
    /// held at, whose rules apply.
    about: OverdueLoan,
    /// The Item's circulation policy.
    policy: PolicyId,
    /// The Item's replacement price.
    replacement_price: Option<Money>,
    /// When it was due.
    due: Zoned,
}

/// The loans whose Item does not have the lost status yet, oldest due date first.
fn loans_out(connection: &Connection, lost_status: i64) -> rusqlite::Result<Vec<LoanOut>> {
    let mut statement = connection.prepare(
        "SELECT l.id, l.patron_id, l.item_id, i.holding_branch, i.policy_id,
                i.replacement_price_cents, l.due
         FROM loans l JOIN items i ON i.id = l.item_id
         WHERE i.lost <> ?1
         ORDER BY l.due, l.id",
    )?;
    statement
        .query_map([lost_status], |row| {
            let due: String = row.get(6)?;
            let due = due.parse().map_err(|error: jiff::Error| {
                rusqlite::Error::FromSqlConversionFailure(6, Type::Text, error.into())
            })?;
            Ok(LoanOut {
                about: OverdueLoan {
                    loan: row.get(0)?,
                    patron: PatronId(row.get(1)?),
                    item: row.get(2)?,
                    branch: BranchId(row.get(3)?),
                },
                policy: PolicyId(row.get(4)?),
                replacement_price: row.get::<_, Option<i64>>(5)?.map(Money),
                due,
            })
        })?
        .collect()
}

/// Charges the patron for the lost Item, once per loan; returns whether anything was
/// charged.
///
/// Koha: `C4::Accounts::chargelostitem`, with the branch of `LostChargesControl`
/// `ItemHomeLibrary` and `HomeOrHoldingBranch` `holdingbranch`.
fn charge_lost(
    connection: &Connection,
    rules: &[CirculationRule],
    loan: &LoanOut,
    at: &str,
) -> rusqlite::Result<bool> {
    let charged_before: bool = connection.query_row(
        "SELECT EXISTS (SELECT 1 FROM account_lines WHERE loan_id = ?1 AND debit_type = ?2)",
        params![loan.about.loan, DebitType::Lost.code()],
        |row| row.get(0),
    )?;
    if charged_before {
        return Ok(false);
    }
    let scope = RuleScopeKey {
        branch: Some(loan.about.branch),
        category: None,
        policy: Some(loan.policy),
    };
    let fee = effective_rule(rules, RuleKind::LostItemProcessingFee, &scope)
        .and_then(|rule| rule_money(&rule.value));
    let charges = lost_charges(fee, loan.replacement_price);
    for (kind, amount) in &charges {
        add_debit(connection, &loan.about, *kind, None, *amount, at)?;
    }
    Ok(!charges.is_empty())
}

/// Applies `circ.mark_long_overdue`: every Item on loan long past its due date at `at`
/// gets the lost status; with `charge`, its fine stops as `LOST`, the patron is charged
/// for it and the loan ends.
///
/// Koha: `misc/cronjobs/longoverdue.pl`, `C4::Circulation::LostItem`.
pub(crate) fn mark_long_overdue(
    connection: &Connection,
    at: &Zoned,
    settings: &LongOverdueSettings,
) -> Result<WriteOutput, CommandFailed<WriteRefusal>> {
    let storage = |error: rusqlite::Error| CommandFailed::from(StoreError::from(error));
    let rules = read_rules(connection).map_err(storage)?;
    let when = at.to_string();
    let mut marked = LongOverdueMarked::default();
    for loan in loans_out(connection, settings.lost_status).map_err(storage)? {
        if !is_long_overdue(&loan.due, at, settings) {
            continue;
        }
        connection
            .execute(
                "UPDATE items SET lost = ?2, row_version = row_version + 1 WHERE id = ?1",
                params![loan.about.item, settings.lost_status],
            )
            .map_err(storage)?;
        marked.lost += 1;
        if settings.charge {
            close_overdue(
                connection,
                loan.about.patron,
                loan.about.item,
                OverdueStatus::Lost,
                &when,
            )
            .map_err(storage)?;
            if charge_lost(connection, &rules, &loan, &when).map_err(storage)? {
                marked.charged += 1;
            }
        }
        if settings.ends_loan() {
            close_loan(connection, loan.about.loan, &when, loan.about.branch).map_err(storage)?;
        }
    }
    Ok(WriteOutput::Circulation(
        CirculationOutput::LongOverdueMarked(marked),
    ))
}
