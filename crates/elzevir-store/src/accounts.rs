// SPDX-License-Identifier: GPL-3.0-or-later
//! `account_lines` and `account_offsets`: what a patron owes (docs/specs/circulacao/conta.md).

use elzevir_domain::account::{
    DebitType, FineUpdate, OverdueStatus, adjusted_outstanding, update_fine,
};
use elzevir_domain::catalog::{BranchId, Money};
use elzevir_domain::patron::PatronId;
use rusqlite::{Connection, OptionalExtension, params};

/// What the patron owes: the outstanding debits. Credits not yet applied do not count.
///
/// Koha: `Koha::Account::non_issues_charges` (every debit type restricts checkouts by
/// default).
pub(crate) fn owed(connection: &Connection, patron: PatronId) -> rusqlite::Result<Money> {
    connection
        .query_row(
            "SELECT coalesce(sum(outstanding), 0) FROM account_lines
             WHERE patron_id = ?1 AND debit_type IS NOT NULL",
            [patron.0],
            |row| row.get(0),
        )
        .map(Money)
}

/// A loan's overdue fine, while the Item is out.
struct Growing {
    id: i64,
    amount: Money,
    outstanding: Money,
}

fn growing(connection: &Connection, loan: i64) -> rusqlite::Result<Option<Growing>> {
    connection
        .query_row(
            "SELECT id, amount, outstanding FROM account_lines
             WHERE loan_id = ?1 AND debit_type = ?2 AND status = ?3",
            params![
                loan,
                DebitType::Overdue.code(),
                OverdueStatus::Unreturned.code()
            ],
            |row| {
                Ok(Growing {
                    id: row.get(0)?,
                    amount: Money(row.get(1)?),
                    outstanding: Money(row.get(2)?),
                })
            },
        )
        .optional()
}

fn offset(
    connection: &Connection,
    debit: i64,
    kind: &str,
    amount: i64,
    at: &str,
) -> rusqlite::Result<()> {
    connection.execute(
        "INSERT INTO account_offsets (debit_id, kind, amount, created_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![debit, kind, amount, at],
    )?;
    Ok(())
}

/// The loan whose overdue fine is charged.
pub(crate) struct OverdueLoan {
    /// `loans.id`.
    pub(crate) loan: i64,
    /// Who borrowed.
    pub(crate) patron: PatronId,
    /// The Item.
    pub(crate) item: i64,
    /// The branch the fine belongs to.
    pub(crate) branch: BranchId,
}

/// Charges the loan's overdue fine at `amount`, or brings its growing fine to it, and
/// returns what the fine is now.
///
/// Koha: `C4::Overdues::UpdateFine`, `Koha::Account::add_debit`,
/// `Koha::Account::Line::adjust`.
pub(crate) fn charge_overdue(
    connection: &Connection,
    loan: &OverdueLoan,
    amount: Money,
    max_fine: Option<Money>,
    at: &str,
) -> rusqlite::Result<Option<Money>> {
    let current = growing(connection, loan.loan)?;
    let owed_in_fines: i64 = connection.query_row(
        "SELECT coalesce(sum(outstanding), 0) FROM account_lines
         WHERE patron_id = ?1 AND debit_type = ?2",
        params![loan.patron.0, DebitType::Overdue.code()],
        |row| row.get(0),
    )?;
    let update = update_fine(
        amount,
        current.as_ref().map(|line| line.amount),
        Money(owed_in_fines),
        max_fine,
    );
    match (update, current) {
        (FineUpdate::Charge(Money(amount)), _) => {
            connection.execute(
                "INSERT INTO account_lines (patron_id, debit_type, status, amount, outstanding,
                                            item_id, loan_id, branch_id, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?4, ?5, ?6, ?7, ?8, ?8)",
                params![
                    loan.patron.0,
                    DebitType::Overdue.code(),
                    OverdueStatus::Unreturned.code(),
                    amount,
                    loan.item,
                    loan.loan,
                    loan.branch.0,
                    at
                ],
            )?;
            offset(
                connection,
                connection.last_insert_rowid(),
                "CREATE",
                amount,
                at,
            )?;
            Ok(Some(Money(amount)))
        }
        (FineUpdate::Adjust(after), Some(line)) => {
            let outstanding = adjusted_outstanding(line.amount, line.outstanding, after);
            connection.execute(
                "UPDATE account_lines SET amount = ?2, outstanding = ?3, updated_at = ?4
                 WHERE id = ?1",
                params![line.id, after.0, outstanding.0, at],
            )?;
            let difference = after.0 - line.amount.0;
            let kind = if difference > 0 {
                "OVERDUE_INCREASE"
            } else {
                "OVERDUE_DECREASE"
            };
            offset(connection, line.id, kind, difference, at)?;
            Ok(Some(after))
        }
        (_, current) => Ok(current.map(|line| line.amount)),
    }
}

/// Stops the growing fine of the patron's Item: it is `RETURNED` now, or gone when it was
/// zero and nothing was paid on it.
///
/// Koha: `C4::Circulation::_FixOverduesOnReturn` (without forgiving the fine).
pub(crate) fn close_overdue(
    connection: &Connection,
    patron: PatronId,
    item: i64,
    at: &str,
) -> rusqlite::Result<()> {
    let line: Option<(i64, i64)> = connection
        .query_row(
            "SELECT id, amount FROM account_lines
             WHERE patron_id = ?1 AND item_id = ?2 AND debit_type = ?3 AND status = ?4
             ORDER BY id LIMIT 1",
            params![
                patron.0,
                item,
                DebitType::Overdue.code(),
                OverdueStatus::Unreturned.code()
            ],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((id, amount)) = line else {
        return Ok(());
    };
    let paid: bool = connection.query_row(
        "SELECT EXISTS (SELECT 1 FROM account_offsets
                        WHERE debit_id = ?1 AND credit_id IS NOT NULL)",
        [id],
        |row| row.get(0),
    )?;
    if amount == 0 && !paid {
        connection.execute("DELETE FROM account_offsets WHERE debit_id = ?1", [id])?;
        connection.execute("DELETE FROM account_lines WHERE id = ?1", [id])?;
    } else {
        connection.execute(
            "UPDATE account_lines SET status = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, OverdueStatus::Returned.code(), at],
        )?;
    }
    Ok(())
}
