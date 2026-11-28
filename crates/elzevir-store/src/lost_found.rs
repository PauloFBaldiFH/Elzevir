// SPDX-License-Identifier: GPL-3.0-or-later
//! What finding a lost Item does to the charges of its loss, by the rules `lostreturn` and
//! `processingreturn` (docs/specs/circulacao/lost-items.md, items 9 to 16).

use elzevir_app::circulation_rules::RuleKind;
use elzevir_app::item_status::LostFound;
use elzevir_domain::account::{CreditType, DebitType, OverdueStatus};
use elzevir_domain::catalog::{BranchId, Money, PolicyId};
use elzevir_domain::circulation_rules::{CirculationRule, RuleScopeKey, effective_rule};
use elzevir_domain::lost_item::{LostReturn, refund};
use elzevir_domain::patron::PatronId;
use jiff::Zoned;
use rusqlite::types::Type;
use rusqlite::{Connection, OptionalExtension, params};

use crate::accounts::close_overdue;
use crate::overdue_fines::{FineMoment, LateLoan, update_overdue_fine};

/// Where the Item was found: the desk's branch, and whether at a checkin, where the
/// `charge` policy charges the fine of the last loan (item 13).
#[derive(Debug, Clone, Copy)]
pub(crate) struct FoundAt {
    /// The branch of the desk, when there is one.
    pub(crate) desk: Option<BranchId>,
    /// Found by a checkin.
    pub(crate) checkin: bool,
}

impl FoundAt {
    /// Found away from any desk: the inventory or the status page.
    pub(crate) const ELSEWHERE: Self = Self {
        desk: None,
        checkin: false,
    };

    /// Found by a checkin at `desk`.
    pub(crate) const fn checkin(desk: BranchId) -> Self {
        Self {
            desk: Some(desk),
            checkin: true,
        }
    }
}

/// A charge of the loss, as the refund needs it.
struct Charge {
    id: i64,
    patron: PatronId,
    amount: i64,
    outstanding: i64,
    loan: Option<i64>,
}

/// The newest charge of `kind` for the Item not yet settled by a finding.
fn newest_charge(
    connection: &Connection,
    item: i64,
    kind: DebitType,
) -> rusqlite::Result<Option<Charge>> {
    connection
        .query_row(
            "SELECT id, patron_id, amount, outstanding, loan_id FROM account_lines
             WHERE item_id = ?1 AND debit_type = ?2
               AND (status IS NULL OR status <> ?3)
             ORDER BY created_at DESC, id DESC LIMIT 1",
            params![item, kind.code(), OverdueStatus::Found.code()],
            |row| {
                Ok(Charge {
                    id: row.get(0)?,
                    patron: PatronId(row.get(1)?),
                    amount: row.get(2)?,
                    outstanding: row.get(3)?,
                    loan: row.get(4)?,
                })
            },
        )
        .optional()
}

/// What was paid on a debit: what credits other than write-offs cancelled of it.
fn paid(connection: &Connection, debit: i64) -> rusqlite::Result<i64> {
    connection.query_row(
        "SELECT coalesce(-sum(o.amount), 0) FROM account_offsets o
         JOIN account_lines c ON c.id = o.credit_id
         WHERE o.debit_id = ?1 AND o.kind = 'APPLY' AND c.credit_type <> ?2",
        params![debit, CreditType::Writeoff.code()],
        |row| row.get(0),
    )
}

/// Gives back what the policy says of the charge with a credit applied to it, and marks
/// the charge found; returns the credit, when there was one.
///
/// Koha: `Koha::Item::_set_found_trigger`, `Koha::Account::add_credit`,
/// `Koha::Account::Line::apply`.
fn settle(
    connection: &Connection,
    (item, desk): (i64, Option<BranchId>),
    charge: &Charge,
    (policy, credit_type): (LostReturn, CreditType),
    at: &str,
) -> rusqlite::Result<Option<Money>> {
    let credit = refund(
        policy,
        Money(charge.amount),
        Money(charge.outstanding),
        Money(paid(connection, charge.id)?),
    );
    let mut given = None;
    if credit.0 > 0 {
        let cancelled = credit.0.min(charge.outstanding.max(0));
        connection.execute(
            "INSERT INTO account_lines (patron_id, credit_type, amount, outstanding, item_id,
                                        loan_id, branch_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
            params![
                charge.patron.0,
                credit_type.code(),
                -credit.0,
                cancelled - credit.0,
                item,
                charge.loan,
                desk.map(|branch| branch.0),
                at
            ],
        )?;
        let line = connection.last_insert_rowid();
        connection.execute(
            "INSERT INTO account_offsets (credit_id, kind, amount, created_at)
             VALUES (?1, 'CREATE', ?2, ?3)",
            params![line, -credit.0, at],
        )?;
        if cancelled > 0 {
            connection.execute(
                "INSERT INTO account_offsets (credit_id, debit_id, kind, amount, created_at)
                 VALUES (?1, ?2, 'APPLY', ?3, ?4)",
                params![line, charge.id, -cancelled, at],
            )?;
        }
        connection.execute(
            "UPDATE account_lines SET outstanding = outstanding - ?2 WHERE id = ?1",
            params![charge.id, cancelled],
        )?;
        given = Some(credit);
    }
    connection.execute(
        "UPDATE account_lines SET status = ?2, updated_at = ?3 WHERE id = ?1",
        params![charge.id, OverdueStatus::Found.code(), at],
    )?;
    Ok(given)
}

/// The policies of the branch that finds the Item, or of its home branch.
///
/// Koha: `Koha::CirculationRules::get_lostreturn_policy` with `RefundLostOnReturnControl`
/// `CheckinLibrary`.
fn policies(rules: &[CirculationRule], branch: BranchId) -> (LostReturn, LostReturn) {
    let scope = RuleScopeKey {
        branch: Some(branch),
        category: None,
        policy: None,
    };
    let policy = |kind| {
        LostReturn::from_rule(effective_rule(rules, kind, &scope).map(|rule| rule.value.as_str()))
    };
    (
        policy(RuleKind::Lostreturn),
        policy(RuleKind::Processingreturn),
    )
}

/// The Item as the finding needs it.
struct FoundItem {
    home: BranchId,
    holding: BranchId,
    policy: PolicyId,
    replacement_price: Option<i64>,
}

/// [`found`], with the circulation rules read first.
pub(crate) fn found_by_rules(
    connection: &Connection,
    item: i64,
    place: FoundAt,
    at: &Zoned,
) -> rusqlite::Result<LostFound> {
    let rules = crate::circulation_rules::read_rules(connection)?;
    found(connection, &rules, item, place, at)
}

/// Runs the steps of a finding for the Item, which was lost and is not any more.
pub(crate) fn found(
    connection: &Connection,
    rules: &[CirculationRule],
    item: i64,
    place: FoundAt,
    at: &Zoned,
) -> rusqlite::Result<LostFound> {
    let found_item = connection.query_row(
        "SELECT home_branch, holding_branch, policy_id, replacement_price_cents
         FROM items WHERE id = ?1",
        [item],
        |row| {
            Ok(FoundItem {
                home: BranchId(row.get(0)?),
                holding: BranchId(row.get(1)?),
                policy: PolicyId(row.get(2)?),
                replacement_price: row.get(3)?,
            })
        },
    )?;
    let (lost_policy, processing_policy) = policies(rules, place.desk.unwrap_or(found_item.home));
    let when = at.to_string();
    let mut done = LostFound::default();
    let lost_charge = if lost_policy.refunds() {
        let charge = newest_charge(connection, item, DebitType::Lost)?;
        if let Some(charge) = &charge {
            done.lost_refunded = settle(
                connection,
                (item, place.desk),
                charge,
                (lost_policy, CreditType::LostFound),
                &when,
            )?;
        }
        charge
    } else {
        None
    };
    // The fine the loss stopped, newest first.
    let lost_fine: Option<(i64, PatronId, Option<i64>)> = connection
        .query_row(
            "SELECT id, patron_id, loan_id FROM account_lines
             WHERE item_id = ?1 AND debit_type = ?2 AND status = ?3
             ORDER BY created_at DESC, id DESC LIMIT 1",
            params![item, DebitType::Overdue.code(), OverdueStatus::Lost.code()],
            |row| Ok((row.get(0)?, PatronId(row.get(1)?), row.get(2)?)),
        )
        .optional()?;
    match (lost_policy, &lost_fine) {
        (LostReturn::Restore, Some((fine, _, _))) => {
            connection.execute(
                "UPDATE account_lines SET status = ?2, updated_at = ?3 WHERE id = ?1",
                params![fine, OverdueStatus::Found.code(), when],
            )?;
            done.restored = true;
        }
        (LostReturn::Charge, _) if place.checkin => {
            let from = lost_fine
                .map(|(_, patron, loan)| (patron, loan))
                .or_else(|| {
                    lost_charge
                        .as_ref()
                        .map(|charge| (charge.patron, charge.loan))
                });
            if let Some((patron, Some(loan))) = from {
                done.charged =
                    charge_last_loan(connection, rules, item, &found_item, (patron, loan), at)?;
            }
        }
        _ => {}
    }
    if processing_policy.refunds()
        && let Some(charge) = newest_charge(connection, item, DebitType::Processing)?
    {
        done.processing_refunded = settle(
            connection,
            (item, place.desk),
            &charge,
            (processing_policy, CreditType::ProcessingFound),
            &when,
        )?;
    }
    Ok(done)
}

/// Charges the fine of the Item's last loan up to `at`, when the loss came from that loan,
/// and stops it as returned (item 13).
///
/// Koha: `C4::Circulation::AddReturn` on the message `lost_charge`, with
/// `_CalculateAndUpdateFine` and `_FixOverduesOnReturn`.
fn charge_last_loan(
    connection: &Connection,
    rules: &[CirculationRule],
    item: i64,
    found_item: &FoundItem,
    (patron, loan): (PatronId, i64),
    at: &Zoned,
) -> rusqlite::Result<Option<Money>> {
    let last: Option<(Option<i64>, String)> = connection
        .query_row(
            "SELECT loan_id, due FROM returned_loans WHERE item_id = ?1
             ORDER BY returned_at DESC, id DESC LIMIT 1",
            [item],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((Some(last_loan), due)) = last else {
        return Ok(None);
    };
    if last_loan != loan {
        return Ok(None);
    }
    let due: Zoned = due.parse().map_err(|error: jiff::Error| {
        rusqlite::Error::FromSqlConversionFailure(1, Type::Text, error.into())
    })?;
    let late = LateLoan {
        loan,
        patron,
        item,
        holding: found_item.holding,
        policy: found_item.policy,
        replacement_price: found_item.replacement_price,
        due,
    };
    let fine = update_overdue_fine(connection, rules, &late, at, FineMoment::Return)?;
    close_overdue(
        connection,
        patron,
        item,
        OverdueStatus::Returned,
        &at.to_string(),
    )?;
    Ok(fine)
}
