// SPDX-License-Identifier: GPL-3.0-or-later
//! The Elzevir's own answers on the loans of a switched copy, for `elzevir parity-koha`
//! (docs/specs/migracao/koha-parity.md, item 7). Read only.

use std::collections::{BTreeMap, BTreeSet};

use elzevir_app::koha_parity::{LoanAnswers, ParityGap, ReturnHold, hold_answer, renewal_answer};
use elzevir_domain::catalog::{BranchId, ItemId, PolicyId};
use elzevir_domain::checkout::loan_rule;
use elzevir_domain::circulation::checkout_due;
use elzevir_domain::circulation_rules::{CirculationRule, RuleScopeKey};
use elzevir_domain::due_date::{ConsiderLibraryHours, DueDateContext, LibraryHours};
use elzevir_domain::hold::{HoldItem, HoldMatch};
use elzevir_domain::patron::PatronId;
use jiff::Zoned;
use rusqlite::types::Type;
use rusqlite::{Connection, OptionalExtension, params};

use crate::StoreError;
use crate::calendar::read_calendar;
use crate::circulation_rules::read_rules;
use crate::holds::{CheckinItem, hold_at_checkin};
use crate::koha_migration::Keyed;
use crate::loans::{USE_DAYS_MODE, find_patron};
use crate::overdue_fines::{LateLoan, overdue_fine};
use crate::renewals::staff_renewal;

/// The format Koha stores and the recorder writes a due date in.
const KOHA_MINUTE: &str = "%Y-%m-%d %H:%M";

impl crate::ElzevirStore {
    /// For the Item of each `itemnumber`, the due date its loan would get again from its
    /// start, its fine at `now`, whether it may be renewed at `renewals_at`, and the hold
    /// it fills when it comes back.
    ///
    /// # Errors
    /// The database could not be read.
    pub fn koha_parity_answers(
        &self,
        itemnumbers: &BTreeSet<u64>,
        now: &Zoned,
        renewals_at: &Zoned,
    ) -> Result<BTreeMap<u64, Result<LoanAnswers, ParityGap>>, StoreError> {
        self.readers().read(|connection| {
            let rules = read_rules(connection)?;
            let mut answers = BTreeMap::new();
            for itemnumber in itemnumbers {
                let asked = Moments { now, renewals_at };
                answers.insert(
                    *itemnumber,
                    answer(connection, &rules, *itemnumber, &asked)?,
                );
            }
            Ok(answers)
        })
    }
}

/// When the recorder asked: fines at `now`, renewals at Koha's clock.
struct Moments<'a> {
    now: &'a Zoned,
    renewals_at: &'a Zoned,
}

/// A loan of the Elzevir and where it started.
struct StoredLoan {
    late: LateLoan,
    issued_at: Zoned,
}

fn answer(
    connection: &Connection,
    rules: &[CirculationRule],
    itemnumber: u64,
    asked: &Moments<'_>,
) -> Result<Result<LoanAnswers, ParityGap>, StoreError> {
    let item: Option<i64> = connection
        .query_row(
            "SELECT item_id FROM item_origins WHERE system = 'koha.itemnumber' AND external_id = ?1
             ORDER BY item_id LIMIT 1",
            [itemnumber.to_string()],
            |row| row.get(0),
        )
        .optional()?;
    let Some(item) = item else {
        return Ok(Err(ParityGap::ItemMissing));
    };
    let Some(loan) = stored_loan(connection, item)? else {
        return Ok(Err(ParityGap::NotOnLoan));
    };
    let Some((category, expires)) = find_patron(connection, loan.late.patron)? else {
        return Ok(Err(ParityGap::NotOnLoan));
    };
    let scope = RuleScopeKey {
        branch: Some(loan.late.holding),
        category: Some(category),
        policy: Some(loan.late.policy),
    };
    let calendar = read_calendar(connection, loan.late.holding.0)?;
    let rule = loan_rule(rules, &scope, USE_DAYS_MODE);
    // The same computation as `circ.checkout`, from the loan's own start.
    let due = checkout_due(
        &loan.issued_at,
        &DueDateContext {
            rule: &rule,
            calendar: &calendar,
            hours: &LibraryHours::default(),
            consider_hours: ConsiderLibraryHours::Ignore,
            return_before_expiry: false,
            patron_expiry: Some(expires),
        },
    )
    .map(|due| due.strftime(KOHA_MINUTE).to_string())
    .map_err(|_| ParityGap::NoDueDate);
    let fine = overdue_fine(connection, rules, &loan.late, asked.now)?.map_or(0, |fine| fine.0);
    let Some(renewal) = staff_renewal(connection, rules, item, asked.renewals_at)? else {
        return Ok(Err(ParityGap::NotOnLoan));
    };
    Ok(Ok(LoanAnswers {
        due,
        fine_cents: fine,
        renewal: renewal_answer(renewal.as_ref().copied()).to_owned(),
        hold_on_return: hold_on_return(connection, rules, item)?,
    }))
}

/// The hold `circ.checkin` would give the Item at its holding branch, read without
/// writing, with its patron's `borrowernumber`.
fn hold_on_return(
    connection: &Connection,
    rules: &[CirculationRule],
    item: i64,
) -> rusqlite::Result<String> {
    let returned = connection.query_row(
        "SELECT home_branch, holding_branch, policy_id, instance_id, not_for_loan
         FROM items WHERE id = ?1",
        [item],
        |row| {
            Ok(CheckinItem {
                item: HoldItem {
                    id: ItemId(item),
                    home: BranchId(row.get(0)?),
                    holding: BranchId(row.get(1)?),
                    policy: PolicyId(row.get(2)?),
                },
                instance: row.get(3)?,
                not_for_loan: row.get(4)?,
            })
        },
    )?;
    let Some(hold) = hold_at_checkin(connection, rules, &returned)? else {
        return Ok(hold_answer(None));
    };
    let state = match hold.found {
        HoldMatch::Waiting(_) => ReturnHold::Waiting,
        HoldMatch::InTransit(_) => ReturnHold::InTransit,
        HoldMatch::Reserved(_) => ReturnHold::Reserved,
    };
    let borrowernumber: Option<String> = connection
        .query_row(
            "SELECT k.koha_key FROM holds h
             JOIN koha_keys k ON k.kind = ?2 AND k.local_id = h.patron_id
             WHERE h.id = ?1",
            params![hold.found.hold().0, Keyed::Patron as i64],
            |row| row.get(0),
        )
        .optional()?;
    Ok(hold_answer(Some((state, borrowernumber.as_deref()))))
}

fn stored_loan(connection: &Connection, item: i64) -> rusqlite::Result<Option<StoredLoan>> {
    let zoned = |text: String, column: usize| -> rusqlite::Result<Zoned> {
        text.parse().map_err(|error: jiff::Error| {
            rusqlite::Error::FromSqlConversionFailure(column, Type::Text, error.into())
        })
    };
    connection
        .query_row(
            "SELECT l.id, l.patron_id, i.holding_branch, i.policy_id, i.replacement_price_cents,
                    l.issued_at, l.due
             FROM loans l JOIN items i ON i.id = l.item_id
             WHERE l.item_id = ?1",
            params![item],
            |row| {
                Ok(StoredLoan {
                    late: LateLoan {
                        loan: row.get(0)?,
                        patron: PatronId(row.get(1)?),
                        item,
                        holding: BranchId(row.get(2)?),
                        policy: PolicyId(row.get(3)?),
                        replacement_price: row.get(4)?,
                        due: zoned(row.get(6)?, 6)?,
                    },
                    issued_at: zoned(row.get(5)?, 5)?,
                })
            },
        )
        .optional()
}
