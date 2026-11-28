// SPDX-License-Identifier: GPL-3.0-or-later
//! The names of Koha's status numbers (the authorised values `LOST`, `DAMAGED`,
//! `WITHDRAWN` and `NOT_LOAN`) and the moment each Item took its status, read for the
//! migration (docs/specs/circulacao/lost-items.md, items 17 and 18). Koha's category
//! codes stay at this edge.

use std::collections::BTreeMap;

use jiff::tz::TimeZone;
use serde::{Deserialize, Serialize};

use super::circulation::{barcodes, row_item, zoned};
use super::dump::{Row, Table};
use super::{KohaItem, KohaPlan, text};
use crate::item_status::{MAX_STATUS_LABEL, MAX_STATUS_NUMBER, StatusKind};

/// Most Items in one write command.
pub const STATUS_DATES_PER_COMMAND: usize = 5_000;

/// The kind of status each authorised value category names.
///
/// Koha: `items.itemlost`, `damaged`, `withdrawn` and `notforloan` take their names from
/// these categories (`Koha::Item::_status`, `C4::Items`).
const CATEGORIES: [(&str, StatusKind); 4] = [
    ("LOST", StatusKind::Lost),
    ("DAMAGED", StatusKind::Damaged),
    ("WITHDRAWN", StatusKind::Withdrawn),
    ("NOT_LOAN", StatusKind::NotForLoan),
];

/// The name of a status number.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KohaStatusName {
    /// The kind.
    pub kind: StatusKind,
    /// `authorised_value`, from 1 to 99.
    pub number: i64,
    /// `lib`, trimmed and cut to the longest name.
    pub label: String,
}

/// When an Item took each of its statuses, in the library's zone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KohaStatusDates {
    /// The Item.
    pub item: KohaItem,
    /// `itemlost_on`.
    pub lost_on: Option<String>,
    /// `damaged_on`.
    pub damaged_on: Option<String>,
    /// `withdrawn_on`.
    pub withdrawn_on: Option<String>,
}

/// One write: the names (on the first) and a share of the dates.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KohaStatusesBatch {
    /// The names.
    pub names: Vec<KohaStatusName>,
    /// The dates.
    pub dates: Vec<KohaStatusDates>,
}

/// What a write of statuses did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KohaStatusesMigrated {
    /// Names given.
    pub names: usize,
    /// Numbers a former run, or the library, already named.
    pub names_existing: usize,
    /// Names another number of the same kind has, given with their number.
    pub names_numbered: usize,
    /// Names left out because even with their number another one has them.
    pub names_taken: usize,
    /// Items given at least one date.
    pub dates: usize,
    /// Items with dates that did not come, or that the Elzevir has more than once.
    pub items_left: usize,
}

impl KohaStatusesMigrated {
    /// Adds what another write did.
    pub fn add(&mut self, other: &Self) {
        self.names += other.names;
        self.names_existing += other.names_existing;
        self.names_numbered += other.names_numbered;
        self.names_taken += other.names_taken;
        self.dates += other.dates;
        self.items_left += other.items_left;
    }
}

/// The name of a number whose name another number of the same kind has: the name and the
/// number, cut to fit the longest name. Koha shows both numbers with the same name.
#[must_use]
pub fn numbered_label(label: &str, number: i64) -> String {
    let suffix = format!(" ({number})");
    let room = MAX_STATUS_LABEL.saturating_sub(suffix.chars().count());
    let mut numbered: String = label.chars().take(room).collect();
    numbered.push_str(&suffix);
    numbered
}

/// Reads the names and the dates into the plan.
pub(super) fn read(tables: &BTreeMap<Table, Vec<Row>>, zone: &TimeZone, plan: &mut KohaPlan) {
    let rows = |table| tables.get(&table).map_or(&[][..], Vec::as_slice);
    for row in rows(Table::AuthorisedValues) {
        let Some(kind) = text(row, "category").and_then(|category| {
            CATEGORIES
                .iter()
                .find(|(code, _)| *code == category)
                .map(|(_, kind)| *kind)
        }) else {
            continue;
        };
        let number = text(row, "authorised_value").and_then(|value| value.parse::<i64>().ok());
        let label: Option<String> =
            text(row, "lib").map(|label| label.chars().take(MAX_STATUS_LABEL).collect());
        match (number, label) {
            (Some(number), Some(label)) if (1..=MAX_STATUS_NUMBER).contains(&number) => {
                plan.status_names.push(KohaStatusName {
                    kind,
                    number,
                    label: label.trim_end().to_owned(),
                });
            }
            // Item 17: a negative not-for-loan number, or one without a name, keeps its
            // generic name.
            _ => plan.status_names_left += 1,
        }
    }
    let barcodes = barcodes(tables);
    for row in rows(Table::Items) {
        // A date stays only with its status, as `Koha::Item::store` clears it with the
        // status; an older Koha may have left one behind.
        let at = |status, column| {
            text(row, status)
                .filter(|number| number != "0")
                .and_then(|_| zoned(text(row, column).as_deref(), zone))
        };
        let (lost_on, damaged_on, withdrawn_on) = (
            at("itemlost", "itemlost_on"),
            at("damaged", "damaged_on"),
            at("withdrawn", "withdrawn_on"),
        );
        if lost_on.is_none() && damaged_on.is_none() && withdrawn_on.is_none() {
            continue;
        }
        if let Some(item) = row_item(row, &barcodes) {
            plan.status_dates.push(KohaStatusDates {
                item,
                lost_on,
                damaged_on,
                withdrawn_on,
            });
        }
    }
}

impl KohaPlan {
    /// The writes of the statuses, at most [`STATUS_DATES_PER_COMMAND`] Items each; the
    /// first carries the names, and there is none when there is nothing to write.
    #[must_use]
    pub fn status_batches(&self) -> Vec<KohaStatusesBatch> {
        let mut batches: Vec<KohaStatusesBatch> = self
            .status_dates
            .chunks(STATUS_DATES_PER_COMMAND)
            .map(|dates| KohaStatusesBatch {
                names: Vec::new(),
                dates: dates.to_vec(),
            })
            .collect();
        if self.status_names.is_empty() {
            return batches;
        }
        match batches.first_mut() {
            Some(first) => first.names.clone_from(&self.status_names),
            None => batches.push(KohaStatusesBatch {
                names: self.status_names.clone(),
                dates: Vec::new(),
            }),
        }
        batches
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_repeated_name_takes_its_number_and_still_fits() {
        assert_eq!(numbered_label("Perdido", 6), "Perdido (6)");
        let long = numbered_label(&"x".repeat(MAX_STATUS_LABEL), 99);
        assert_eq!(long.chars().count(), MAX_STATUS_LABEL);
        assert!(long.ends_with("x (99)"));
    }
}
