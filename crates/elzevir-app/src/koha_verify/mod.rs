// SPDX-License-Identifier: GPL-3.0-or-later
//! The comparison of a Koha dump with what the Elzevir holds from it, by Koha's numbers
//! (docs/specs/migracao/verificacao-do-koha.md). Pure: the dump is read by the migration's
//! readers and the database by the store, and this module only compares.

use std::collections::{BTreeMap, BTreeSet};

use crate::calendar::Holiday;
use crate::import::koha_sql::KohaCatalogRecord;
use crate::koha_migration::{
    CirculationLeftover, HolidayLeftover, HolidayTable, KohaPlan, Leftover, RuleLeftover,
};

mod calendar;
mod circulation;
mod rules;

/// A rule by its scope as Koha's codes (`None` is "any") and its `rule_name`.
pub type RuleKey = (Option<String>, Option<String>, Option<String>, String);

pub use circulation::{FoundHold, FoundLoan, HoldState};

/// What the Elzevir holds under Koha's numbers, read from the edge tables (item 2).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Footprint {
    /// Branch code, and the branch's name.
    pub branches: BTreeMap<String, String>,
    /// Patron category codes.
    pub categories: BTreeSet<String>,
    /// Patrons, by `borrowernumber`.
    pub patrons: BTreeMap<String, FoundPatron>,
    /// How many Instances each `biblionumber` names.
    pub records: BTreeMap<String, usize>,
    /// Items, by `itemnumber`; more than one is a difference.
    pub items: BTreeMap<String, Vec<FoundItem>>,
    /// Current loans, by the `itemnumber` of the Item lent.
    pub loans: BTreeMap<String, Vec<FoundLoan>>,
    /// Current holds, by the `biblionumber` of the Instance, in priority order.
    pub holds: BTreeMap<String, Vec<FoundHold>>,
    /// Patron accounts, by `borrowernumber`: the balance in cents and the number of lines.
    pub accounts: BTreeMap<String, (i64, usize)>,
    /// Returned loans, by `issue_id`: whether the patron is kept with it.
    pub returned: BTreeMap<String, bool>,
    /// The Koha item type codes of the circulation policies.
    pub item_types: BTreeSet<String>,
    /// Circulation rules and their values. A dimension the Elzevir has without a Koha code
    /// is named `#` and its id, so that it never passes for "any".
    pub rules: BTreeMap<RuleKey, String>,
    /// The calendar entries of each branch that has a Koha code, by the code.
    pub holidays: BTreeMap<String, Vec<Holiday>>,
}

/// A patron as the Elzevir holds it, with its branch and category as Koha codes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FoundPatron {
    /// Library card number.
    pub cardnumber: String,
    /// The branch's Koha code.
    pub branch: Option<String>,
    /// The category's Koha code.
    pub category: Option<String>,
    /// Anonymized after the migration (item 6).
    pub anonymized: bool,
}

/// An Item as the Elzevir holds it, with its places as Koha codes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FoundItem {
    /// The `biblionumber` of its Instance.
    pub biblionumber: Option<String>,
    /// Barcode.
    pub barcode: Option<String>,
    /// Home branch code.
    pub home: Option<String>,
    /// Holding branch code.
    pub holding: Option<String>,
    /// Item type code.
    pub item_type: Option<String>,
}

/// A table the verifier compares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Table {
    /// `branches`, by `branchcode`.
    Branches,
    /// `categories`, by `categorycode`.
    Categories,
    /// `borrowers`, by `borrowernumber`.
    Patrons,
    /// `biblio_metadata`, by `biblionumber`.
    Records,
    /// `items`, by `itemnumber`.
    Items,
    /// `issues`, by `issue_id`.
    Loans,
    /// `reserves`, by `reserve_id`.
    Holds,
    /// The accounts of `accountlines`, by `borrowernumber`.
    Accounts,
    /// `old_issues`, by `issue_id`.
    Returned,
    /// `circulation_rules`, by `id`.
    Rules,
    /// `repeatable_holidays`, by `id`.
    RepeatableHolidays,
    /// `special_holidays`, by `id`.
    SpecialHolidays,
}

/// Why a number of the dump is not in the Elzevir, found in the dump or the database
/// (item 10).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Cause {
    /// The patron migration's cleaning left the patron out.
    Patron(Leftover),
    /// An Item of the record names a branch the Elzevir does not have.
    UnknownBranch(String),
    /// The Item's record did not come.
    RecordLeftOut,
    /// Another Item of the dump has the same barcode.
    RepeatedBarcode,
    /// The patron came and was anonymized afterwards.
    Anonymized,
    /// The migration of loans and holds left the line out for what the dump lacks.
    Circulation(CirculationLeftover),
    /// The patron did not come.
    PatronLeftOut,
    /// The Item did not come.
    ItemLeftOut,
    /// Another loan of the dump has the same Item and came.
    ItemOnLoan,
    /// An earlier hold of the dump has the same patron and record.
    AlreadyHeld,
    /// The record's MARCXML cannot be read, so the switch set it aside.
    UnreadableRecord,
    /// The rule cannot be set, as `circ.set_rules` checks it.
    Rule(RuleLeftover),
    /// The rule names a code of this column that the Elzevir does not have.
    UnknownCode(&'static str),
    /// The calendar row cannot come as it is, or joins an earlier row of the same day.
    Holiday(HolidayLeftover),
}

/// A difference without a cause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unexplained {
    /// In the dump, not in the Elzevir.
    Missing,
    /// In the Elzevir more than once.
    Repeated,
    /// In the Elzevir, not among what the dump brings.
    Extra,
    /// In the Elzevir and not in the dump, named by this other number (a loan by its
    /// Item, a hold by its record), since the Elzevir does not keep Koha's own.
    ExtraBy(&'static str),
    /// In both, with this column different.
    Field(&'static str),
    /// A record of the dump without `999 $c`.
    NoNumber,
}

/// The comparison of one table (item 11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableCheck {
    /// The table.
    pub table: Table,
    /// Numbers in the dump.
    pub in_dump: usize,
    /// Numbers of the dump found in the Elzevir.
    pub arrived: usize,
    /// Numbers with a cause, by cause.
    pub explained: BTreeMap<Cause, Vec<String>>,
    /// Each difference without a cause, by Koha number, in number order.
    pub unexplained: Vec<(String, Unexplained)>,
}

impl TableCheck {
    pub(super) fn new(table: Table) -> Self {
        Self {
            table,
            in_dump: 0,
            arrived: 0,
            explained: BTreeMap::new(),
            unexplained: Vec::new(),
        }
    }

    pub(super) fn explain(&mut self, cause: Cause, number: &str) {
        self.explained
            .entry(cause)
            .or_default()
            .push(number.to_owned());
    }

    pub(super) fn differ(&mut self, number: &str, what: Unexplained) {
        self.unexplained.push((number.to_owned(), what));
    }

    /// Numbers left out with a cause, anonymized patrons aside, since they came.
    #[must_use]
    pub fn left_out(&self) -> usize {
        self.explained
            .iter()
            .filter(|(cause, _)| **cause != Cause::Anonymized)
            .map(|(_, numbers)| numbers.len())
            .sum()
    }
}

/// Compares the dump, read as the migration and the switch read it, with `found`.
#[must_use]
pub fn verify(
    plan: &KohaPlan,
    catalog: &[KohaCatalogRecord],
    found: &Footprint,
) -> Vec<TableCheck> {
    vec![
        branches(plan, found),
        categories(plan, found),
        patrons(plan, found),
        records(catalog, found),
        items(catalog, found),
        circulation::loans(plan, found),
        circulation::holds(plan, found),
        circulation::accounts(plan, found),
        circulation::returned(plan, found),
        rules::rules(plan, found),
        calendar::holidays(plan, found, HolidayTable::Repeatable),
        calendar::holidays(plan, found, HolidayTable::Special),
    ]
}

/// The numbers `found` holds that the dump does not bring (item 9).
pub(super) fn extras<'a, V>(
    check: &mut TableCheck,
    found: impl Iterator<Item = (&'a String, V)>,
    seen: &BTreeSet<&str>,
) {
    for (number, _) in found {
        if !seen.contains(number.as_str()) {
            check.differ(number, Unexplained::Extra);
        }
    }
}

pub(super) fn sorted(mut check: TableCheck) -> TableCheck {
    check.unexplained.sort_by(|a, b| by_number(&a.0, &b.0));
    for numbers in check.explained.values_mut() {
        numbers.sort_by(|a, b| by_number(a, b));
    }
    check
}

/// Koha's numbers in numeric order, codes in text order after them.
fn by_number(a: &str, b: &str) -> std::cmp::Ordering {
    match (a.parse::<u64>(), b.parse::<u64>()) {
        (Ok(a), Ok(b)) => a.cmp(&b),
        _ => a.cmp(b),
    }
}

fn branches(plan: &KohaPlan, found: &Footprint) -> TableCheck {
    let mut check = TableCheck::new(Table::Branches);
    let mut seen = BTreeSet::new();
    for branch in &plan.branches {
        check.in_dump += 1;
        seen.insert(branch.code.as_str());
        match found.branches.get(&branch.code) {
            None => check.differ(&branch.code, Unexplained::Missing),
            Some(name) => {
                check.arrived += 1;
                if *name != branch.name {
                    check.differ(&branch.code, Unexplained::Field("branchname"));
                }
            }
        }
    }
    extras(&mut check, found.branches.iter(), &seen);
    sorted(check)
}

fn categories(plan: &KohaPlan, found: &Footprint) -> TableCheck {
    let mut check = TableCheck::new(Table::Categories);
    let mut seen = BTreeSet::new();
    for category in &plan.categories {
        check.in_dump += 1;
        seen.insert(category.code.as_str());
        if found.categories.contains(&category.code) {
            check.arrived += 1;
        } else {
            check.differ(&category.code, Unexplained::Missing);
        }
    }
    extras(&mut check, found.categories.iter().map(|c| (c, ())), &seen);
    sorted(check)
}

fn patrons(plan: &KohaPlan, found: &Footprint) -> TableCheck {
    let mut check = TableCheck::new(Table::Patrons);
    let mut seen = BTreeSet::new();
    let left: Vec<(String, Leftover)> = plan
        .left
        .iter()
        .map(|(number, why)| (number.to_string(), *why))
        .collect();
    for (number, why) in &left {
        check.in_dump += 1;
        seen.insert(number.as_str());
        if found.patrons.contains_key(number) {
            check.differ(number, Unexplained::Extra);
        } else {
            check.explain(Cause::Patron(*why), number);
        }
    }
    let numbers: Vec<String> = plan
        .patrons
        .iter()
        .map(|patron| patron.borrowernumber.to_string())
        .collect();
    for (patron, number) in plan.patrons.iter().zip(&numbers) {
        check.in_dump += 1;
        seen.insert(number.as_str());
        let Some(stored) = found.patrons.get(number) else {
            check.differ(number, Unexplained::Missing);
            continue;
        };
        check.arrived += 1;
        if stored.anonymized {
            check.explain(Cause::Anonymized, number);
            continue;
        }
        if stored.cardnumber != patron.cardnumber {
            check.differ(number, Unexplained::Field("cardnumber"));
        }
        if stored.branch.as_deref() != Some(patron.branch.as_str()) {
            check.differ(number, Unexplained::Field("branchcode"));
        }
        if stored.category.as_deref() != Some(patron.category.as_str()) {
            check.differ(number, Unexplained::Field("categorycode"));
        }
    }
    extras(&mut check, found.patrons.iter(), &seen);
    sorted(check)
}

/// The record's Items whose branches the Elzevir lacks, the first one as the cause.
fn unknown_branch(record: &KohaCatalogRecord, found: &Footprint) -> Option<String> {
    record
        .items
        .iter()
        .flat_map(|item| [&item.home, &item.holding])
        .flatten()
        .find(|code| !found.branches.contains_key(*code))
        .cloned()
}

fn records(catalog: &[KohaCatalogRecord], found: &Footprint) -> TableCheck {
    let mut check = TableCheck::new(Table::Records);
    let mut seen = BTreeSet::new();
    for (index, record) in catalog.iter().enumerate() {
        check.in_dump += 1;
        let Some(number) = &record.biblionumber else {
            check.differ(
                &format!("record {}", index.saturating_add(1)),
                Unexplained::NoNumber,
            );
            continue;
        };
        if !seen.insert(number.as_str()) {
            check.differ(number, Unexplained::Repeated);
            continue;
        }
        match found.records.get(number).copied().unwrap_or(0) {
            0 if record.unreadable => check.explain(Cause::UnreadableRecord, number),
            0 => match unknown_branch(record, found) {
                Some(code) => check.explain(Cause::UnknownBranch(code), number),
                None => check.differ(number, Unexplained::Missing),
            },
            1 => check.arrived += 1,
            _ => {
                check.arrived += 1;
                check.differ(number, Unexplained::Repeated);
            }
        }
    }
    extras(&mut check, found.records.iter(), &seen);
    sorted(check)
}

fn items(catalog: &[KohaCatalogRecord], found: &Footprint) -> TableCheck {
    let mut check = TableCheck::new(Table::Items);
    let mut seen = BTreeSet::new();
    let mut barcodes: BTreeMap<&str, usize> = BTreeMap::new();
    for item in catalog.iter().flat_map(|record| &record.items) {
        if let Some(barcode) = &item.barcode {
            *barcodes.entry(barcode).or_default() += 1;
        }
    }
    for record in catalog {
        let record_in = record
            .biblionumber
            .as_ref()
            .is_some_and(|number| found.records.get(number).copied().unwrap_or(0) > 0);
        // `itemnumber` is the key of `items`: every Item of the dump has one.
        for item in &record.items {
            let Some(number) = &item.itemnumber else {
                continue;
            };
            check.in_dump += 1;
            seen.insert(number.as_str());
            let stored = found.items.get(number).map_or(&[][..], Vec::as_slice);
            let [stored] = stored else {
                if stored.is_empty() {
                    if !record_in {
                        check.explain(Cause::RecordLeftOut, number);
                    } else if item
                        .barcode
                        .as_deref()
                        .is_some_and(|barcode| barcodes.get(barcode).copied().unwrap_or(0) > 1)
                    {
                        check.explain(Cause::RepeatedBarcode, number);
                    } else {
                        check.differ(number, Unexplained::Missing);
                    }
                } else {
                    check.arrived += 1;
                    check.differ(number, Unexplained::Repeated);
                }
                continue;
            };
            check.arrived += 1;
            let fields = [
                ("biblionumber", &stored.biblionumber, &record.biblionumber),
                ("barcode", &stored.barcode, &item.barcode),
                ("homebranch", &stored.home, &item.home),
            ];
            for (name, stored, dumped) in fields {
                if stored != dumped {
                    check.differ(number, Unexplained::Field(name));
                }
            }
            // Without a value in the dump the import fills them (the holding branch from the
            // home one, the type from the record), so only a value Koha gave is compared.
            let optional = [
                ("holdingbranch", &stored.holding, &item.holding),
                ("itype", &stored.item_type, &item.item_type),
            ];
            for (name, stored, dumped) in optional {
                if dumped.is_some() && stored != dumped {
                    check.differ(number, Unexplained::Field(name));
                }
            }
        }
    }
    extras(&mut check, found.items.iter(), &seen);
    sorted(check)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::koha_sql::KohaCatalogItem;

    fn plan() -> KohaPlan {
        let dump = include_str!("../../tests/data/koha-patrons.sql");
        let zone = jiff::tz::TimeZone::get("America/Sao_Paulo").unwrap();
        crate::koha_migration::plan(dump.as_bytes(), jiff::civil::date(2026, 11, 27), &zone)
            .unwrap()
    }

    /// What a faithful migration of `plan` and `catalog` leaves in the Elzevir.
    fn faithful(plan: &KohaPlan, catalog: &[KohaCatalogRecord]) -> Footprint {
        let mut found = Footprint {
            branches: plan
                .branches
                .iter()
                .map(|b| (b.code.clone(), b.name.clone()))
                .collect(),
            categories: plan.categories.iter().map(|c| c.code.clone()).collect(),
            patrons: plan
                .patrons
                .iter()
                .map(|p| {
                    let patron = FoundPatron {
                        cardnumber: p.cardnumber.clone(),
                        branch: Some(p.branch.clone()),
                        category: Some(p.category.clone()),
                        anonymized: false,
                    };
                    (p.borrowernumber.to_string(), patron)
                })
                .collect(),
            ..Footprint::default()
        };
        for record in catalog {
            let number = record.biblionumber.clone().unwrap();
            found.records.insert(number.clone(), 1);
            for item in &record.items {
                let stored = FoundItem {
                    biblionumber: Some(number.clone()),
                    barcode: item.barcode.clone(),
                    home: item.home.clone(),
                    holding: item.holding.clone(),
                    item_type: item.item_type.clone(),
                };
                found
                    .items
                    .insert(item.itemnumber.clone().unwrap(), vec![stored]);
            }
        }
        circulate(plan, &mut found);
        for rule in &plan.rules {
            found.item_types.extend(rule.item_type.clone());
            let key = (
                rule.branch.clone(),
                rule.category.clone(),
                rule.item_type.clone(),
                rule.name.clone(),
            );
            found.rules.insert(key, rule.value.clone());
        }
        for holiday in &plan.holidays {
            found
                .holidays
                .entry(holiday.branch.clone())
                .or_default()
                .push(Holiday {
                    when: holiday.when().unwrap(),
                    text: holiday.text().unwrap().clone(),
                });
        }
        found
    }

    fn holiday(
        table: HolidayTable,
        id: u64,
        branch: &str,
        action: crate::calendar::CalendarAction,
    ) -> crate::koha_migration::KohaHoliday {
        crate::koha_migration::KohaHoliday {
            table,
            id,
            branch: branch.to_owned(),
            action,
            untitled: false,
            cut: false,
        }
    }

    #[test]
    fn each_calendar_row_is_compared_in_its_branch_by_its_day() {
        use crate::calendar::{CalendarAction, HolidayText, HolidayWhen};
        use elzevir_domain::calendar::{AnnualHoliday, DatedEntry};
        let text = |title: &str| HolidayText {
            title: title.to_owned(),
            description: None,
        };
        let day = |d| jiff::civil::date(2026, 12, d);
        let dated = |d, entry| CalendarAction::SetDates {
            first: day(d),
            last: day(d),
            entry,
            text: text("Fechado"),
        };
        let mut plan = plan();
        plan.holidays = vec![
            holiday(
                HolidayTable::Repeatable,
                1,
                "CTR",
                CalendarAction::CloseWeekday {
                    weekday: 0,
                    text: text("Domingo"),
                },
            ),
            holiday(
                HolidayTable::Repeatable,
                2,
                "CTR",
                CalendarAction::CloseAnnually {
                    holiday: AnnualHoliday::new(12, 25).unwrap(),
                    text: text("Natal"),
                },
            ),
            holiday(
                HolidayTable::Repeatable,
                3,
                "LUA",
                CalendarAction::CloseWeekday {
                    weekday: 6,
                    text: text("Sábado"),
                },
            ),
            holiday(
                HolidayTable::Special,
                1,
                "CTR",
                dated(24, DatedEntry::Closed),
            ),
            holiday(HolidayTable::Special, 2, "CTR", dated(26, DatedEntry::Open)),
            holiday(
                HolidayTable::Special,
                3,
                "CTR",
                dated(31, DatedEntry::Closed),
            ),
        ];
        plan.holidays_left = vec![(HolidayTable::Special, 4, HolidayLeftover::SameDay)];
        let mut found = faithful(&plan, &[]);
        found.holidays.remove("LUA");
        let ctr = found.holidays.get_mut("CTR").unwrap();
        ctr[1].text.title = "Natal!".to_owned();
        ctr[3].when = HolidayWhen::Dated(day(26), DatedEntry::Closed);
        ctr.remove(4);
        ctr.push(Holiday {
            when: HolidayWhen::Weekly(3),
            text: text("Quarta"),
        });
        let checks = verify(&plan, &[], &found);
        let repeatable = check(&checks, Table::RepeatableHolidays);
        assert_eq!((repeatable.in_dump, repeatable.arrived), (3, 2));
        assert_eq!(
            repeatable.explained,
            BTreeMap::from([(Cause::UnknownBranch("LUA".to_owned()), vec!["3".to_owned()])])
        );
        assert_eq!(
            repeatable.unexplained,
            vec![
                ("2".to_owned(), Unexplained::Field("title")),
                ("CTR weekday 3".to_owned(), Unexplained::ExtraBy("entry")),
            ]
        );
        let special = check(&checks, Table::SpecialHolidays);
        assert_eq!((special.in_dump, special.arrived), (4, 2));
        assert_eq!(
            special.explained,
            BTreeMap::from([(
                Cause::Holiday(HolidayLeftover::SameDay),
                vec!["4".to_owned()]
            )])
        );
        assert_eq!(
            special.unexplained,
            vec![
                ("2".to_owned(), Unexplained::Field("isexception")),
                ("3".to_owned(), Unexplained::Missing),
            ]
        );
    }

    fn rule(
        id: u64,
        branch: Option<&str>,
        item_type: Option<&str>,
        value: &str,
    ) -> crate::koha_migration::KohaRule {
        crate::koha_migration::KohaRule {
            id,
            branch: branch.map(str::to_owned),
            category: None,
            item_type: item_type.map(str::to_owned),
            name: "issuelength".to_owned(),
            value: value.to_owned(),
        }
    }

    #[test]
    fn each_rule_is_compared_at_its_scope_by_koha_codes() {
        let mut plan = plan();
        plan.rules = vec![
            rule(1, None, None, "21"),
            rule(2, Some("CTR"), Some("LIV"), "14"),
            rule(3, Some("LUA"), None, "7"),
            rule(4, None, Some("REV"), "7"),
            rule(5, Some("NRT"), None, "10"),
        ];
        plan.rules_left = vec![(6, RuleLeftover::UnknownKind)];
        let mut found = faithful(&plan, &[]);
        for id in [3, 4] {
            let lost = &plan.rules[id - 1];
            found.rules.remove(&(
                lost.branch.clone(),
                None,
                lost.item_type.clone(),
                lost.name.clone(),
            ));
        }
        found.item_types.remove("REV");
        *found
            .rules
            .get_mut(&(None, None, None, "issuelength".to_owned()))
            .unwrap() = "20".to_owned();
        found
            .rules
            .remove(&(Some("NRT".to_owned()), None, None, "issuelength".to_owned()));
        found.rules.insert(
            (Some("#9".to_owned()), None, None, "issuelength".to_owned()),
            "5".to_owned(),
        );
        let checks = verify(&plan, &[], &found);
        let rules = check(&checks, Table::Rules);
        assert_eq!((rules.in_dump, rules.arrived), (6, 2));
        assert_eq!(
            rules.explained,
            BTreeMap::from([
                (Cause::UnknownBranch("LUA".to_owned()), vec!["3".to_owned()]),
                (Cause::Rule(RuleLeftover::UnknownKind), vec!["6".to_owned()]),
                (Cause::UnknownCode("itemtype"), vec!["4".to_owned()]),
            ])
        );
        assert_eq!(
            rules.unexplained,
            vec![
                ("1".to_owned(), Unexplained::Field("rule_value")),
                ("5".to_owned(), Unexplained::Missing),
                (
                    "issuelength (#9, *, *)".to_owned(),
                    Unexplained::ExtraBy("rule")
                ),
            ]
        );
    }

    /// The loans, holds, accounts and returned loans a faithful migration writes, given
    /// the patrons, records and Items in `found`.
    fn circulate(plan: &KohaPlan, found: &mut Footprint) {
        let patron_in =
            |found: &Footprint, number: u64| found.patrons.contains_key(&number.to_string());
        let item_in =
            |found: &Footprint, number: u64| found.items.contains_key(&number.to_string());
        for loan in &plan.loans {
            let item = loan.item.itemnumber.to_string();
            if patron_in(found, loan.borrowernumber)
                && item_in(found, loan.item.itemnumber)
                && found.branches.contains_key(&loan.branch)
                && !found.loans.contains_key(&item)
            {
                let stored = FoundLoan {
                    borrowernumber: Some(loan.borrowernumber.to_string()),
                    due: loan.due.clone(),
                };
                found.loans.insert(item, vec![stored]);
            }
        }
        for hold in &plan.holds {
            let record = hold.biblionumber.to_string();
            let borrower = Some(hold.borrowernumber.to_string());
            let needs_item = hold.item_level
                || !matches!(hold.state, crate::koha_migration::KohaHoldState::Queued(_));
            let queue = found.holds.get(&record).map_or(0, Vec::len);
            if !patron_in(found, hold.borrowernumber)
                || !found.records.contains_key(&record)
                || (needs_item
                    && hold
                        .item
                        .as_ref()
                        .is_some_and(|i| !item_in(found, i.itemnumber)))
                || !found.branches.contains_key(&hold.pickup)
                || found
                    .holds
                    .get(&record)
                    .is_some_and(|holds| holds.iter().any(|h| h.borrowernumber == borrower))
            {
                continue;
            }
            let state = HoldState::from(hold.state);
            let priority = if state == HoldState::Queued {
                i64::try_from(queue).unwrap() + 1
            } else {
                0
            };
            found.holds.entry(record).or_default().push(FoundHold {
                borrowernumber: borrower,
                state,
                priority,
            });
        }
        for account in &plan.accounts {
            if patron_in(found, account.borrowernumber) && !account.lines.is_empty() {
                let balance = account.lines.iter().map(|line| line.outstanding.0).sum();
                found.accounts.insert(
                    account.borrowernumber.to_string(),
                    (balance, account.lines.len()),
                );
            }
        }
        for loan in &plan.returned {
            if item_in(found, loan.item.itemnumber) {
                let kept = loan.borrowernumber.is_some_and(|number| {
                    patron_in(found, number)
                        && plan.patrons.iter().any(|p| {
                            p.borrowernumber == number
                                && p.reading_history == crate::patrons::ReadingHistory::Kept
                        })
                });
                found.returned.insert(loan.issue_id.to_string(), kept);
            }
        }
    }

    fn item(number: &str, barcode: &str, branch: &str) -> KohaCatalogItem {
        KohaCatalogItem {
            itemnumber: Some(number.to_owned()),
            barcode: Some(barcode.to_owned()),
            home: Some(branch.to_owned()),
            holding: Some(branch.to_owned()),
            item_type: Some("BK".to_owned()),
        }
    }

    fn catalog() -> Vec<KohaCatalogRecord> {
        vec![
            KohaCatalogRecord {
                biblionumber: Some("10".to_owned()),
                unreadable: false,
                items: vec![item("100", "B100", "CTR"), item("101", "B101", "CTR")],
            },
            KohaCatalogRecord {
                biblionumber: Some("11".to_owned()),
                unreadable: false,
                items: vec![item("110", "B110", "NRT")],
            },
        ]
    }

    fn check(checks: &[TableCheck], table: Table) -> &TableCheck {
        checks.iter().find(|check| check.table == table).unwrap()
    }

    #[test]
    fn a_faithful_migration_has_only_explained_differences() {
        let plan = plan();
        let catalog = catalog();
        let checks = verify(&plan, &catalog, &faithful(&plan, &catalog));
        for check in &checks {
            assert!(check.unexplained.is_empty(), "{check:?}");
        }
        let patrons = check(&checks, Table::Patrons);
        assert_eq!(
            (patrons.in_dump, patrons.arrived, patrons.left_out()),
            (6, 3, 3)
        );
        assert_eq!(
            patrons
                .explained
                .get(&Cause::Patron(Leftover::UnknownBranch)),
            Some(&vec!["4".to_owned()])
        );
        assert_eq!(check(&checks, Table::Items).arrived, 3);
        let loans = check(&checks, Table::Loans);
        assert!(loans.arrived > 0, "{loans:?}");
        // Issues 2 and 4 lend Items 120 and 999, which this catalog lacks; issue 5 lends
        // Item 100 again.
        assert_eq!(
            loans.explained.get(&Cause::ItemLeftOut),
            Some(&vec!["2".to_owned(), "4".to_owned()])
        );
        assert_eq!(
            loans.explained.get(&Cause::ItemOnLoan),
            Some(&vec!["5".to_owned()])
        );
        assert!(check(&checks, Table::Holds).arrived > 0);
        assert!(check(&checks, Table::Accounts).arrived > 0);
        assert!(check(&checks, Table::Returned).arrived > 0);
    }

    #[test]
    fn each_circulation_difference_is_named_by_its_koha_number() {
        let plan = plan();
        let catalog = catalog();
        let mut found = faithful(&plan, &catalog);
        // A loan of the dump lost, another with a different due date, one the dump lacks.
        let (item, mut lost) = found.loans.pop_first().unwrap();
        let loan = plan
            .loans
            .iter()
            .find(|loan| loan.item.itemnumber.to_string() == item)
            .unwrap();
        lost[0].due = "2000-01-01 00:00:00".to_owned();
        found.loans.insert(item.clone(), lost);
        found
            .loans
            .insert("555".to_owned(), vec![FoundLoan::default()]);
        // A balance off by a cent, a returned loan that kept a patron it should not.
        let (patron, (balance, lines)) = found.accounts.pop_first().unwrap();
        found.accounts.insert(patron.clone(), (balance + 1, lines));
        let (issue, kept) = found.returned.pop_first().unwrap();
        found.returned.insert(issue.clone(), !kept);
        // The queue of record 10 (reserves 1 and 2) in the other order.
        for hold in found.holds.get_mut("10").unwrap() {
            hold.priority = -hold.priority;
        }
        let checks = verify(&plan, &catalog, &found);
        let loans = check(&checks, Table::Loans);
        assert_eq!(
            loans.unexplained,
            vec![
                (loan.issue_id.to_string(), Unexplained::Field("date_due")),
                ("555".to_owned(), Unexplained::ExtraBy("itemnumber")),
            ]
        );
        assert_eq!(
            check(&checks, Table::Accounts).unexplained,
            vec![(patron, Unexplained::Field("amountoutstanding"))]
        );
        assert_eq!(
            check(&checks, Table::Returned).unexplained,
            vec![(issue, Unexplained::Field("borrowernumber"))]
        );
        assert_eq!(
            check(&checks, Table::Holds).unexplained,
            vec![
                ("1".to_owned(), Unexplained::Field("priority")),
                ("2".to_owned(), Unexplained::Field("priority")),
            ]
        );
    }

    #[test]
    fn each_patron_difference_is_named_by_its_number_and_column() {
        let plan = plan();
        let mut found = faithful(&plan, &[]);
        found.patrons.get_mut("1").unwrap().cardnumber = "X".to_owned();
        found.patrons.get_mut("2").unwrap().category = None;
        found.patrons.get_mut("6").unwrap().anonymized = true;
        found.patrons.get_mut("6").unwrap().cardnumber = "anonymous".to_owned();
        let left = found.patrons.get("1").unwrap().clone();
        found.patrons.insert("3".to_owned(), left.clone());
        found.patrons.insert("99".to_owned(), left);
        found.categories.insert("LOCAL".to_owned());
        found.branches.remove("NRT");
        let checks = verify(&plan, &[], &found);
        let patrons = check(&checks, Table::Patrons);
        assert_eq!(
            patrons.unexplained,
            vec![
                ("1".to_owned(), Unexplained::Field("cardnumber")),
                ("2".to_owned(), Unexplained::Field("categorycode")),
                ("3".to_owned(), Unexplained::Extra),
                ("99".to_owned(), Unexplained::Extra),
            ]
        );
        assert_eq!(
            patrons.explained.get(&Cause::Anonymized),
            Some(&vec!["6".to_owned()])
        );
        assert_eq!(
            check(&checks, Table::Categories).unexplained,
            vec![("LOCAL".to_owned(), Unexplained::Extra)]
        );
        assert_eq!(
            check(&checks, Table::Branches).unexplained,
            vec![("NRT".to_owned(), Unexplained::Missing)]
        );
    }

    #[test]
    fn records_and_items_left_out_are_explained_by_the_dump_or_the_database() {
        let plan = plan();
        let mut catalog = catalog();
        catalog.push(KohaCatalogRecord {
            biblionumber: Some("12".to_owned()),
            unreadable: false,
            items: vec![item("120", "B120", "LUA")],
        });
        catalog.push(KohaCatalogRecord {
            biblionumber: Some("13".to_owned()),
            unreadable: false,
            items: vec![item("130", "B100", "CTR")],
        });
        let mut found = faithful(&plan, &catalog);
        // 12 names a branch the Elzevir lacks; 130 repeats the barcode of 100.
        found.records.remove("12");
        found.items.remove("120");
        found.items.remove("130");
        found.branches.remove("LUA");
        let checks = verify(&plan, &catalog, &found);
        let records = check(&checks, Table::Records);
        assert!(records.unexplained.is_empty(), "{records:?}");
        assert_eq!(
            records
                .explained
                .get(&Cause::UnknownBranch("LUA".to_owned())),
            Some(&vec!["12".to_owned()])
        );
        let items = check(&checks, Table::Items);
        assert!(items.unexplained.is_empty(), "{items:?}");
        assert_eq!(
            items.explained.get(&Cause::RecordLeftOut),
            Some(&vec!["120".to_owned()])
        );
        assert_eq!(
            items.explained.get(&Cause::RepeatedBarcode),
            Some(&vec!["130".to_owned()])
        );
    }

    #[test]
    fn a_record_set_aside_as_unreadable_is_explained_and_only_when_absent() {
        let plan = plan();
        let mut catalog = catalog();
        catalog.push(KohaCatalogRecord {
            biblionumber: Some("12".to_owned()),
            unreadable: true,
            items: vec![KohaCatalogItem {
                itemnumber: Some("120".to_owned()),
                ..KohaCatalogItem::default()
            }],
        });
        let mut found = faithful(&plan, &catalog);
        found.records.remove("12");
        found.items.remove("120");
        let checks = verify(&plan, &catalog, &found);
        let records = check(&checks, Table::Records);
        assert!(records.unexplained.is_empty(), "{records:?}");
        assert_eq!(
            records.explained,
            BTreeMap::from([(Cause::UnreadableRecord, vec!["12".to_owned()])])
        );
        let items = check(&checks, Table::Items);
        assert!(items.unexplained.is_empty(), "{items:?}");
        assert_eq!(
            items.explained.get(&Cause::RecordLeftOut),
            Some(&vec!["120".to_owned()])
        );
        // A readable record that is missing is still a difference.
        catalog[2].unreadable = false;
        let checks = verify(&plan, &catalog, &found);
        assert!(!check(&checks, Table::Records).unexplained.is_empty());
    }

    #[test]
    fn each_item_difference_is_named_by_its_number_and_column() {
        let plan = plan();
        let catalog = catalog();
        let mut found = faithful(&plan, &catalog);
        found.records.remove("11");
        found.records.insert("12".to_owned(), 2);
        let stored = &mut found.items.get_mut("100").unwrap()[0];
        stored.barcode = Some("other".to_owned());
        stored.holding = Some("NRT".to_owned());
        let again = found.items.get("101").unwrap()[0].clone();
        found.items.get_mut("101").unwrap().push(again);
        found.items.get_mut("110").unwrap()[0].item_type = None;
        let checks = verify(&plan, &catalog, &found);
        assert_eq!(
            check(&checks, Table::Records).unexplained,
            vec![
                ("11".to_owned(), Unexplained::Missing),
                ("12".to_owned(), Unexplained::Extra),
            ]
        );
        assert_eq!(
            check(&checks, Table::Items).unexplained,
            vec![
                ("100".to_owned(), Unexplained::Field("barcode")),
                ("100".to_owned(), Unexplained::Field("holdingbranch")),
                ("101".to_owned(), Unexplained::Repeated),
                ("110".to_owned(), Unexplained::Field("itype")),
            ]
        );
    }
}
