// SPDX-License-Identifier: GPL-3.0-or-later
//! `circ.checkin`: taking an Item back (docs/specs/circulacao/devolucao.md).

use elzevir_app::circulation_rules::{CirculationOutput, CirculationRefusal, RuleKind};
use elzevir_app::commands::{WriteOutput, WriteRefusal};
use elzevir_app::loans::{CheckedIn, CheckinRefusal, CheckinRequest, checked_barcode};
use elzevir_domain::account::OverdueStatus;
use elzevir_domain::catalog::{BranchId, ItemId, Money, PolicyId};
use elzevir_domain::checkin::{
    CheckinFacts, CheckinSettings, Destination, LoanOutcome, LostOutcome, OpenTransfer,
    ReturnPolicy, TransferReason, checkin as decide,
};
use elzevir_domain::circulation_rules::CirculationRule;
use elzevir_domain::circulation_rules::{RuleScopeKey, effective_rule};
use elzevir_domain::fine::FineSettings;
use elzevir_domain::hold::HoldItem;
use elzevir_domain::patron::PatronId;
use elzevir_domain::suspension::{LateReturn, SuspensionSettings, suspension_until};
use jiff::Zoned;
use jiff::civil::Date;
use rusqlite::{Connection, OptionalExtension, params};

use crate::accounts::close_overdue;
use crate::calendar::read_calendar;
use crate::circulation_rules::read_rules;
use crate::holds::{CheckinItem, hold_at_checkin, revert_waiting};
use crate::loans::find_patron;
use crate::overdue_fines::{FineMoment, LateLoan, update_overdue_fine};
use crate::restrictions::{suspend, suspension};
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
    replacement_price: Option<i64>,
    instance: i64,
    not_for_loan: i64,
}

struct Loan {
    id: i64,
    patron: PatronId,
    due: Zoned,
}

fn find_item(connection: &Connection, barcode: &str) -> rusqlite::Result<Option<Item>> {
    connection
        .query_row(
            "SELECT id, home_branch, holding_branch, policy_id, withdrawn, lost,
                    replacement_price_cents, instance_id, not_for_loan
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
                    replacement_price: row.get(6)?,
                    instance: row.get(7)?,
                    not_for_loan: row.get(8)?,
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
fn return_policy(rules: &[CirculationRule], item: &Item) -> ReturnPolicy {
    let at = RuleScopeKey {
        branch: Some(item.home),
        category: None,
        policy: Some(item.policy),
    };
    let rule = effective_rule(rules, RuleKind::Returnbranch, &at);
    ReturnPolicy::from_rule(rule.map(|rule| rule.value.as_str()))
}

/// Suspends the patron of a late return, and returns when their suspension ends.
///
/// The control branch is the desk's: the Item is already there when Koha suspends.
///
/// Koha: `C4::Circulation::_debar_user_on_return`.
fn suspend_late(
    connection: &Connection,
    rules: &[CirculationRule],
    item: &Item,
    loan: &Loan,
    request: &CheckinRequest,
) -> rusqlite::Result<Option<Date>> {
    let Some((category, _)) = find_patron(connection, loan.patron)? else {
        return Ok(None);
    };
    let calendar = read_calendar(connection, request.desk.0)?;
    let returned = request.at.date();
    let current = suspension(connection, loan.patron)?
        .filter(|current| current.holds_on(returned))
        .and_then(|current| current.expires);
    let late = LateReturn {
        rules,
        at: RuleScopeKey {
            branch: Some(request.desk),
            category: Some(category),
            policy: Some(item.policy),
        },
        due: &loan.due,
        returned: &request.at,
        calendar: &calendar,
        suspended_until: current,
    };
    match suspension_until(&late, &SuspensionSettings::default()) {
        Some(until) => suspend(connection, loan.patron, until, &request.at.to_string()).map(Some),
        None => Ok(None),
    }
}

/// Charges the fine of a late return and stops it growing, and returns the fine. A lost
/// Item is not fined.
///
/// Koha: `C4::Circulation::_CalculateAndUpdateFine`, `C4::Circulation::_FixOverduesOnReturn`.
fn fine_late(
    connection: &Connection,
    rules: &[CirculationRule],
    item: &Item,
    loan: &Loan,
    request: &CheckinRequest,
) -> rusqlite::Result<Option<Money>> {
    let mut fine = None;
    if FineSettings::default().on_return && item.lost == 0 {
        let late = LateLoan {
            loan: loan.id,
            patron: loan.patron,
            item: item.id,
            holding: item.holding,
            policy: item.policy,
            replacement_price: item.replacement_price,
            due: loan.due.clone(),
        };
        fine = update_overdue_fine(connection, rules, &late, &request.at, FineMoment::Return)?;
    }
    close_overdue(
        connection,
        loan.patron,
        item.id,
        OverdueStatus::Returned,
        &request.at.to_string(),
    )?;
    Ok(fine)
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
    close_loan(connection, loan.id, &request.at.to_string(), request.desk)
}

/// Moves a loan to the returned loans, returned at `at` in `branch`, without its patron
/// unless the patron keeps a reading history.
///
/// Koha: `C4::Circulation::MarkIssueReturned` (`privacy` 2 anonymizes at once).
pub(crate) fn close_loan(
    connection: &Connection,
    loan: i64,
    at: &str,
    branch: BranchId,
) -> rusqlite::Result<()> {
    connection.execute(
        "INSERT INTO returned_loans (item_id, patron_id, branch_id, issued_at, due, on_site,
                                     returned_at, return_branch_id, renewals, category_id)
         SELECT l.item_id,
                CASE WHEN p.reading_history = 1 THEN l.patron_id END,
                l.branch_id, l.issued_at, l.due, l.on_site, ?2, ?3, l.renewals, p.category_id
         FROM loans l JOIN patrons p ON p.id = l.patron_id
         WHERE l.id = ?1",
        params![loan, at, branch.0],
    )?;
    connection.execute("DELETE FROM loans WHERE id = ?1", [loan])?;
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
    let rules = read_rules(connection).map_err(storage)?;
    // Koha looks for the hold once the Item is at the desk's branch.
    let hold = if item.withdrawn == 0 {
        let at_desk = CheckinItem {
            item: HoldItem {
                id: ItemId(item.id),
                home: item.home,
                holding: request.desk,
                policy: item.policy,
            },
            instance: item.instance,
            not_for_loan: item.not_for_loan,
        };
        hold_at_checkin(connection, &rules, &at_desk).map_err(storage)?
    } else {
        None
    };
    let facts = CheckinFacts {
        desk: request.desk,
        home: item.home,
        holding: item.holding,
        withdrawn: item.withdrawn,
        lost: item.lost,
        due: loan.as_ref().map(|loan| loan.due.clone()),
        transfer: transfer.map(|(_, transfer)| transfer),
        policy: return_policy(&rules, &item),
        held: hold.is_some(),
        at: request.at.clone(),
    };
    let done = decide(&facts, &CheckinSettings::default())
        .map_err(|_| refused(CheckinRefusal::Withdrawn))?;
    let mut patron = None;
    let mut suspended_until = None;
    let mut fine = None;
    if let (LoanOutcome::Ended { overdue }, Some(loan)) = (done.loan, &loan) {
        end_loan(connection, loan, request).map_err(storage)?;
        patron = Some(loan.patron);
        fine = fine_late(connection, &rules, &item, loan, request).map_err(storage)?;
        if overdue {
            suspended_until =
                suspend_late(connection, &rules, &item, loan, request).map_err(storage)?;
        }
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
    let hold = hold
        .map(|found| revert_waiting(connection, found, request.desk, item.instance))
        .transpose()
        .map_err(storage)?;
    Ok(WriteOutput::Circulation(CirculationOutput::CheckedIn(
        CheckedIn {
            patron,
            loan: done.loan,
            lost: done.lost,
            destination: done.destination,
            suspended_until,
            fine,
            hold,
        },
    )))
}

/// Keeps the day of the patron's latest checkout, from the moment it was issued
/// (docs/specs/operacao/relatorios.md, item 9).
pub(crate) fn note_checkout(
    connection: &Connection,
    patron: i64,
    issued_at: &str,
) -> rusqlite::Result<()> {
    connection.execute(
        "UPDATE patrons SET last_issued_on = substr(?2, 1, 10)
         WHERE id = ?1 AND (last_issued_on IS NULL OR last_issued_on < substr(?2, 1, 10))",
        params![patron, issued_at],
    )?;
    Ok(())
}
