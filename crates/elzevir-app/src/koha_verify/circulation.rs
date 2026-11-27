// SPDX-License-Identifier: GPL-3.0-or-later
//! Loans, holds, accounts and returned loans of the dump against the Elzevir
//! (docs/specs/migracao/verificacao-do-koha.md, items 13 to 16).

use crate::holds::Suspension;
use std::collections::{BTreeMap, BTreeSet};

use crate::koha_migration::{KohaHoldState, KohaPlan};
use crate::patrons::ReadingHistory;

use super::{Cause, Footprint, Table, TableCheck, Unexplained, extras, sorted};

/// A current loan as the Elzevir holds it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FoundLoan {
    /// The patron's `borrowernumber`.
    pub borrowernumber: Option<String>,
    /// Due date and time, as stored.
    pub due: String,
}

/// Where a hold of the Elzevir stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoldState {
    /// In the queue.
    Queued,
    /// Set aside and waiting at the pickup branch.
    Waiting,
    /// Set aside and travelling to the pickup branch.
    InTransit,
}

/// A current hold as the Elzevir holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundHold {
    /// The patron's `borrowernumber`.
    pub borrowernumber: Option<String>,
    /// Queue or set aside.
    pub state: HoldState,
    /// Place in the queue; 0 when set aside.
    pub priority: i64,
    /// Whether it is suspended, and until when.
    pub suspended: Option<Suspension>,
}

impl From<KohaHoldState> for HoldState {
    fn from(state: KohaHoldState) -> Self {
        match state {
            KohaHoldState::Queued(_) => Self::Queued,
            KohaHoldState::Waiting { .. } => Self::Waiting,
            KohaHoldState::InTransit => Self::InTransit,
        }
    }
}

fn patron_in(found: &Footprint, number: u64) -> bool {
    found.patrons.contains_key(&number.to_string())
}

fn item_in(found: &Footprint, number: u64) -> bool {
    found
        .items
        .get(&number.to_string())
        .is_some_and(|items| !items.is_empty())
}

fn record_in(found: &Footprint, number: &str) -> bool {
    found.records.get(number).copied().unwrap_or(0) > 0
}

pub(super) fn loans(plan: &KohaPlan, found: &Footprint) -> TableCheck {
    let mut check = TableCheck::new(Table::Loans);
    for (number, why) in &plan.loans_left {
        check.in_dump += 1;
        check.explain(Cause::Circulation(*why), &number.to_string());
    }
    // Who each Item is lent to in the dump, to explain a second loan of the same Item.
    let mut lent_to: BTreeMap<u64, BTreeSet<u64>> = BTreeMap::new();
    for loan in &plan.loans {
        lent_to
            .entry(loan.item.itemnumber)
            .or_default()
            .insert(loan.borrowernumber);
    }
    let mut seen = BTreeSet::new();
    for loan in &plan.loans {
        check.in_dump += 1;
        let number = loan.issue_id.to_string();
        let item = loan.item.itemnumber.to_string();
        let stored = found.loans.get(&item).map_or(&[][..], Vec::as_slice);
        let borrower = loan.borrowernumber.to_string();
        if let Some(stored) = stored
            .iter()
            .find(|stored| stored.borrowernumber.as_deref() == Some(borrower.as_str()))
        {
            check.arrived += 1;
            if stored.due != loan.due {
                check.differ(&number, Unexplained::Field("date_due"));
            }
        } else if !patron_in(found, loan.borrowernumber) {
            check.explain(Cause::PatronLeftOut, &number);
        } else if !item_in(found, loan.item.itemnumber) {
            check.explain(Cause::ItemLeftOut, &number);
        } else if !found.branches.contains_key(&loan.branch) {
            check.explain(Cause::UnknownBranch(loan.branch.clone()), &number);
        } else if stored.iter().any(|stored| {
            stored
                .borrowernumber
                .as_deref()
                .and_then(|other| other.parse().ok())
                .is_some_and(|other| {
                    lent_to
                        .get(&loan.item.itemnumber)
                        .is_some_and(|to| to.contains(&other))
                })
        }) {
            check.explain(Cause::ItemOnLoan, &number);
        } else if stored.is_empty() {
            check.differ(&number, Unexplained::Missing);
        } else {
            check.differ(&number, Unexplained::Field("borrowernumber"));
        }
        seen.insert(item);
    }
    let seen: BTreeSet<&str> = seen.iter().map(String::as_str).collect();
    for (item, _) in found
        .loans
        .iter()
        .filter(|(item, _)| !seen.contains(item.as_str()))
    {
        check.differ(item, Unexplained::ExtraBy("itemnumber"));
    }
    sorted(check)
}

/// Why a hold that the dump brings is not in the Elzevir.
fn hold_cause(plan_hold: &crate::koha_migration::KohaHold, found: &Footprint) -> Option<Cause> {
    let needs_item = plan_hold.item_level || !matches!(plan_hold.state, KohaHoldState::Queued(_));
    if !patron_in(found, plan_hold.borrowernumber) {
        Some(Cause::PatronLeftOut)
    } else if !record_in(found, &plan_hold.biblionumber.to_string()) {
        Some(Cause::RecordLeftOut)
    } else if needs_item
        && plan_hold
            .item
            .as_ref()
            .is_some_and(|item| !item_in(found, item.itemnumber))
    {
        Some(Cause::ItemLeftOut)
    } else if !found.branches.contains_key(&plan_hold.pickup) {
        Some(Cause::UnknownBranch(plan_hold.pickup.clone()))
    } else {
        None
    }
}

pub(super) fn holds(plan: &KohaPlan, found: &Footprint) -> TableCheck {
    let mut check = TableCheck::new(Table::Holds);
    for (number, why) in &plan.holds_left {
        check.in_dump += 1;
        check.explain(Cause::Circulation(*why), &number.to_string());
    }
    let mut held = BTreeSet::new();
    // The queued holds that came, by record: (Koha priority, reserve_id, borrowernumber).
    let mut queues: BTreeMap<String, Vec<(u32, u64, String)>> = BTreeMap::new();
    for hold in &plan.holds {
        check.in_dump += 1;
        let number = hold.reserve_id.to_string();
        let record = hold.biblionumber.to_string();
        let borrower = hold.borrowernumber.to_string();
        if !held.insert((record.clone(), borrower.clone())) {
            check.explain(Cause::AlreadyHeld, &number);
            continue;
        }
        let stored = found.holds.get(&record).and_then(|holds| {
            holds
                .iter()
                .find(|stored| stored.borrowernumber.as_deref() == Some(borrower.as_str()))
        });
        let Some(stored) = stored else {
            match hold_cause(hold, found) {
                Some(cause) => check.explain(cause, &number),
                None => check.differ(&number, Unexplained::Missing),
            }
            continue;
        };
        check.arrived += 1;
        if stored.state != HoldState::from(hold.state) {
            check.differ(&number, Unexplained::Field("found"));
        } else if stored.suspended != hold.suspended {
            check.differ(&number, Unexplained::Field("suspend"));
        } else if let KohaHoldState::Queued(priority) = hold.state {
            queues
                .entry(record)
                .or_default()
                .push((priority, hold.reserve_id, borrower));
        }
    }
    for (record, mut expected) in queues {
        expected.sort();
        let borrowers: BTreeSet<&str> = expected.iter().map(|(_, _, b)| b.as_str()).collect();
        let mut stored: Vec<&FoundHold> = found
            .holds
            .get(&record)
            .map_or(&[][..], Vec::as_slice)
            .iter()
            .filter(|hold| {
                hold.state == HoldState::Queued
                    && hold
                        .borrowernumber
                        .as_deref()
                        .is_some_and(|b| borrowers.contains(b))
            })
            .collect();
        stored.sort_by_key(|hold| hold.priority);
        for ((_, reserve, borrower), stored) in expected.iter().zip(&stored) {
            if stored.borrowernumber.as_deref() != Some(borrower.as_str()) {
                check.differ(&reserve.to_string(), Unexplained::Field("priority"));
            }
        }
    }
    for (record, holds) in &found.holds {
        for hold in holds {
            let key = (
                record.clone(),
                hold.borrowernumber.clone().unwrap_or_default(),
            );
            if !held.contains(&key) {
                check.differ(record, Unexplained::ExtraBy("biblionumber"));
            }
        }
    }
    sorted(check)
}

/// Each account's balance as Koha computes it, the sum of `amountoutstanding`.
///
/// Koha: `Koha::Account::balance`
pub(super) fn accounts(plan: &KohaPlan, found: &Footprint) -> TableCheck {
    let mut check = TableCheck::new(Table::Accounts);
    let mut seen = BTreeSet::new();
    let numbers: Vec<String> = plan
        .accounts
        .iter()
        .map(|account| account.borrowernumber.to_string())
        .collect();
    for (account, number) in plan.accounts.iter().zip(&numbers) {
        check.in_dump += 1;
        seen.insert(number.as_str());
        let balance: i64 = account.lines.iter().map(|line| line.outstanding.0).sum();
        match found.accounts.get(number) {
            None if account.lines.is_empty() => check.arrived += 1,
            None if !patron_in(found, account.borrowernumber) => {
                check.explain(Cause::PatronLeftOut, number);
            }
            None => check.differ(number, Unexplained::Missing),
            Some((stored, lines)) => {
                check.arrived += 1;
                if *stored != balance {
                    check.differ(number, Unexplained::Field("amountoutstanding"));
                }
                if *lines != account.lines.len() {
                    check.differ(number, Unexplained::Field("accountlines"));
                }
            }
        }
    }
    extras(&mut check, found.accounts.iter(), &seen);
    sorted(check)
}

pub(super) fn returned(plan: &KohaPlan, found: &Footprint) -> TableCheck {
    let mut check = TableCheck::new(Table::Returned);
    for (number, why) in &plan.returned_left {
        check.in_dump += 1;
        check.explain(Cause::Circulation(*why), &number.to_string());
    }
    let keeping: BTreeSet<u64> = plan
        .patrons
        .iter()
        .filter(|patron| patron.reading_history == ReadingHistory::Kept)
        .map(|patron| patron.borrowernumber)
        .collect();
    let numbers: Vec<String> = plan
        .returned
        .iter()
        .map(|loan| loan.issue_id.to_string())
        .collect();
    let mut seen = BTreeSet::new();
    for (loan, number) in plan.returned.iter().zip(&numbers) {
        check.in_dump += 1;
        seen.insert(number.as_str());
        let Some(with_patron) = found.returned.get(number) else {
            if item_in(found, loan.item.itemnumber) {
                check.differ(number, Unexplained::Missing);
            } else {
                check.explain(Cause::ItemLeftOut, number);
            }
            continue;
        };
        check.arrived += 1;
        let patron = loan
            .borrowernumber
            .and_then(|b| found.patrons.get(&b.to_string()));
        if patron.is_some_and(|patron| patron.anonymized) {
            continue;
        }
        let expected = patron.is_some()
            && loan
                .borrowernumber
                .is_some_and(|number| keeping.contains(&number));
        if *with_patron != expected {
            check.differ(number, Unexplained::Field("borrowernumber"));
        }
    }
    extras(&mut check, found.returned.iter(), &seen);
    sorted(check)
}
