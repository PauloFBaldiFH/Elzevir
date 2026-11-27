// SPDX-License-Identifier: GPL-3.0-or-later
//! The Elzevir's own answers on the loans of a switched copy, for `elzevir parity-koha`
//! (docs/specs/migracao/koha-parity.md, item 7). Read only.

use std::collections::{BTreeMap, BTreeSet};

use elzevir_app::koha_parity::{LoanAnswers, ParityGap};
use elzevir_domain::catalog::{BranchId, PolicyId};
use elzevir_domain::checkout::loan_rule;
use elzevir_domain::circulation::checkout_due;
use elzevir_domain::circulation_rules::{CirculationRule, RuleScopeKey};
use elzevir_domain::due_date::{ConsiderLibraryHours, DueDateContext, LibraryHours};
use elzevir_domain::patron::PatronId;
use jiff::Zoned;
use rusqlite::types::Type;
use rusqlite::{Connection, OptionalExtension, params};

use crate::StoreError;
use crate::calendar::read_calendar;
use crate::circulation_rules::read_rules;
use crate::loans::{USE_DAYS_MODE, find_patron};
use crate::overdue_fines::{LateLoan, overdue_fine};

/// The format Koha stores and the recorder writes a due date in.
const KOHA_MINUTE: &str = "%Y-%m-%d %H:%M";

impl crate::ElzevirStore {
    /// For the Item of each `itemnumber`, the due date its loan would get again from its
    /// start, and its fine at `now`.
    ///
    /// # Errors
    /// The database could not be read.
    pub fn koha_parity_answers(
        &self,
        itemnumbers: &BTreeSet<u64>,
        now: &Zoned,
    ) -> Result<BTreeMap<u64, Result<LoanAnswers, ParityGap>>, StoreError> {
        self.readers().read(|connection| {
            let rules = read_rules(connection)?;
            let mut answers = BTreeMap::new();
            for itemnumber in itemnumbers {
                answers.insert(*itemnumber, answer(connection, &rules, *itemnumber, now)?);
            }
            Ok(answers)
        })
    }
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
    now: &Zoned,
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
    let fine = overdue_fine(connection, rules, &loan.late, now)?.map_or(0, |fine| fine.0);
    Ok(Ok(LoanAnswers {
        due,
        fine_cents: fine,
    }))
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
