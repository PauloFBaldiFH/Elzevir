// SPDX-License-Identifier: GPL-3.0-or-later
//! `patrons.migrate_koha_accounts`: one write of Koha's patron accounts, each whole
//! (docs/specs/migracao/circulacao-do-koha.md, items 9 to 13).

use std::collections::BTreeMap;

use elzevir_app::accounts::LineKind;
use elzevir_app::commands::WriteRefusal;
use elzevir_app::koha_migration::{
    KohaAccount, KohaAccountBatch, KohaAccountLine, KohaAccountsMigrated,
};
use elzevir_app::patrons::PatronOutput;
use rusqlite::{Connection, OptionalExtension, params};

use crate::koha_circulation::{item, patron};
use crate::places::{Coded, by_code};
use crate::{CommandFailed, StoreError};

/// Applies one write: each account with its lines and offsets.
pub(crate) fn apply(
    connection: &Connection,
    batch: &KohaAccountBatch,
) -> Result<PatronOutput, CommandFailed<WriteRefusal>> {
    let mut done = KohaAccountsMigrated::default();
    for account in &batch.accounts {
        add_account(connection, account, &mut done)?;
    }
    Ok(PatronOutput::KohaAccountsMigrated(done))
}

/// Item 11: the account comes whole, unless the patron did not come or already has one.
fn add_account(
    connection: &Connection,
    account: &KohaAccount,
    done: &mut KohaAccountsMigrated,
) -> Result<(), StoreError> {
    let Ok(patron) = patron(connection, account.borrowernumber)? else {
        done.accounts_left.push(account.borrowernumber);
        return Ok(());
    };
    let has_lines = connection
        .query_row(
            "SELECT 1 FROM account_lines WHERE patron_id = ?1 LIMIT 1",
            [patron],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if has_lines {
        done.already_migrated += 1;
        return Ok(());
    }
    let mut ids = BTreeMap::new();
    for line in &account.lines {
        ids.insert(line.accountlines_id, add_line(connection, patron, line)?);
    }
    for offset in &account.offsets {
        connection.execute(
            "INSERT INTO account_offsets (credit_id, debit_id, kind, amount, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                offset.credit.and_then(|id| ids.get(&id)),
                offset.debit.and_then(|id| ids.get(&id)),
                offset.kind,
                offset.amount.0,
                offset.created_at
            ],
        )?;
    }
    done.accounts += 1;
    done.lines += account.lines.len();
    done.offsets += account.offsets.len();
    Ok(())
}

/// Item 10: the line as Koha had it; the Item, loan and branch when they are here.
fn add_line(
    connection: &Connection,
    patron: i64,
    line: &KohaAccountLine,
) -> Result<i64, StoreError> {
    let item_id = match &line.item {
        Some(koha) => item(connection, koha)?.ok(),
        None => None,
    };
    let loan_id: Option<i64> = match &line.loan {
        Some(koha) => match item(connection, koha)? {
            Ok(lent) => connection
                .query_row(
                    "SELECT id FROM loans WHERE item_id = ?1 AND patron_id = ?2",
                    params![lent, patron],
                    |row| row.get(0),
                )
                .optional()?,
            Err(_) => None,
        },
        None => None,
    };
    let branch_id = match &line.branch {
        Some(code) => by_code(connection, Coded::Branch, code)?,
        None => None,
    };
    let (debit, credit) = match line.kind {
        LineKind::Debit(debit) => (Some(debit.code()), None),
        LineKind::Credit(credit) => (None, Some(credit.code())),
    };
    connection.execute(
        "INSERT INTO account_lines (patron_id, debit_type, credit_type, status, amount,
                                    outstanding, item_id, loan_id, branch_id, created_at,
                                    updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            patron,
            debit,
            credit,
            line.status,
            line.amount.0,
            line.outstanding.0,
            item_id,
            loan_id,
            branch_id,
            line.created_at,
            line.updated_at
        ],
    )?;
    Ok(connection.last_insert_rowid())
}
