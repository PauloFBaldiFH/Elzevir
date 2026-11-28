// SPDX-License-Identifier: GPL-3.0-or-later
//! The migration of branches, patron categories, patrons and their restrictions from a
//! whole Koha database dump (docs/specs/migracao/leitores-do-koha.md). Koha's codes stay
//! at this edge: the writer keeps them in its edge tables, never in the core.

use std::collections::BTreeMap;
use std::io::BufRead;

use elzevir_domain::catalog::BranchId;
use elzevir_domain::patron::{Enrolment, MAX_ENROLMENT_MONTHS, PatronCategoryId};
use elzevir_domain::suspension::RestrictionKind;
use jiff::civil::{Date, DateTime};
use jiff::tz::TimeZone;
use serde::{Deserialize, Serialize};

use crate::import::koha_sql::{self, DumpError};
use crate::patron_account::is_koha_hash;
use crate::patrons::{
    MAX_CARDNUMBER, PatronField, PatronFields, PatronRefusal, ReadingHistory, checked_patron,
};

mod accounts;
mod attributes;
mod calendar;
mod circulation;
mod dump;
mod history;
mod item_statuses;
mod rules;

pub use accounts::{
    ACCOUNT_LINES_PER_COMMAND, AccountsRead, KohaAccount, KohaAccountBatch, KohaAccountLine,
    KohaAccountsMigrated, KohaOffset,
};

pub use attributes::{
    AttributesRead, KohaAttributeValue, KohaAttributesBatch, KohaAttributesMigrated, KohaFieldType,
    SETTINGS_NOT_KEPT, VALUES_PER_COMMAND, ValueLeftover,
};

pub use calendar::{
    HOLIDAYS_PER_COMMAND, HolidayLeftover, HolidayTable, KohaCalendarBatch, KohaCalendarMigrated,
    KohaHoliday, UNTITLED, holiday_name,
};

pub use circulation::{
    CIRCULATION_PER_COMMAND, CirculationLeftover, KohaCirculationBatch, KohaCirculationMigrated,
    KohaHold, KohaHoldState, KohaItem, KohaLoan,
};

use dump::{KohaTables, Row, Table};
pub use dump::{LEFT_OUT_COLUMNS, MAX_KOHA_ROWS};
pub use history::{KohaHistoryBatch, KohaHistoryMigrated, KohaReturnedLoan, RETURNED_PER_COMMAND};
pub use item_statuses::{
    KohaStatusDates, KohaStatusName, KohaStatusesBatch, KohaStatusesMigrated,
    STATUS_DATES_PER_COMMAND, numbered_label,
};
pub use rules::{
    KohaRule, KohaRulesBatch, KohaRulesMigrated, RuleLeftover, checked as checked_rule,
};

/// Most patrons in one write command (item 16).
pub const PATRONS_PER_COMMAND: usize = 500;

/// Months given to a category without a valid period (item 6).
const DEFAULT_MONTHS: u16 = 12;

/// A Koha branch (`branches`), by its code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KohaBranch {
    /// `branchcode`.
    pub code: String,
    /// `branchname`, or the code when empty.
    pub name: String,
    /// The address lines, joined.
    pub address: Option<String>,
    /// `branchphone`.
    pub phone: Option<String>,
    /// `branchemail`.
    pub email: Option<String>,
    /// `branchurl`.
    pub url: Option<String>,
}

/// A Koha patron category (`categories`), by its code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KohaCategory {
    /// `categorycode`.
    pub code: String,
    /// `description`, or the code when empty.
    pub name: String,
    /// The period of enrolment.
    pub enrolment: Enrolment,
}

/// A restriction of a migrated patron, one per kind (item 15).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KohaRestriction {
    /// Its kind.
    pub kind: RestrictionKind,
    /// Its last day; `None` until the staff lift it.
    pub expires: Option<Date>,
    /// When the earliest Koha restriction it joins was created, in the library's zone.
    pub created_at: String,
}

/// A Koha patron, ready for the writer: every field already follows the Elzevir's rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KohaPatron {
    /// `borrowernumber`, kept at the edge for the migration of loans.
    pub borrowernumber: u64,
    /// `branchcode`.
    pub branch: String,
    /// `categorycode`.
    pub category: String,
    /// Library card number.
    pub cardnumber: String,
    /// Surname.
    pub surname: String,
    /// First name.
    pub first_name: Option<String>,
    /// Date of birth.
    pub date_of_birth: Option<Date>,
    /// E-mail address.
    pub email: Option<String>,
    /// Phone number.
    pub phone: Option<String>,
    /// Postal address.
    pub address: Option<String>,
    /// Day of enrolment.
    pub enrolled: Date,
    /// Last day of the enrolment; `None` takes it from the category.
    pub expires: Option<Date>,
    /// Note only the staff sees.
    pub staff_note: Option<String>,
    /// Whether the reading history is kept.
    pub reading_history: ReadingHistory,
    /// The restrictions still holding.
    pub restrictions: Vec<KohaRestriction>,
    /// Koha's bcrypt hash of the OPAC password, kept until the patron signs in (item 12).
    pub password_hash: Option<String>,
}

impl KohaPatron {
    /// The patron's fields, at the branch and category the writer found for the codes.
    #[must_use]
    pub fn fields(&self, branch: BranchId, category: PatronCategoryId) -> PatronFields {
        PatronFields {
            cardnumber: self.cardnumber.clone(),
            surname: self.surname.clone(),
            first_name: self.first_name.clone(),
            cpf: None,
            date_of_birth: self.date_of_birth,
            email: self.email.clone(),
            phone: self.phone.clone(),
            address: self.address.clone(),
            branch,
            category,
            enrolled: self.enrolled,
            expires: self.expires,
            staff_note: self.staff_note.clone(),
            reading_history: self.reading_history,
            extra: Vec::new(),
        }
    }

    fn clear(&mut self, field: PatronField) -> bool {
        match field {
            PatronField::FirstName => self.first_name = None,
            PatronField::DateOfBirth => self.date_of_birth = None,
            PatronField::Email => self.email = None,
            PatronField::Phone => self.phone = None,
            PatronField::Address => self.address = None,
            PatronField::Expires => self.expires = None,
            PatronField::StaffNote => self.staff_note = None,
            PatronField::Cardnumber | PatronField::Surname | PatronField::Cpf => return false,
        }
        true
    }
}

/// One write: the branches and categories (on the first) and a share of the patrons.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KohaBatch {
    /// Branches to create or update.
    pub branches: Vec<KohaBranch>,
    /// Categories to link or create.
    pub categories: Vec<KohaCategory>,
    /// Patrons to add.
    pub patrons: Vec<KohaPatron>,
}

/// What one write did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KohaMigrated {
    /// Branches created.
    pub branches_created: usize,
    /// Branches that had the code and took Koha's data.
    pub branches_updated: usize,
    /// Categories created.
    pub categories_created: usize,
    /// Categories found by name.
    pub categories_linked: usize,
    /// Patrons added.
    pub patrons_added: usize,
    /// Patrons a former run already added.
    pub already_migrated: usize,
    /// Restrictions added.
    pub restrictions: usize,
    /// OPAC passwords brought, also to patrons a former run added without one.
    pub passwords: usize,
    /// Patrons left out because another patron has the card number.
    pub cardnumber_taken: Vec<u64>,
}

impl KohaMigrated {
    /// Adds what another write did.
    pub fn add(&mut self, other: Self) {
        self.branches_created += other.branches_created;
        self.branches_updated += other.branches_updated;
        self.categories_created += other.categories_created;
        self.categories_linked += other.categories_linked;
        self.patrons_added += other.patrons_added;
        self.already_migrated += other.already_migrated;
        self.restrictions += other.restrictions;
        self.passwords += other.passwords;
        self.cardnumber_taken.extend(other.cardnumber_taken);
    }
}

/// Why a Koha patron does not come (item 11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Leftover {
    /// No `borrowernumber`, or not a positive number.
    NoNumber,
    /// No card number, or one longer than the Elzevir keeps.
    Cardnumber,
    /// Neither surname nor first name, or a name longer than the Elzevir keeps.
    Name,
    /// A `branchcode` the dump's `branches` lacks.
    UnknownBranch,
    /// A `categorycode` the dump's `categories` lacks.
    UnknownCategory,
}

/// What the migration reads and changes, before the writes (item 17).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KohaPlan {
    /// The branches.
    pub branches: Vec<KohaBranch>,
    /// The categories.
    pub categories: Vec<KohaCategory>,
    /// The patrons that come.
    pub patrons: Vec<KohaPatron>,
    /// Patrons that do not come, by `borrowernumber` (0 when it had none).
    pub left: Vec<(u64, Leftover)>,
    /// For each field, how many patrons had it emptied for breaking a rule.
    pub cleared: BTreeMap<PatronFieldName, usize>,
    /// Categories given the default period.
    pub default_periods: usize,
    /// Patrons with staff permissions in Koha.
    pub staff: usize,
    /// Restrictions that already ended, not brought.
    pub ended_restrictions: usize,
    /// Passwords in a format older than bcrypt, not brought.
    pub old_passwords: usize,
    /// For each column of `borrowers` not kept, how many patrons had it filled.
    pub left_out: Vec<(&'static str, usize)>,
    /// The current loans.
    pub loans: Vec<KohaLoan>,
    /// The current holds, each Instance's queue in Koha's order.
    pub holds: Vec<KohaHold>,
    /// Loans the dump does not describe well enough, by `issue_id`.
    pub loans_left: Vec<(u64, CirculationLeftover)>,
    /// Holds the dump does not describe well enough, by `reserve_id`.
    pub holds_left: Vec<(u64, CirculationLeftover)>,
    /// Suspended holds, which come suspended.
    pub suspended_holds: usize,
    /// Queued holds with an end date, which comes without it.
    pub holds_with_end: usize,
    /// The patrons' accounts, by `borrowernumber`.
    pub accounts: Vec<KohaAccount>,
    /// What reading the accounts left out or changed.
    pub accounts_read: AccountsRead,
    /// The returned loans, oldest first.
    pub returned: Vec<KohaReturnedLoan>,
    /// Returned loans the dump does not describe well enough, by `issue_id`.
    pub returned_left: Vec<(u64, CirculationLeftover)>,
    /// Circulation rules that come, by `id` (troca-do-koha.md, item 7a).
    pub rules: Vec<KohaRule>,
    /// Circulation rules left out, by `id`.
    pub rules_left: Vec<(u64, RuleLeftover)>,
    /// Calendar entries that come, weekly and annual first (troca-do-koha.md, item 7b).
    pub holidays: Vec<KohaHoliday>,
    /// Calendar rows left out, by table and `id`.
    pub holidays_left: Vec<(HolidayTable, u64, HolidayLeftover)>,
    /// The patron attribute types (patron-attributes.md, item 16).
    pub attribute_types: Vec<KohaFieldType>,
    /// The values the patrons hold.
    pub attribute_values: Vec<KohaAttributeValue>,
    /// What reading the attributes left out or changed.
    pub attributes_read: AttributesRead,
    /// The active currency, cut to the three letters SIP2 sends in `BH`
    /// (patron-attributes.md, item 20).
    pub sip2_currency: Option<String>,
    /// The names of the status numbers (lost-items.md, item 17).
    pub status_names: Vec<KohaStatusName>,
    /// Authorised values of the status categories left out: outside 1 to 99, or unnamed.
    pub status_names_left: usize,
    /// When the Items took their statuses.
    pub status_dates: Vec<KohaStatusDates>,
}

/// A patron field, ordered for the report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PatronFieldName(pub &'static str);

impl KohaPlan {
    /// The writes, at most [`PATRONS_PER_COMMAND`] patrons each; the first carries the
    /// branches and categories, and there is always one.
    #[must_use]
    pub fn batches(&self) -> Vec<KohaBatch> {
        let mut chunks = self.patrons.chunks(PATRONS_PER_COMMAND);
        let mut batches = vec![KohaBatch {
            branches: self.branches.clone(),
            categories: self.categories.clone(),
            patrons: chunks.next().map(<[_]>::to_vec).unwrap_or_default(),
        }];
        batches.extend(chunks.map(|patrons| KohaBatch {
            branches: Vec::new(),
            categories: Vec::new(),
            patrons: patrons.to_vec(),
        }));
        batches
    }
}

/// Reads a whole Koha database dump and turns its four tables into what the writer adds,
/// as of `today` in the library's `zone`.
///
/// # Errors
/// A located, value-free grammar, budget or table failure; nothing is written then.
pub fn plan(input: impl BufRead, today: Date, zone: &TimeZone) -> Result<KohaPlan, DumpError> {
    let tables = koha_sql::read(input, KohaTables::default())?;
    let rows = |table| tables.rows.get(&table).map_or(&[][..], Vec::as_slice);
    let mut plan = KohaPlan {
        branches: rows(Table::Branches).iter().filter_map(branch).collect(),
        left_out: LEFT_OUT_COLUMNS
            .iter()
            .filter_map(|column| Some((*column, *tables.left_out.get(column)?)))
            .collect(),
        ..KohaPlan::default()
    };
    for row in rows(Table::Categories) {
        if let Some((category, default)) = category(row) {
            plan.default_periods += usize::from(default);
            plan.categories.push(category);
        }
    }
    let mut restrictions = restrictions(rows(Table::Debarments), today, zone, &mut plan);
    for row in rows(Table::Borrowers) {
        plan.staff += usize::from(number(text(row, "flags").as_deref()).is_some());
        let password = text(row, "password").filter(|hash| hash != "!");
        let (bcrypt, old): (Option<String>, Option<String>) = match password {
            Some(hash) if is_koha_hash(&hash) => (Some(hash), None),
            other => (None, other),
        };
        match patron(row, today, &plan).and_then(|mut patron| {
            let cleared = clean(&mut patron).map_err(|why| (patron.borrowernumber, why))?;
            Ok((patron, cleared))
        }) {
            Ok((mut patron, cleared)) => {
                for field in cleared {
                    *plan.cleared.entry(PatronFieldName(field)).or_default() += 1;
                }
                patron.restrictions = restrictions
                    .remove(&patron.borrowernumber)
                    .unwrap_or_default();
                patron.password_hash = bcrypt;
                plan.old_passwords += usize::from(old.is_some());
                plan.patrons.push(patron);
            }
            Err(left) => plan.left.push(left),
        }
    }
    circulation::read(&tables.rows, zone, &mut plan);
    accounts::read(&tables.rows, zone, &mut plan);
    history::read(&tables.rows, zone, &mut plan);
    rules::read(&tables.rows, &mut plan);
    calendar::read(&tables.rows, &mut plan);
    attributes::read(&tables.rows, &mut plan);
    plan.sip2_currency = attributes::active_currency(&tables.rows);
    item_statuses::read(&tables.rows, zone, &mut plan);
    Ok(plan)
}

/// The cell `column` as trimmed text; `None` when NULL, empty or not UTF-8.
fn text(row: &Row, column: &str) -> Option<String> {
    let value = row.iter().find(|(name, _)| *name == column)?.1.as_deref()?;
    let value = std::str::from_utf8(value).ok()?.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

/// A positive whole number.
fn number(value: Option<&str>) -> Option<u64> {
    value?.parse().ok().filter(|number| *number > 0)
}

/// A date; `0000-00-00` and anything else not a date is none (item 9).
fn date(value: Option<&str>) -> Option<Date> {
    value?.get(..10)?.parse().ok()
}

/// The filled parts, joined by commas.
fn joined(parts: impl IntoIterator<Item = Option<String>>) -> Option<String> {
    let parts: Vec<String> = parts.into_iter().flatten().collect();
    (!parts.is_empty()).then(|| parts.join(", "))
}

fn branch(row: &Row) -> Option<KohaBranch> {
    let code = text(row, "branchcode")?;
    Some(KohaBranch {
        name: text(row, "branchname").unwrap_or_else(|| code.clone()),
        address: joined(
            [
                "branchaddress1",
                "branchaddress2",
                "branchaddress3",
                "branchcity",
                "branchstate",
                "branchzip",
                "branchcountry",
            ]
            .map(|column| text(row, column)),
        ),
        phone: text(row, "branchphone"),
        email: text(row, "branchemail"),
        url: text(row, "branchurl"),
        code,
    })
}

/// The category, and whether it took the default period.
fn category(row: &Row) -> Option<(KohaCategory, bool)> {
    let code = text(row, "categorycode")?;
    let months = text(row, "enrolmentperiod")
        .and_then(|months| months.parse::<u16>().ok())
        .filter(|months| (1..=MAX_ENROLMENT_MONTHS).contains(months));
    let until = date(text(row, "enrolmentperioddate").as_deref());
    let (enrolment, default) = match (months, until) {
        (Some(months), _) => (Enrolment::Months(months), false),
        (None, Some(day)) => (Enrolment::Until(day), false),
        (None, None) => (Enrolment::Months(DEFAULT_MONTHS), true),
    };
    Some((
        KohaCategory {
            // Names are unique and short in the Elzevir (leitores, item 1).
            name: text(row, "description")
                .unwrap_or_else(|| code.clone())
                .chars()
                .take(crate::patrons::MAX_NAME)
                .collect(),
            code,
            enrolment,
        },
        default,
    ))
}

/// The restrictions still holding on `today`, by patron and kind (items 14 and 15).
fn restrictions(
    rows: &[Row],
    today: Date,
    zone: &TimeZone,
    plan: &mut KohaPlan,
) -> BTreeMap<u64, Vec<KohaRestriction>> {
    // Per patron and kind: the end (`None` without one) and the earliest creation.
    let mut found: BTreeMap<(u64, &'static str), (RestrictionKind, Option<Date>, DateTime)> =
        BTreeMap::new();
    let midnight = today.to_datetime(jiff::civil::Time::midnight());
    for row in rows {
        let Some(patron) = number(text(row, "borrowernumber").as_deref()) else {
            continue;
        };
        let kind = match text(row, "type").as_deref() {
            Some("OVERDUES") => RestrictionKind::Overdues,
            _ => RestrictionKind::Suspension,
        };
        // Koha: `9999-12-31` is a restriction without end.
        let expires = date(text(row, "expiration").as_deref()).filter(|day| day.year() < 9999);
        if expires.is_some_and(|day| day < today) {
            plan.ended_restrictions += 1;
            continue;
        }
        let created = text(row, "created")
            .and_then(|at| at.parse::<DateTime>().ok())
            .unwrap_or(midnight);
        found
            .entry((patron, kind.as_str()))
            .and_modify(|(_, end, earliest)| {
                // Koha: `UpdateBorrowerDebarmentFlags`; no end wins, else the latest.
                *end = end.zip(expires).map(|(a, b)| a.max(b));
                *earliest = (*earliest).min(created);
            })
            .or_insert((kind, expires, created));
    }
    let mut patrons: BTreeMap<u64, Vec<KohaRestriction>> = BTreeMap::new();
    for ((patron, _), (kind, expires, created)) in found {
        let created_at = created
            .to_zoned(zone.clone())
            .map_or_else(|_| created.to_string(), |at| at.to_string());
        patrons.entry(patron).or_default().push(KohaRestriction {
            kind,
            expires,
            created_at,
        });
    }
    patrons
}

/// The patron as Koha has it, or why it does not come (item 11).
fn patron(row: &Row, today: Date, plan: &KohaPlan) -> Result<KohaPatron, (u64, Leftover)> {
    let borrowernumber =
        number(text(row, "borrowernumber").as_deref()).ok_or((0, Leftover::NoNumber))?;
    let left = |why| (borrowernumber, why);
    let cardnumber = text(row, "cardnumber")
        .filter(|card| card.chars().count() <= MAX_CARDNUMBER)
        .ok_or(left(Leftover::Cardnumber))?;
    let mut first_name = text(row, "firstname");
    let surname = match text(row, "surname") {
        Some(surname) => surname,
        None => first_name.take().ok_or(left(Leftover::Name))?,
    };
    let branch = text(row, "branchcode")
        .filter(|code| plan.branches.iter().any(|branch| branch.code == *code))
        .ok_or(left(Leftover::UnknownBranch))?;
    let category = text(row, "categorycode")
        .filter(|code| {
            plan.categories
                .iter()
                .any(|category| category.code == *code)
        })
        .ok_or(left(Leftover::UnknownCategory))?;
    let street = joined([text(row, "streettype"), text(row, "address")])
        .map(|street| street.replace(", ", " "));
    Ok(KohaPatron {
        borrowernumber,
        branch,
        category,
        cardnumber,
        surname,
        first_name,
        date_of_birth: date(text(row, "dateofbirth").as_deref()),
        email: text(row, "email").or_else(|| text(row, "emailpro")),
        phone: text(row, "phone")
            .or_else(|| text(row, "mobile"))
            .or_else(|| text(row, "phonepro")),
        address: joined([
            street,
            text(row, "streetnumber"),
            text(row, "address2"),
            text(row, "city"),
            text(row, "state"),
            text(row, "zipcode"),
            text(row, "country"),
        ]),
        enrolled: date(text(row, "dateenrolled").as_deref()).unwrap_or(today),
        expires: date(text(row, "dateexpiry").as_deref()),
        staff_note: text(row, "borrowernotes"),
        // Koha: 0 is "forever", the patron's own choice; 1, the default, and 2 are not.
        reading_history: if text(row, "privacy").as_deref() == Some("0") {
            ReadingHistory::Kept
        } else {
            ReadingHistory::NotKept
        },
        restrictions: Vec::new(),
        password_hash: None,
    })
}

/// Empties each optional field that breaks a rule, and names them (item 10); a card
/// number or surname that breaks one leaves the patron out (item 11).
fn clean(patron: &mut KohaPatron) -> Result<Vec<&'static str>, Leftover> {
    let mut cleared = Vec::new();
    // The branch and category are the writer's to find; the check does not read them.
    while let Err(PatronRefusal::BadField(field)) =
        checked_patron(patron.fields(BranchId(0), PatronCategoryId(0)))
    {
        if !patron.clear(field) {
            return Err(if field == PatronField::Surname {
                Leftover::Name
            } else {
                Leftover::Cardnumber
            });
        }
        cleared.push(field_name(field));
    }
    Ok(cleared)
}

const fn field_name(field: PatronField) -> &'static str {
    match field {
        PatronField::Cardnumber => "cardnumber",
        PatronField::Surname => "surname",
        PatronField::FirstName => "first_name",
        PatronField::Cpf => "cpf",
        PatronField::DateOfBirth => "date_of_birth",
        PatronField::Email => "email",
        PatronField::Phone => "phone",
        PatronField::Address => "address",
        PatronField::Expires => "expires",
        PatronField::StaffNote => "staff_note",
    }
}
