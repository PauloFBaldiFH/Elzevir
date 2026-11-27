// SPDX-License-Identifier: GPL-3.0-or-later
//! Fixed reports, each also as CSV (docs/specs/operacao/relatorios.md).

use jiff::civil::Date;

use crate::material::ContentKind;

/// The days a report counts, both included (item 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Period {
    /// The first day.
    pub from: Date,
    /// The last day.
    pub to: Date,
}

impl Period {
    /// The period from `from` to `to`; `None` when it ends before it starts.
    #[must_use]
    pub fn new(from: Date, to: Date) -> Option<Self> {
        (from <= to).then_some(Self { from, to })
    }
}

/// One line of the circulation report: a branch and a patron category (item 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CirculationLine {
    /// The branch's name.
    pub branch: String,
    /// The category's name; `None` for loans whose category is no longer known.
    pub category: Option<String>,
    /// Loans issued at the branch in the period.
    pub checkouts: u64,
    /// Loans returned at the branch in the period.
    pub returns: u64,
}

/// One line of the collection report: a Dewey main class and a kind of content (item 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionLine {
    /// The hundreds of the class (0 to 900); `None` for a call number without one.
    pub class: Option<u16>,
    /// The content of the Item's Instance, when its description gives it.
    pub content: Option<ContentKind>,
    /// Items, withdrawn ones left out.
    pub items: u64,
}

/// One line of the patrons report: a branch and a category (item 8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatronsLine {
    /// The home branch's name.
    pub branch: String,
    /// The category's name.
    pub category: String,
    /// Patrons, anonymized ones left out.
    pub patrons: u64,
    /// Of those, the ones who borrowed on the day asked or after it.
    pub active: u64,
}

/// Why a report could not be read.
#[derive(Debug, thiserror::Error)]
#[error("the reports could not be read: {0}")]
pub struct ReportError(pub String);

/// Port: reading the reports. The store implements it from the read pool.
pub trait ReportReader: Send + Sync {
    /// Loans issued and returned in `period`, by branch and category, in name order.
    ///
    /// # Errors
    ///
    /// When storage fails.
    fn circulation(&self, period: Period) -> Result<Vec<CirculationLine>, ReportError>;

    /// The Items of the collection, by main class and content, classes in order.
    ///
    /// # Errors
    ///
    /// When storage fails.
    fn collection(&self) -> Result<Vec<CollectionLine>, ReportError>;

    /// Patrons, and the ones who borrowed on `since` or after it, by home branch and
    /// category, in name order.
    ///
    /// # Errors
    ///
    /// When storage fails.
    fn patrons(&self, since: Date) -> Result<Vec<PatronsLine>, ReportError>;
}

#[cfg(test)]
mod tests {
    use jiff::civil::date;

    use super::*;

    #[test]
    fn a_period_does_not_end_before_it_starts() {
        assert!(Period::new(date(2026, 1, 1), date(2026, 1, 1)).is_some());
        assert!(Period::new(date(2026, 1, 2), date(2026, 1, 1)).is_none());
    }
}
