// SPDX-License-Identifier: GPL-3.0-or-later
//! Koha's current loans and holds, read for the migration
//! (docs/specs/migracao/circulacao-do-koha.md). Patrons, records and Items are named by
//! Koha's numbers; the writer finds them through the edge tables.

use std::collections::BTreeMap;

use jiff::civil::{Date, DateTime, Time};
use jiff::tz::TimeZone;
use serde::{Deserialize, Serialize};

use super::dump::{Row, Table};
use super::{KohaPlan, date, number, text};

/// Most loans or holds in one write command.
pub const CIRCULATION_PER_COMMAND: usize = 500;

/// An Item as Koha numbers it, with its barcode for an Item imported without the number.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KohaItem {
    /// `itemnumber`.
    pub itemnumber: u64,
    /// `items.barcode`.
    pub barcode: Option<String>,
}

/// A current loan (`issues`), item 2.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KohaLoan {
    /// `issue_id`.
    pub issue_id: u64,
    /// `borrowernumber`.
    pub borrowernumber: u64,
    /// The Item.
    pub item: KohaItem,
    /// `branchcode`, where it was lent.
    pub branch: String,
    /// `issuedate`, in the library's zone.
    pub issued_at: String,
    /// `date_due`, in the library's zone.
    pub due: String,
    /// `renewals_count`.
    pub renewals: u32,
    /// `lastreneweddate`, in the library's zone.
    pub renewed_at: Option<String>,
    /// `onsite_checkout`.
    pub on_site: bool,
}

/// Where a hold stands (`reserves.found`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KohaHoldState {
    /// In the queue, at this place.
    Queued(u32),
    /// Set aside and waiting at the pickup branch since this day, until the other.
    Waiting {
        /// `waitingdate`.
        since: Date,
        /// `expirationdate`, the last day to pick it up.
        until: Option<Date>,
    },
    /// Set aside and travelling to the pickup branch.
    InTransit,
}

/// A current hold (`reserves`), item 4.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KohaHold {
    /// `reserve_id`.
    pub reserve_id: u64,
    /// `borrowernumber`.
    pub borrowernumber: u64,
    /// `biblionumber`.
    pub biblionumber: u64,
    /// The Item asked for, or set aside.
    pub item: Option<KohaItem>,
    /// Whether the patron asked for that Item (`item_level_hold`).
    pub item_level: bool,
    /// `branchcode`, the pickup branch.
    pub pickup: String,
    /// `reservedate`.
    pub placed_on: Date,
    /// When it was placed, in the library's zone (`reservedate` at midnight).
    pub created_at: String,
    /// Where it stands.
    pub state: KohaHoldState,
}

/// One write of loans and holds.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KohaCirculationBatch {
    /// Loans to add.
    pub loans: Vec<KohaLoan>,
    /// Holds to add.
    pub holds: Vec<KohaHold>,
}

/// Why a loan or a hold does not come.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CirculationLeftover {
    /// A row without a number, patron, Item or date it needs.
    Incomplete,
    /// Its patron did not come.
    Patron,
    /// No Elzevir Item has its `itemnumber` or barcode, or two do.
    Item,
    /// No Elzevir Instance has its `biblionumber`, or two do.
    Record,
    /// The branch is not in the migration.
    Branch,
    /// The Item is already lent to another patron.
    ItemOnLoan,
}

/// What a write of loans and holds did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KohaCirculationMigrated {
    /// Loans added.
    pub loans: usize,
    /// Holds added.
    pub holds: usize,
    /// Loans and holds a former run already added.
    pub already_migrated: usize,
    /// Loans left out, by `issue_id`.
    pub loans_left: Vec<(u64, CirculationLeftover)>,
    /// Holds left out, by `reserve_id`.
    pub holds_left: Vec<(u64, CirculationLeftover)>,
}

impl KohaCirculationMigrated {
    /// Adds what another write did.
    pub fn add(&mut self, other: Self) {
        self.loans += other.loans;
        self.holds += other.holds;
        self.already_migrated += other.already_migrated;
        self.loans_left.extend(other.loans_left);
        self.holds_left.extend(other.holds_left);
    }
}

/// A Koha date and time, in the library's zone.
fn zoned(value: Option<&str>, zone: &TimeZone) -> Option<String> {
    let at: DateTime = value?.parse().ok()?;
    Some(at.to_zoned(zone.clone()).ok()?.to_string())
}

/// Reads `issues` and `reserves` into the plan, with the Items' barcodes.
pub(super) fn read(tables: &BTreeMap<Table, Vec<Row>>, zone: &TimeZone, plan: &mut KohaPlan) {
    let rows = |table| tables.get(&table).map_or(&[][..], Vec::as_slice);
    let barcodes: BTreeMap<u64, String> = rows(Table::Items)
        .iter()
        .filter_map(|row| {
            Some((
                number(text(row, "itemnumber").as_deref())?,
                text(row, "barcode")?,
            ))
        })
        .collect();
    let item = |row: &Row| {
        number(text(row, "itemnumber").as_deref()).map(|itemnumber| KohaItem {
            itemnumber,
            barcode: barcodes.get(&itemnumber).cloned(),
        })
    };
    for row in rows(Table::Issues) {
        let issue_id = number(text(row, "issue_id").as_deref()).unwrap_or(0);
        match loan(row, issue_id, item(row), zone) {
            Some(loan) => plan.loans.push(loan),
            None => plan
                .loans_left
                .push((issue_id, CirculationLeftover::Incomplete)),
        }
    }
    let mut holds = Vec::new();
    for row in rows(Table::Reserves) {
        let reserve_id = number(text(row, "reserve_id").as_deref()).unwrap_or(0);
        plan.suspended_holds += usize::from(text(row, "suspend").as_deref() == Some("1"));
        match hold(row, reserve_id, item(row), zone, plan) {
            Some(hold) => holds.push(hold),
            None => plan
                .holds_left
                .push((reserve_id, CirculationLeftover::Incomplete)),
        }
    }
    // The queue in Koha's order: set aside first, then by place, then by age.
    holds.sort_by_key(|hold| {
        let place = match hold.state {
            KohaHoldState::Queued(place) => place,
            KohaHoldState::Waiting { .. } | KohaHoldState::InTransit => 0,
        };
        (hold.biblionumber, place, hold.reserve_id)
    });
    plan.holds = holds;
}

fn loan(row: &Row, issue_id: u64, item: Option<KohaItem>, zone: &TimeZone) -> Option<KohaLoan> {
    let issued_at = zoned(text(row, "issuedate").as_deref(), zone)?;
    Some(KohaLoan {
        issue_id: (issue_id > 0).then_some(issue_id)?,
        borrowernumber: number(text(row, "borrowernumber").as_deref())?,
        item: item?,
        branch: text(row, "branchcode")?,
        due: zoned(text(row, "date_due").as_deref(), zone)?,
        renewals: text(row, "renewals_count")
            .and_then(|count| count.parse().ok())
            .unwrap_or(0),
        renewed_at: zoned(text(row, "lastreneweddate").as_deref(), zone),
        on_site: text(row, "onsite_checkout").as_deref() == Some("1"),
        issued_at,
    })
}

fn hold(
    row: &Row,
    reserve_id: u64,
    item: Option<KohaItem>,
    zone: &TimeZone,
    plan: &mut KohaPlan,
) -> Option<KohaHold> {
    let placed_on = date(text(row, "reservedate").as_deref())?;
    let state = match text(row, "found").as_deref() {
        None => {
            if text(row, "expirationdate").is_some() {
                plan.holds_with_end += 1;
            }
            KohaHoldState::Queued(
                text(row, "priority")
                    .and_then(|place| place.parse().ok())
                    .unwrap_or(u32::MAX),
            )
        }
        Some("T") => KohaHoldState::InTransit,
        // `P` is a hold set aside and being processed at the desk: it waits there.
        Some(_) => KohaHoldState::Waiting {
            since: date(text(row, "waitingdate").as_deref()).unwrap_or(placed_on),
            until: date(text(row, "expirationdate").as_deref()),
        },
    };
    if !matches!(state, KohaHoldState::Queued(_)) && item.is_none() {
        return None;
    }
    Some(KohaHold {
        reserve_id: (reserve_id > 0).then_some(reserve_id)?,
        borrowernumber: number(text(row, "borrowernumber").as_deref())?,
        biblionumber: number(text(row, "biblionumber").as_deref())?,
        item_level: text(row, "item_level_hold").as_deref() == Some("1"),
        item,
        pickup: text(row, "branchcode")?,
        created_at: placed_on
            .to_datetime(Time::midnight())
            .to_zoned(zone.clone())
            .ok()?
            .to_string(),
        placed_on,
        state,
    })
}

impl KohaPlan {
    /// The writes of loans and holds, at most [`CIRCULATION_PER_COMMAND`] of each.
    #[must_use]
    pub fn circulation_batches(&self) -> Vec<KohaCirculationBatch> {
        let mut loans = self.loans.chunks(CIRCULATION_PER_COMMAND);
        let mut holds = self.holds.chunks(CIRCULATION_PER_COMMAND);
        let mut batches = Vec::new();
        loop {
            let batch = KohaCirculationBatch {
                loans: loans.next().map(<[_]>::to_vec).unwrap_or_default(),
                holds: holds.next().map(<[_]>::to_vec).unwrap_or_default(),
            };
            if batch.loans.is_empty() && batch.holds.is_empty() {
                return batches;
            }
            batches.push(batch);
        }
    }
}
