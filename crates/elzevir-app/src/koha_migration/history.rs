// SPDX-License-Identifier: GPL-3.0-or-later
//! Koha's returned loans (`old_issues`), read for the migration
//! (docs/specs/migracao/circulacao-do-koha.md, items 14 to 17).

use std::collections::BTreeMap;

use jiff::tz::TimeZone;
use serde::{Deserialize, Serialize};

use super::circulation::{barcodes, row_item, zoned};
use super::dump::{Row, Table};
use super::{CirculationLeftover, KohaItem, KohaPlan, number, text};

/// Most returned loans in one write command.
pub const RETURNED_PER_COMMAND: usize = 500;

/// A returned loan (`old_issues`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KohaReturnedLoan {
    /// `issue_id`.
    pub issue_id: u64,
    /// `borrowernumber`; kept only for a patron who keeps a reading history.
    pub borrowernumber: Option<u64>,
    /// The Item.
    pub item: KohaItem,
    /// `branchcode`, where it was lent.
    pub branch: String,
    /// `checkin_library`, where it came back; else where it was lent.
    pub return_branch: String,
    /// `issuedate`, in the library's zone.
    pub issued_at: String,
    /// `date_due`, in the library's zone.
    pub due: String,
    /// `returndate`, in the library's zone.
    pub returned_at: String,
    /// `renewals_count`.
    pub renewals: u32,
    /// `onsite_checkout`.
    pub on_site: bool,
}

/// One write of returned loans.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KohaHistoryBatch {
    /// The returned loans to add.
    pub returned: Vec<KohaReturnedLoan>,
}

/// What a write of returned loans did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KohaHistoryMigrated {
    /// Returned loans added.
    pub returned: usize,
    /// Of those, the ones that kept their patron.
    pub with_patron: usize,
    /// Returned loans a former run already added.
    pub already_migrated: usize,
    /// Returned loans left out, by `issue_id`.
    pub left: Vec<(u64, CirculationLeftover)>,
}

impl KohaHistoryMigrated {
    /// Adds what another write did.
    pub fn add(&mut self, other: Self) {
        self.returned += other.returned;
        self.with_patron += other.with_patron;
        self.already_migrated += other.already_migrated;
        self.left.extend(other.left);
    }
}

/// Reads `old_issues` into the plan, oldest first.
pub(super) fn read(tables: &BTreeMap<Table, Vec<Row>>, zone: &TimeZone, plan: &mut KohaPlan) {
    let barcodes = barcodes(tables);
    for row in tables.get(&Table::OldIssues).map_or(&[][..], Vec::as_slice) {
        let issue_id = number(text(row, "issue_id").as_deref()).unwrap_or(0);
        match returned(row, issue_id, row_item(row, &barcodes), zone) {
            Some(loan) => plan.returned.push(loan),
            None => plan
                .returned_left
                .push((issue_id, CirculationLeftover::Incomplete)),
        }
    }
    plan.returned.sort_by_key(|loan| loan.issue_id);
}

fn returned(
    row: &Row,
    issue_id: u64,
    item: Option<KohaItem>,
    zone: &TimeZone,
) -> Option<KohaReturnedLoan> {
    let branch = text(row, "branchcode")?;
    Some(KohaReturnedLoan {
        issue_id: (issue_id > 0).then_some(issue_id)?,
        borrowernumber: number(text(row, "borrowernumber").as_deref()),
        item: item?,
        return_branch: text(row, "checkin_library").unwrap_or_else(|| branch.clone()),
        branch,
        issued_at: zoned(text(row, "issuedate").as_deref(), zone)?,
        due: zoned(text(row, "date_due").as_deref(), zone)?,
        returned_at: zoned(text(row, "returndate").as_deref(), zone)?,
        renewals: text(row, "renewals_count")
            .and_then(|count| count.parse().ok())
            .unwrap_or(0),
        on_site: text(row, "onsite_checkout").as_deref() == Some("1"),
    })
}

impl KohaPlan {
    /// The writes of returned loans, at most [`RETURNED_PER_COMMAND`] each.
    #[must_use]
    pub fn history_batches(&self) -> Vec<KohaHistoryBatch> {
        self.returned
            .chunks(RETURNED_PER_COMMAND)
            .map(|returned| KohaHistoryBatch {
                returned: returned.to_vec(),
            })
            .collect()
    }
}
