// SPDX-License-Identifier: GPL-3.0-or-later
//! `circ.checkin`: taking an Item back (docs/specs/circulacao/devolucao.md).

use elzevir_app::circulation_rules::{CirculationOutput, CirculationRefusal, RuleKind};
use elzevir_app::commands::{WriteOutput, WriteRefusal};
use elzevir_app::loans::{CheckedIn, CheckinRefusal, CheckinRequest, checked_barcode};
use elzevir_domain::catalog::{BranchId, PolicyId};
use elzevir_domain::checkin::{
    CheckinFacts, CheckinSettings, Destination, LoanOutcome, LostOutcome, OpenTransfer,
    ReturnPolicy, TransferReason, checkin as decide,
};
use elzevir_domain::circulation_rules::{RuleScopeKey, effective_rule};
use elzevir_domain::patron::PatronId;
use jiff::Zoned;
use rusqlite::{Connection, OptionalExtension, params};

use crate::circulation_rules::read_rules;
use crate::{CommandFailed, StoreError};

type Applied = Result<WriteOutput, CommandFailed<WriteRefusal>>;

fn refused(refusal: CheckinRefusal) -> CommandFailed<WriteRefusal> {
    CommandFailed::Rejected(WriteRefusal::Circulation(CirculationRefusal::Checkin(
        refusal,
    )))
}

fn storage(error: rusqlite::Error) -> CommandFailed<WriteRefusal> {
    StoreError::from(error).into()
}

struct Item {
    id: i64,
    home: BranchId,
    holding: BranchId,
    policy: PolicyId,
    withdrawn: i64,
    lost: i64,
}

struct Loan {
    id: i64,
    patron: PatronId,
    due: Zoned,
}

fn find_item(connection: &Connection, barcode: &str) -> rusqlite::Result<Option<Item>> {
    connection
        .query_row(
            "SELECT id, home_branch, holding_branch, policy_id, withdrawn, lost
             FROM items WHERE barcode = ?1",
            [barcode],
            |row| {
                Ok(Item {
                    id: row.get(0)?,
                    home: BranchId(row.get(1)?),
                    holding: BranchId(row.get(2)?),
                    policy: PolicyId(row.get(3)?),
                    withdrawn: row.get(4)?,
                    lost: row.get(5)?,
                })
            },
        )
        .optional()
}

fn find_loan(connection: &Connection, item: i64) -> rusqlite::Result<Option<Loan>> {
    connection
        .query_row(
            "SELECT id, patron_id, due FROM loans WHERE item_id = ?1",
            [item],
            |row| {
                let due: String = row.get(2)?;
                let due = due.parse().map_err(|error: jiff::Error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        2,
                        rusqlite::types::Type::Text,
                        error.into(),
                    )
                })?;
                Ok(Loan {
                    id: row.get(0)?,
                    patron: PatronId(row.get(1)?),
                    due,
                })
            },
        )
        .optional()
}

fn open_transfer(
    connection: &Connection,
    item: i64,
) -> rusqlite::Result<Option<(i64, OpenTransfer)>> {
    connection
        .query_row(
            "SELECT id, from_branch, to_branch FROM transfers
             WHERE item_id = ?1 AND arrived_at IS NULL",
            [item],
            |row| {
                Ok((
                    row.get(0)?,
                    OpenTransfer {
                        from: BranchId(row.get(1)?),
                        to: BranchId(row.get(2)?),
                    },
                ))
            },
        )
        .optional()
}

/// The `returnbranch` rule of the Item's home branch and policy.
///
/// Koha: `Koha::CirculationRules::get_return_branch_policy`, with `CircControlReturnsBranch`
/// `ItemHomeLibrary` (the default).
fn return_policy(connection: &Connection, item: &Item) -> rusqlite::Result<ReturnPolicy> {
    let rules = read_rules(connection)?;
    let at = RuleScopeKey {
        branch: Some(item.home),
        category: None,
        policy: Some(item.policy),
    };
    let rule = effective_rule(&rules, RuleKind::Returnbranch, &at);
    Ok(ReturnPolicy::from_rule(
        rule.map(|rule| rule.value.as_str()),
    ))
}

/// Moves the loan to the returned loans, without its patron unless the patron keeps a
/// reading history.
///
/// Koha: `C4::Circulation::MarkIssueReturned` (`privacy` 2 anonymizes at once).
fn end_loan(
    connection: &Connection,
    loan: &Loan,
    request: &CheckinRequest,
) -> rusqlite::Result<()> {
    connection.execute(
        "INSERT INTO returned_loans (item_id, patron_id, branch_id, issued_at, due, on_site,
                                     returned_at, return_branch_id)
         SELECT l.item_id,
                CASE WHEN p.reading_history = 1 THEN l.patron_id END,
                l.branch_id, l.issued_at, l.due, l.on_site, ?2, ?3
         FROM loans l JOIN patrons p ON p.id = l.patron_id
         WHERE l.id = ?1",
        params![loan.id, request.at.to_string(), request.desk.0],
    )?;
    connection.execute("DELETE FROM loans WHERE id = ?1", [loan.id])?;
    Ok(())
}

fn reason_name(reason: TransferReason) -> &'static str {
    match reason {
        TransferReason::ReturnToHome => "ReturnToHome",
        TransferReason::ReturnToHolding => "ReturnToHolding",
    }
}

/// Applies `circ.checkin` inside the writer's transaction.
///
/// Koha: `C4::Circulation::AddReturn`.
pub(crate) fn checkin(connection: &Connection, request: &CheckinRequest) -> Applied {
    let barcode =
        checked_barcode(&request.barcode).ok_or_else(|| refused(CheckinRefusal::UnknownBarcode))?;
    let item = find_item(connection, barcode)
        .map_err(storage)?
        .ok_or_else(|| refused(CheckinRefusal::UnknownBarcode))?;
    let loan = find_loan(connection, item.id).map_err(storage)?;
    let transfer = open_transfer(connection, item.id).map_err(storage)?;
    let facts = CheckinFacts {
        desk: request.desk,
        home: item.home,
        holding: item.holding,
        withdrawn: item.withdrawn,
        lost: item.lost,
        due: loan.as_ref().map(|loan| loan.due.clone()),
        transfer: transfer.map(|(_, transfer)| transfer),
        policy: return_policy(connection, &item).map_err(storage)?,
        at: request.at.clone(),
    };
    let done = decide(&facts, &CheckinSettings::default())
        .map_err(|_| refused(CheckinRefusal::Withdrawn))?;
    let mut patron = None;
    if let (LoanOutcome::Ended { .. }, Some(loan)) = (done.loan, &loan) {
        end_loan(connection, loan, request).map_err(storage)?;
        patron = Some(loan.patron);
    }
    let found = done.lost == LostOutcome::Found;
    connection
        .execute(
            "UPDATE items SET holding_branch = ?2, lost = CASE WHEN ?3 THEN 0 ELSE lost END,
                              row_version = row_version + 1
             WHERE id = ?1",
            params![item.id, request.desk.0, found],
        )
        .map_err(storage)?;
    match (done.destination, transfer) {
        (Destination::Arrived { .. }, Some((id, _))) => {
            connection
                .execute(
                    "UPDATE transfers SET arrived_at = ?2 WHERE id = ?1",
                    params![id, request.at.to_string()],
                )
                .map_err(storage)?;
        }
        (Destination::Sent { to, reason }, _) => {
            connection
                .execute(
                    "INSERT INTO transfers (item_id, from_branch, to_branch, reason, sent_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        item.id,
                        request.desk.0,
                        to.0,
                        reason_name(reason),
                        request.at.to_string()
                    ],
                )
                .map_err(storage)?;
        }
        _ => {}
    }
    Ok(WriteOutput::Circulation(CirculationOutput::CheckedIn(
        CheckedIn {
            patron,
            loan: done.loan,
            lost: done.lost,
            destination: done.destination,
        },
    )))
}
