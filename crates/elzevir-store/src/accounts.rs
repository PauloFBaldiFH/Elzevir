// SPDX-License-Identifier: GPL-3.0-or-later
//! `account_lines` and `account_offsets`: what a patron owes (docs/specs/circulacao/conta.md).

use elzevir_app::accounts::{
    AccountLine, AccountLineId, LineKind, MAX_LINES_PER_PAYMENT, Paid, PayRefusal, PaymentRequest,
};
use elzevir_app::circulation_rules::{CirculationOutput, CirculationRefusal};
use elzevir_app::commands::{WriteOutput, WriteRefusal};
use elzevir_domain::account::{
    CreditType, DebitType, FineUpdate, OverdueStatus, adjusted_outstanding, apply_credit,
    check_payment, update_fine,
};
use elzevir_domain::catalog::{BranchId, Money};
use elzevir_domain::patron::PatronId;
use rusqlite::types::Type;
use rusqlite::{Connection, OptionalExtension, params};

use crate::catalog::exists;
use crate::{CommandFailed, StoreError};

type Applied = Result<WriteOutput, CommandFailed<WriteRefusal>>;

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

/// Adds a debit about the loan, with its `CREATE` movement.
///
/// Koha: `Koha::Account::add_debit`.
pub(crate) fn add_debit(
    connection: &Connection,
    loan: &OverdueLoan,
    kind: DebitType,
    status: Option<OverdueStatus>,
    amount: Money,
    at: &str,
) -> rusqlite::Result<()> {
    connection.execute(
        "INSERT INTO account_lines (patron_id, debit_type, status, amount, outstanding,
                                    item_id, loan_id, branch_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?4, ?5, ?6, ?7, ?8, ?8)",
        params![
            loan.patron.0,
            kind.code(),
            status.map(OverdueStatus::code),
            amount.0,
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
        amount.0,
        at,
    )
}

/// The loan a debit is about.
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
        (FineUpdate::Charge(amount), _) => {
            add_debit(
                connection,
                loan,
                DebitType::Overdue,
                Some(OverdueStatus::Unreturned),
                amount,
                at,
            )?;
            Ok(Some(amount))
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

/// Stops the growing fine of the patron's Item: it gets `status` (`RETURNED` or `LOST`),
/// or is gone when it was zero and nothing was paid on it.
///
/// Koha: `C4::Circulation::_FixOverduesOnReturn` (without forgiving the fine).
pub(crate) fn close_overdue(
    connection: &Connection,
    patron: PatronId,
    item: i64,
    status: OverdueStatus,
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
            params![id, status.code(), at],
        )?;
    }
    Ok(())
}

/// The patron's debits that still owe, oldest first: the given ids, or all of them.
/// `None` when an id is not one of them.
fn owing(
    connection: &Connection,
    patron: PatronId,
    only: &[AccountLineId],
) -> rusqlite::Result<Option<Vec<(i64, Money)>>> {
    let mut statement = connection.prepare_cached(
        "SELECT id, outstanding FROM account_lines
         WHERE patron_id = ?1 AND debit_type IS NOT NULL AND amount > 0 AND outstanding > 0
         ORDER BY created_at, id",
    )?;
    let all = statement
        .query_map([patron.0], |row| Ok((row.get(0)?, Money(row.get(1)?))))?
        .collect::<rusqlite::Result<Vec<(i64, Money)>>>()?;
    if only.is_empty() {
        return Ok(Some(all));
    }
    let chosen: Vec<(i64, Money)> = all
        .into_iter()
        .filter(|(id, _)| only.contains(&AccountLineId(*id)))
        .collect();
    let mut distinct = only.to_vec();
    distinct.sort_unstable();
    distinct.dedup();
    Ok((chosen.len() == distinct.len()).then_some(chosen))
}

/// Applies `circ.pay`: a credit for the amount, applied to the debits chosen and then to
/// any other debit, oldest first, while it lasts.
///
/// Koha: `Koha::Account::pay`, `Koha::Account::add_credit`, `Koha::Account::Line::apply`.
pub(crate) fn pay(connection: &Connection, request: &PaymentRequest) -> Applied {
    let refused = |refusal: PayRefusal| {
        CommandFailed::Rejected(WriteRefusal::Circulation(CirculationRefusal::Pay(refusal)))
    };
    let storage = |error: rusqlite::Error| CommandFailed::from(StoreError::from(error));
    if !exists(
        connection,
        "SELECT 1 FROM patrons WHERE id = ?1",
        request.patron.0,
    )? {
        return Err(refused(PayRefusal::UnknownPatron));
    }
    if request.lines.len() > MAX_LINES_PER_PAYMENT {
        return Err(refused(PayRefusal::UnknownLine));
    }
    let chosen = owing(connection, request.patron, &request.lines)
        .map_err(storage)?
        .ok_or_else(|| refused(PayRefusal::UnknownLine))?;
    let due = Money(chosen.iter().map(|(_, owed)| owed.0).sum());
    check_payment(request.amount, due).map_err(|error| refused(error.into()))?;
    let at = request.at.to_string();
    connection
        .execute(
            "INSERT INTO account_lines (patron_id, credit_type, amount, outstanding, branch_id,
                                        created_at, updated_at)
             VALUES (?1, ?2, ?3, ?3, ?4, ?5, ?5)",
            params![
                request.patron.0,
                request.kind.credit().code(),
                -request.amount.0,
                request.desk.0,
                at
            ],
        )
        .map_err(storage)?;
    let credit = connection.last_insert_rowid();
    connection
        .execute(
            "INSERT INTO account_offsets (credit_id, kind, amount, created_at)
             VALUES (?1, 'CREATE', ?2, ?3)",
            params![credit, -request.amount.0, at],
        )
        .map_err(storage)?;
    let (mut applied, left) = apply_credit(request.amount, &chosen);
    // Koha's `pay` goes on to every other debit with what is left.
    if left.0 > 0 {
        let others = owing(connection, request.patron, &[])
            .map_err(storage)?
            .unwrap_or_default()
            .into_iter()
            .filter(|(id, _)| !applied.iter().any(|(done, _)| done == id))
            .collect::<Vec<_>>();
        applied.extend(apply_credit(left, &others).0);
    }
    let used: i64 = applied.iter().map(|(_, cancelled)| cancelled.0).sum();
    for (debit, Money(cancelled)) in &applied {
        connection
            .execute(
                "UPDATE account_lines SET outstanding = outstanding - ?2, updated_at = ?3
                 WHERE id = ?1",
                params![debit, cancelled, at],
            )
            .map_err(storage)?;
        connection
            .execute(
                "INSERT INTO account_offsets (credit_id, debit_id, kind, amount, created_at)
                 VALUES (?1, ?2, 'APPLY', ?3, ?4)",
                params![credit, debit, -cancelled, at],
            )
            .map_err(storage)?;
    }
    connection
        .execute(
            "UPDATE account_lines SET outstanding = ?2 WHERE id = ?1",
            params![credit, used - request.amount.0],
        )
        .map_err(storage)?;
    Ok(WriteOutput::Circulation(CirculationOutput::Paid(Paid {
        credit: AccountLineId(credit),
        owed: owed(connection, request.patron).map_err(storage)?,
    })))
}

/// The patron's account, newest first.
pub(crate) fn lines(
    connection: &Connection,
    patron: PatronId,
) -> rusqlite::Result<Vec<AccountLine>> {
    let mut statement = connection.prepare_cached(
        "SELECT a.id, a.debit_type, a.credit_type, a.created_at, a.amount, a.outstanding,
                i.barcode
         FROM account_lines a LEFT JOIN items i ON i.id = a.item_id
         WHERE a.patron_id = ?1
         ORDER BY a.created_at DESC, a.id DESC",
    )?;
    let rows = statement.query_map([patron.0], |row| {
        let debit: Option<String> = row.get(1)?;
        let credit: Option<String> = row.get(2)?;
        let created: String = row.get(3)?;
        Ok((
            row.get::<_, i64>(0)?,
            debit,
            credit,
            created,
            (row.get::<_, i64>(4)?, row.get::<_, i64>(5)?),
            row.get::<_, Option<String>>(6)?,
        ))
    })?;
    let mut lines = Vec::new();
    for row in rows {
        let (id, debit, credit, created, (amount, outstanding), barcode) = row?;
        let kind = match (debit.as_deref(), credit.as_deref()) {
            (Some(code), _) => DebitType::from_code(code).map(LineKind::Debit),
            (None, Some(code)) => CreditType::from_code(code).map(LineKind::Credit),
            (None, None) => None,
        };
        // A line of a type this version does not know is left out rather than misnamed.
        let Some(kind) = kind else { continue };
        let day = created
            .parse::<jiff::Zoned>()
            .map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(3, Type::Text, error.into())
            })?
            .date();
        lines.push(AccountLine {
            id: AccountLineId(id),
            kind,
            day,
            amount: Money(amount),
            outstanding: Money(outstanding),
            barcode,
        });
    }
    Ok(lines)
}
