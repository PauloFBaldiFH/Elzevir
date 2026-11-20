// SPDX-License-Identifier: GPL-3.0-or-later
//! `circ.checkout`: lending an Item (docs/specs/circulacao/emprestimo.md).

use elzevir_app::circulation_rules::{CirculationOutput, CirculationRefusal};
use elzevir_app::commands::{WriteOutput, WriteRefusal};
use elzevir_app::loans::{CheckedOut, CheckoutRefusal, CheckoutRequest, LoanId, checked_barcode};
use elzevir_domain::catalog::{BranchId, ItemId, PolicyId};
use elzevir_domain::checkout::{
    CheckoutFacts, CheckoutSettings, CurrentLoan, HeldFor, ItemFacts, LimitQuery, LoanHolder,
    check_checkout, loan_rule, too_many,
};
use elzevir_domain::circulation::checkout_due;
use elzevir_domain::circulation_rules::{CirculationRule, RuleScopeKey};
use elzevir_domain::due_date::{ConsiderLibraryHours, DueDateContext, LibraryHours};
use elzevir_domain::hold::{HoldItem, HoldMatch};
use elzevir_domain::patron::{PatronCategoryId, PatronId};
use jiff::civil::Date;
use rusqlite::{Connection, OptionalExtension, params};

use crate::accounts::owed;
use crate::calendar::read_calendar;
use crate::catalog::exists;
use crate::circulation_rules::read_rules;
use crate::holds::{ItemHold, fill_at_checkout, hold_for_checkout};
use crate::restrictions::{Suspension, suspension};
use crate::{CommandFailed, StoreError};

type Applied = Result<WriteOutput, CommandFailed<WriteRefusal>>;

/// Koha's default `useDaysMode`.
pub(crate) const USE_DAYS_MODE: &str = "Calendar";

fn refused(refusal: CheckoutRefusal) -> CommandFailed<WriteRefusal> {
    CommandFailed::Rejected(WriteRefusal::Circulation(CirculationRefusal::Checkout(
        refusal,
    )))
}

fn storage(error: rusqlite::Error) -> CommandFailed<WriteRefusal> {
    StoreError::from(error).into()
}

struct Item {
    id: i64,
    home_branch: BranchId,
    holding_branch: BranchId,
    policy: PolicyId,
    instance: i64,
    facts: ItemFacts,
}

fn find_item(connection: &Connection, barcode: &str) -> rusqlite::Result<Option<Item>> {
    connection
        .query_row(
            "SELECT id, holding_branch, policy_id, not_for_loan, withdrawn, restricted, lost,
                    home_branch, instance_id
             FROM items WHERE barcode = ?1",
            [barcode],
            |row| {
                Ok(Item {
                    id: row.get(0)?,
                    home_branch: BranchId(row.get(7)?),
                    holding_branch: BranchId(row.get(1)?),
                    policy: PolicyId(row.get(2)?),
                    instance: row.get(8)?,
                    facts: ItemFacts {
                        not_for_loan: row.get(3)?,
                        withdrawn: row.get(4)?,
                        restricted: row.get(5)?,
                        lost: row.get(6)?,
                        on_loan: None,
                    },
                })
            },
        )
        .optional()
}

pub(crate) fn find_patron(
    connection: &Connection,
    patron: PatronId,
) -> rusqlite::Result<Option<(PatronCategoryId, Date)>> {
    connection
        .query_row(
            "SELECT category_id, expires FROM patrons WHERE id = ?1",
            [patron.0],
            |row| {
                let expires: String = row.get(1)?;
                let expires = expires.parse().map_err(|error: jiff::Error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        1,
                        rusqlite::types::Type::Text,
                        error.into(),
                    )
                })?;
                Ok((PatronCategoryId(row.get(0)?), expires))
            },
        )
        .optional()
}

/// The patron's loans as the limits count them: each at its Item's holding branch (Koha's
/// default `CircControl` and `HomeOrHoldingBranch`).
fn current_loans(connection: &Connection, patron: PatronId) -> rusqlite::Result<Vec<CurrentLoan>> {
    let mut statement = connection.prepare_cached(
        "SELECT i.holding_branch, i.policy_id, l.on_site
         FROM loans l JOIN items i ON i.id = l.item_id
         WHERE l.patron_id = ?1",
    )?;
    let rows = statement.query_map([patron.0], |row| {
        Ok(CurrentLoan {
            branch: BranchId(row.get(0)?),
            policy: PolicyId(row.get(1)?),
            on_site: row.get(2)?,
        })
    })?;
    rows.collect()
}

fn holder(
    connection: &Connection,
    item: i64,
    patron: PatronId,
) -> rusqlite::Result<Option<LoanHolder>> {
    let borrower: Option<i64> = connection
        .query_row(
            "SELECT patron_id FROM loans WHERE item_id = ?1",
            [item],
            |row| row.get(0),
        )
        .optional()?;
    Ok(borrower.map(|borrower| {
        if borrower == patron.0 {
            LoanHolder::ThisPatron
        } else {
            LoanHolder::AnotherPatron
        }
    }))
}

/// The Item as the holds see it, and the hold it answers where it is now.
fn item_hold(
    connection: &Connection,
    rules: &[CirculationRule],
    item: &Item,
) -> rusqlite::Result<(HoldItem, Option<ItemHold>)> {
    let hold_item = HoldItem {
        id: ItemId(item.id),
        home: item.home_branch,
        holding: item.holding_branch,
        policy: item.policy,
    };
    let answered = hold_for_checkout(
        connection,
        rules,
        &hold_item,
        item.instance,
        item.facts.not_for_loan,
    )?;
    Ok((hold_item, answered))
}

/// How the Item answers a hold of someone else than `patron`.
fn held_for_another(answered: Option<ItemHold>, patron: PatronId) -> Option<HeldFor> {
    answered
        .filter(|found| found.patron != patron)
        .map(|found| match found.found {
            HoldMatch::Waiting(_) => HeldFor::Waiting,
            HoldMatch::InTransit(_) => HeldFor::InTransit,
            HoldMatch::Reserved(_) => HeldFor::Reserved,
        })
}

/// Stores the loan, and moves the Item to the desk's branch and finds it if it was lost
/// (Koha: `AddIssue`).
fn record_loan(
    connection: &Connection,
    item: i64,
    request: &CheckoutRequest,
    due: &impl std::fmt::Display,
) -> rusqlite::Result<LoanId> {
    connection.execute(
        "INSERT INTO loans (item_id, patron_id, branch_id, issued_at, due, on_site)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            item,
            request.patron.0,
            request.desk.0,
            request.at.to_string(),
            due.to_string(),
            request.on_site
        ],
    )?;
    let loan = LoanId(connection.last_insert_rowid());
    connection.execute(
        "UPDATE items SET holding_branch = ?2, lost = 0, row_version = row_version + 1
         WHERE id = ?1",
        params![item, request.desk.0],
    )?;
    Ok(loan)
}

/// Applies `circ.checkout` inside the writer's transaction.
///
/// The rules that apply are those of the Item's holding branch before the loan, the
/// patron's category and the Item's policy; the calendar is that branch's.
///
/// Koha: `C4::Circulation::CanBookBeIssued` and `C4::Circulation::AddIssue`.
pub(crate) fn checkout(connection: &Connection, request: &CheckoutRequest) -> Applied {
    let barcode = checked_barcode(&request.barcode)
        .ok_or_else(|| refused(CheckoutRefusal::UnknownBarcode))?;
    let mut item = find_item(connection, barcode)
        .map_err(storage)?
        .ok_or_else(|| refused(CheckoutRefusal::UnknownBarcode))?;
    let (category, expires) = find_patron(connection, request.patron)
        .map_err(storage)?
        .ok_or_else(|| refused(CheckoutRefusal::UnknownPatron))?;
    if !exists(
        connection,
        "SELECT 1 FROM branches WHERE id = ?1",
        request.desk.0,
    )? {
        return Err(CommandFailed::Rejected(WriteRefusal::Circulation(
            CirculationRefusal::UnknownBranch,
        )));
    }
    item.facts.on_loan = holder(connection, item.id, request.patron).map_err(storage)?;
    let rules = read_rules(connection).map_err(storage)?;
    let loans = current_loans(connection, request.patron).map_err(storage)?;
    let owed = owed(connection, request.patron).map_err(storage)?;
    let suspended = suspension(connection, request.patron)
        .map_err(storage)?
        .filter(|suspension| suspension.holds_on(request.at.date()))
        .map(Suspension::end);
    let (hold_item, answered) = item_hold(connection, &rules, &item).map_err(storage)?;
    let query = LimitQuery {
        branch: item.holding_branch,
        category,
        policy: item.policy,
        on_site: request.on_site,
        switching_on_site: false,
        on_site_counts_as_normal: true,
    };
    let check = check_checkout(
        &CheckoutFacts {
            suspended,
            patron_expired: expires < request.at.date(),
            owed,
            item: item.facts,
            too_many: too_many(&rules, &query, &loans),
            held_for_another: held_for_another(answered, request.patron),
        },
        &CheckoutSettings::default(),
    );
    if !check.impossible.is_empty() {
        return Err(refused(CheckoutRefusal::Impossible(check.impossible)));
    }
    if !check.confirm.is_empty() && !request.confirmed {
        return Err(refused(CheckoutRefusal::NeedsConfirmation(check.confirm)));
    }
    if item.facts.on_loan.is_some() {
        return Err(refused(CheckoutRefusal::OnLoan));
    }
    let scope = RuleScopeKey {
        branch: Some(item.holding_branch),
        category: Some(category),
        policy: Some(item.policy),
    };
    let calendar = read_calendar(connection, item.holding_branch.0).map_err(storage)?;
    let rule = loan_rule(&rules, &scope, USE_DAYS_MODE);
    let due = checkout_due(
        &request.at,
        &DueDateContext {
            rule: &rule,
            calendar: &calendar,
            hours: &LibraryHours::default(),
            consider_hours: ConsiderLibraryHours::Ignore,
            return_before_expiry: false,
            patron_expiry: Some(expires),
        },
    )
    .map_err(|_| refused(CheckoutRefusal::NoDueDate))?;
    let loan = record_loan(connection, item.id, request, &due).map_err(storage)?;
    fill_at_checkout(
        connection,
        &rules,
        (&hold_item, item.instance),
        request.patron,
        answered,
        &request.at.to_string(),
    )
    .map_err(storage)?;
    Ok(WriteOutput::Circulation(CirculationOutput::CheckedOut(
        CheckedOut {
            loan,
            due,
            alerts: check.alerts,
        },
    )))
}
