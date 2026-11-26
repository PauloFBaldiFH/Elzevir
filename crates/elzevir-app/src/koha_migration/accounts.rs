// SPDX-License-Identifier: GPL-3.0-or-later
//! Koha's patron accounts, read for the migration (docs/specs/migracao/circulacao-do-koha.md,
//! items 9 to 13): every line, paid or not, and the offsets between them.

use std::collections::BTreeMap;

use elzevir_domain::account::{CreditType, DebitType};
use elzevir_domain::catalog::Money;
use jiff::tz::TimeZone;
use serde::{Deserialize, Serialize};

use super::circulation::{barcodes, row_item, zoned};
use super::dump::{Row, Table};
use super::{KohaItem, KohaPlan, number, text};
use crate::accounts::LineKind;

/// Most account lines in one write command; a patron's account is never split.
pub const ACCOUNT_LINES_PER_COMMAND: usize = 500;

/// One line of a patron's account (`accountlines`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KohaAccountLine {
    /// `accountlines_id`.
    pub accountlines_id: u64,
    /// The debit or credit type; one this version does not know is `MANUAL` or `CREDIT`.
    pub kind: LineKind,
    /// `status`.
    pub status: Option<String>,
    /// `amount`: positive for a debit, negative for a credit.
    pub amount: Money,
    /// `amountoutstanding`.
    pub outstanding: Money,
    /// The Item it is about (`itemnumber`).
    pub item: Option<KohaItem>,
    /// The Item of the current loan it is about (`issue_id`).
    pub loan: Option<KohaItem>,
    /// `branchcode`.
    pub branch: Option<String>,
    /// `date`, else `timestamp`, in the library's zone.
    pub created_at: String,
    /// `timestamp`, in the library's zone.
    pub updated_at: String,
}

/// One movement between lines (`account_offsets`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KohaOffset {
    /// `credit_id`.
    pub credit: Option<u64>,
    /// `debit_id`.
    pub debit: Option<u64>,
    /// `type`.
    pub kind: String,
    /// `amount`.
    pub amount: Money,
    /// `created_on`, in the library's zone.
    pub created_at: String,
}

/// A patron's whole account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KohaAccount {
    /// `borrowernumber`.
    pub borrowernumber: u64,
    /// The lines, oldest first.
    pub lines: Vec<KohaAccountLine>,
    /// The offsets between them.
    pub offsets: Vec<KohaOffset>,
}

/// One write of accounts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KohaAccountBatch {
    /// The accounts to add.
    pub accounts: Vec<KohaAccount>,
}

/// What a write of accounts did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KohaAccountsMigrated {
    /// Accounts added.
    pub accounts: usize,
    /// Lines added.
    pub lines: usize,
    /// Offsets added.
    pub offsets: usize,
    /// Accounts a former run already added.
    pub already_migrated: usize,
    /// Accounts left out, by `borrowernumber`: the patron did not come.
    pub accounts_left: Vec<u64>,
}

impl KohaAccountsMigrated {
    /// Adds what another write did.
    pub fn add(&mut self, other: Self) {
        self.accounts += other.accounts;
        self.lines += other.lines;
        self.offsets += other.offsets;
        self.already_migrated += other.already_migrated;
        self.accounts_left.extend(other.accounts_left);
    }
}

/// What reading the accounts found besides the accounts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccountsRead {
    /// Lines the dump does not describe well enough, by `accountlines_id` (0 when none).
    pub lines_left: Vec<u64>,
    /// Offsets that name no line that comes, or lack a value.
    pub offsets_left: usize,
    /// Types this version does not know, with how many lines came as `MANUAL` or `CREDIT`.
    pub other_types: BTreeMap<String, usize>,
    /// Amounts with more than cents, rounded.
    pub rounded: usize,
}

/// A Koha `decimal` in cents, rounded half away from zero; whether it was rounded.
fn cents(value: Option<&str>) -> Option<(Money, bool)> {
    let value = value?;
    let (negative, digits) = match value.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, value.strip_prefix('+').unwrap_or(value)),
    };
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    if whole.is_empty() && fraction.is_empty()
        || !whole
            .bytes()
            .chain(fraction.bytes())
            .all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let whole: i64 = if whole.is_empty() {
        0
    } else {
        whole.parse().ok()?
    };
    let digit = |at: usize| i64::from(fraction.as_bytes().get(at).map_or(0, |b| b - b'0'));
    let rest = fraction.get(2..).unwrap_or("");
    let mut amount = whole
        .checked_mul(100)?
        .checked_add(digit(0) * 10 + digit(1))?;
    if digit(2) >= 5 {
        amount = amount.checked_add(1)?;
    }
    let rounded = rest.bytes().any(|b| b != b'0');
    Some((Money(if negative { -amount } else { amount }), rounded))
}

/// The line's type: exactly one of the two codes.
fn kind(row: &Row, other: &mut BTreeMap<String, usize>) -> Option<LineKind> {
    match (text(row, "debit_type_code"), text(row, "credit_type_code")) {
        (Some(code), None) => Some(LineKind::Debit(DebitType::from_code(&code).unwrap_or_else(
            || {
                *other.entry(code).or_default() += 1;
                DebitType::Manual
            },
        ))),
        (None, Some(code)) => Some(LineKind::Credit(
            CreditType::from_code(&code).unwrap_or_else(|| {
                *other.entry(code).or_default() += 1;
                CreditType::Credit
            }),
        )),
        _ => None,
    }
}

/// Reads `accountlines` and `account_offsets` into the plan, by patron.
pub(super) fn read(tables: &BTreeMap<Table, Vec<Row>>, zone: &TimeZone, plan: &mut KohaPlan) {
    let rows = |table| tables.get(&table).map_or(&[][..], Vec::as_slice);
    let barcodes = barcodes(tables);
    let loans: BTreeMap<u64, KohaItem> = rows(Table::Issues)
        .iter()
        .filter_map(|row| {
            Some((
                number(text(row, "issue_id").as_deref())?,
                row_item(row, &barcodes)?,
            ))
        })
        .collect();
    let read = &mut plan.accounts_read;
    let mut accounts: BTreeMap<u64, KohaAccount> = BTreeMap::new();
    let mut owners: BTreeMap<u64, u64> = BTreeMap::new();
    for row in rows(Table::AccountLines) {
        let id = number(text(row, "accountlines_id").as_deref()).unwrap_or(0);
        let Some((borrowernumber, line)) = line(row, id, zone, &barcodes, &loans, read) else {
            read.lines_left.push(id);
            continue;
        };
        owners.insert(id, borrowernumber);
        accounts
            .entry(borrowernumber)
            .or_insert_with(|| KohaAccount {
                borrowernumber,
                lines: Vec::new(),
                offsets: Vec::new(),
            })
            .lines
            .push(line);
    }
    for row in rows(Table::AccountOffsets) {
        let credit = number(text(row, "credit_id").as_deref());
        let debit = number(text(row, "debit_id").as_deref());
        let owner = [credit, debit]
            .into_iter()
            .flatten()
            .find_map(|id| owners.get(&id));
        let offset = cents(text(row, "amount").as_deref()).and_then(|(amount, _)| {
            Some(KohaOffset {
                credit: credit.filter(|id| owners.contains_key(id)),
                debit: debit.filter(|id| owners.contains_key(id)),
                kind: text(row, "type")?,
                amount,
                created_at: zoned(text(row, "created_on").as_deref(), zone)?,
            })
        });
        match (owner.and_then(|owner| accounts.get_mut(owner)), offset) {
            (Some(account), Some(offset)) => account.offsets.push(offset),
            _ => read.offsets_left += 1,
        }
    }
    for account in accounts.values_mut() {
        account.lines.sort_by_key(|line| line.accountlines_id);
    }
    plan.accounts = accounts.into_values().collect();
}

fn line(
    row: &Row,
    id: u64,
    zone: &TimeZone,
    barcodes: &BTreeMap<u64, String>,
    loans: &BTreeMap<u64, KohaItem>,
    read: &mut AccountsRead,
) -> Option<(u64, KohaAccountLine)> {
    let borrowernumber = number(text(row, "borrowernumber").as_deref())?;
    let (amount, amount_rounded) = cents(text(row, "amount").as_deref())?;
    let (outstanding, outstanding_rounded) =
        cents(text(row, "amountoutstanding").as_deref()).unwrap_or((Money(0), false));
    let updated_at = zoned(text(row, "timestamp").as_deref(), zone);
    let created_at = zoned(text(row, "date").as_deref(), zone).or_else(|| updated_at.clone())?;
    let kind = kind(row, &mut read.other_types)?;
    read.rounded += usize::from(amount_rounded || outstanding_rounded);
    Some((
        borrowernumber,
        KohaAccountLine {
            accountlines_id: (id > 0).then_some(id)?,
            kind,
            status: text(row, "status"),
            amount,
            outstanding,
            item: row_item(row, barcodes),
            loan: number(text(row, "issue_id").as_deref())
                .and_then(|issue| loans.get(&issue).cloned()),
            branch: text(row, "branchcode"),
            updated_at: updated_at.unwrap_or_else(|| created_at.clone()),
            created_at,
        },
    ))
}

impl KohaPlan {
    /// The writes of accounts, each whole, about [`ACCOUNT_LINES_PER_COMMAND`] lines each.
    #[must_use]
    pub fn account_batches(&self) -> Vec<KohaAccountBatch> {
        let mut batches = Vec::new();
        let mut batch = KohaAccountBatch::default();
        let mut lines = 0;
        for account in &self.accounts {
            if lines > 0 && lines + account.lines.len() > ACCOUNT_LINES_PER_COMMAND {
                batches.push(std::mem::take(&mut batch));
                lines = 0;
            }
            lines += account.lines.len();
            batch.accounts.push(account.clone());
        }
        if !batch.accounts.is_empty() {
            batches.push(batch);
        }
        batches
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn koha_decimals_become_cents() {
        let money = |value| cents(Some(value));
        assert_eq!(money("5.000000"), Some((Money(500), false)));
        assert_eq!(money("-2.500000"), Some((Money(-250), false)));
        assert_eq!(money("0.333333"), Some((Money(33), true)));
        assert_eq!(money("1.005000"), Some((Money(101), true)));
        assert_eq!(money("12"), Some((Money(1200), false)));
        assert_eq!(money(".5"), Some((Money(50), false)));
        for wrong in ["", "-", ".", "1e3", "1.2.3", "abc"] {
            assert_eq!(money(wrong), None, "{wrong}");
        }
    }
}
