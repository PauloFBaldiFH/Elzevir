// SPDX-License-Identifier: GPL-3.0-or-later
//! Patron and patron category commands (docs/specs/circulacao/leitores.md).

use elzevir_domain::catalog::{BranchId, Money};
use elzevir_domain::patron::{Cpf, Enrolment, MAX_ENROLMENT_MONTHS, PatronCategoryId, PatronId};
use jiff::Zoned;
use jiff::civil::Date;
use serde::{Deserialize, Serialize};

use crate::staff::StaffId;

/// Longest category name, surname or first name.
pub const MAX_NAME: usize = 256;
/// Longest library card number (Koha: `borrowers.cardnumber`, `varchar(32)`).
pub const MAX_CARDNUMBER: usize = 32;
/// Longest e-mail address.
pub const MAX_EMAIL: usize = 254;
/// Longest phone number.
pub const MAX_PHONE: usize = 50;
/// Longest postal address.
pub const MAX_ADDRESS: usize = 512;
/// Longest internal note.
pub const MAX_NOTE: usize = 1000;

/// Every write to patrons and their categories.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PatronCommand {
    /// Creates, changes or deletes a category.
    ChangeCategory(CategoryChange),
    /// Enrols a new patron.
    Add(Box<PatronFields>),
    /// Replaces every field of a patron.
    Update(Box<PatronUpdate>),
    /// Records that a staff member opened the patron's record; changes nothing else
    /// (interface §18.1: who saw which patron, and when).
    View(PatronId),
    /// Records that a staff member asked to see the patron's whole CPF; changes nothing
    /// else (interface §18.4: the whole CPF only by an explicit, logged action).
    RevealCpf(PatronId),
    /// Sets the patron's OPAC password, or takes it away (docs/specs/opac/conta-do-leitor.md,
    /// items 1 to 4). Carries only the hash, never the password.
    SetPassword {
        /// The patron.
        patron: PatronId,
        /// The Argon2id hash; `None` takes the OPAC access away.
        password_hash: Option<String>,
    },
    /// The patron gives or takes back their consent to keep the reading history, on the
    /// OPAC (docs/specs/opac/conta-do-leitor.md). Only the loans returned afterwards follow it.
    SetReadingHistory {
        /// The patron.
        patron: PatronId,
        /// The choice.
        reading_history: ReadingHistory,
    },
    /// Brings branches, categories and patrons from a Koha dump
    /// (docs/specs/migracao/leitores-do-koha.md, item 16).
    MigrateKoha(Box<crate::koha_migration::KohaBatch>),
    /// Brings Koha's current loans and holds, after the patrons
    /// (docs/specs/migracao/circulacao-do-koha.md).
    MigrateKohaCirculation(Box<crate::koha_migration::KohaCirculationBatch>),
}

/// One change to the patron categories.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CategoryChange {
    /// A new category.
    Create {
        /// Its name.
        name: String,
        /// How long its patrons are enrolled for.
        enrolment: Enrolment,
    },
    /// Renames a category or changes its period; enrolled patrons keep their expiry.
    Update {
        /// The category.
        category: PatronCategoryId,
        /// Its new name.
        name: String,
        /// Its new period.
        enrolment: Enrolment,
    },
    /// Deletes a category no patron uses.
    Delete(PatronCategoryId),
}

/// Whether the library keeps what a patron borrowed after the return.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReadingHistory {
    /// Forgotten at the return: the default, without the patron's consent.
    #[default]
    NotKept,
    /// Kept, because the patron agreed.
    Kept,
}

/// What the library knows about a patron.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatronFields {
    /// Library card number, unique.
    pub cardnumber: String,
    /// Surname.
    pub surname: String,
    /// First name.
    pub first_name: Option<String>,
    /// Brazilian taxpayer number, unique.
    pub cpf: Option<Cpf>,
    /// Date of birth.
    pub date_of_birth: Option<Date>,
    /// E-mail address.
    pub email: Option<String>,
    /// Phone number.
    pub phone: Option<String>,
    /// Postal address.
    pub address: Option<String>,
    /// Home branch.
    pub branch: BranchId,
    /// Category.
    pub category: PatronCategoryId,
    /// Day of enrolment.
    pub enrolled: Date,
    /// Last day of the enrolment; `None` takes it from the category.
    pub expires: Option<Date>,
    /// Note only the staff sees.
    pub staff_note: Option<String>,
    /// Whether the reading history is kept.
    pub reading_history: ReadingHistory,
}

/// New fields for a patron read at `row_version`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatronUpdate {
    /// The patron.
    pub patron: PatronId,
    /// The version the form was read at.
    pub row_version: i64,
    /// Every field, as it should be.
    pub fields: PatronFields,
}

/// What a patron command returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatronOutput {
    /// The category created or changed; `None` after a delete.
    CategoryChanged(Option<PatronCategoryId>),
    /// The new patron.
    Added(PatronId),
    /// The patron changed.
    Updated(PatronId),
    /// The opening of the record was logged.
    Viewed(PatronId),
    /// The patron's OPAC password was set or taken away.
    PasswordSet(PatronId),
    /// The patron's choice about the reading history was recorded.
    ReadingHistorySet(PatronId),
    /// What a write of the Koha migration did.
    KohaMigrated(crate::koha_migration::KohaMigrated),
    /// What a write of Koha's loans and holds did.
    KohaCirculationMigrated(crate::koha_migration::KohaCirculationMigrated),
}

/// A field of the patron form, to name the one refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PatronField {
    /// Card number.
    Cardnumber,
    /// Surname.
    Surname,
    /// First name.
    FirstName,
    /// CPF, when the typed text is not one.
    Cpf,
    /// Date of birth.
    DateOfBirth,
    /// E-mail address.
    Email,
    /// Phone number.
    Phone,
    /// Postal address.
    Address,
    /// Expiry date.
    Expires,
    /// Internal note.
    StaffNote,
}

/// Why a patron command was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub enum PatronRefusal {
    /// A field is empty when required, too long or malformed.
    #[error("the field {0:?} is not valid")]
    BadField(PatronField),
    /// The category name is empty or too long, or its period out of range.
    #[error("the category is not valid")]
    BadCategory,
    /// Another category has this name.
    #[error("another category has this name")]
    CategoryNameTaken,
    /// The category has patrons, so it stays.
    #[error("the category has patrons")]
    CategoryInUse,
    /// Another patron has this card number.
    #[error("another patron has this card number")]
    CardnumberTaken,
    /// Another patron has this CPF.
    #[error("another patron has this CPF")]
    CpfTaken,
    /// No such category.
    #[error("no such patron category")]
    UnknownCategory,
    /// No such branch.
    #[error("no such branch")]
    UnknownBranch,
    /// No such patron.
    #[error("no such patron")]
    UnknownPatron,
    /// The patron changed since the form was read.
    #[error("the patron changed since it was read")]
    Stale,
}

impl PatronRefusal {
    /// Fluent key of the message.
    #[must_use]
    pub const fn message_key(self) -> &'static str {
        match self {
            Self::BadField(PatronField::Cardnumber) => "patron-error-cardnumber",
            Self::BadField(PatronField::Surname) => "patron-error-surname",
            Self::BadField(PatronField::FirstName) => "patron-error-first-name",
            Self::BadField(PatronField::Cpf) => "patron-error-cpf",
            Self::BadField(PatronField::DateOfBirth) => "patron-error-date-of-birth",
            Self::BadField(PatronField::Email) => "patron-error-email",
            Self::BadField(PatronField::Phone) => "patron-error-phone",
            Self::BadField(PatronField::Address) => "patron-error-address",
            Self::BadField(PatronField::Expires) => "patron-error-expires",
            Self::BadField(PatronField::StaffNote) => "patron-error-staff-note",
            Self::BadCategory => "patron-error-category",
            Self::CategoryNameTaken => "patron-error-category-taken",
            Self::CategoryInUse => "patron-error-category-in-use",
            Self::CardnumberTaken => "patron-error-cardnumber-taken",
            Self::CpfTaken => "patron-error-cpf-taken",
            Self::UnknownCategory => "patron-error-unknown-category",
            Self::UnknownBranch => "patron-error-unknown-branch",
            Self::UnknownPatron => "patron-error-unknown",
            Self::Stale => "patron-error-stale",
        }
    }
}

/// The category change as stored: the name trimmed.
///
/// # Errors
///
/// [`PatronRefusal::BadCategory`] for an empty or long name, or a period of 0 or more
/// than [`MAX_ENROLMENT_MONTHS`] months.
pub fn checked_category(change: CategoryChange) -> Result<CategoryChange, PatronRefusal> {
    let valid = |name: String, enrolment: Enrolment| {
        let name = required(&name, MAX_NAME).ok_or(PatronRefusal::BadCategory)?;
        if let Enrolment::Months(months) = enrolment
            && !(1..=MAX_ENROLMENT_MONTHS).contains(&months)
        {
            return Err(PatronRefusal::BadCategory);
        }
        Ok((name, enrolment))
    };
    Ok(match change {
        CategoryChange::Create { name, enrolment } => {
            let (name, enrolment) = valid(name, enrolment)?;
            CategoryChange::Create { name, enrolment }
        }
        CategoryChange::Update {
            category,
            name,
            enrolment,
        } => {
            let (name, enrolment) = valid(name, enrolment)?;
            CategoryChange::Update {
                category,
                name,
                enrolment,
            }
        }
        CategoryChange::Delete(category) => CategoryChange::Delete(category),
    })
}

/// The patron fields as stored: texts trimmed, empty optional ones dropped.
///
/// # Errors
///
/// [`PatronRefusal::BadField`] naming the first field that breaks its rule.
pub fn checked_patron(fields: PatronFields) -> Result<PatronFields, PatronRefusal> {
    let bad = PatronRefusal::BadField;
    let cardnumber =
        required(&fields.cardnumber, MAX_CARDNUMBER).ok_or(bad(PatronField::Cardnumber))?;
    let surname = required(&fields.surname, MAX_NAME).ok_or(bad(PatronField::Surname))?;
    let first_name = optional(fields.first_name.as_deref(), MAX_NAME)
        .map_err(|()| bad(PatronField::FirstName))?;
    let email =
        optional(fields.email.as_deref(), MAX_EMAIL).map_err(|()| bad(PatronField::Email))?;
    if let Some(email) = &email
        && !email
            .split_once('@')
            .is_some_and(|(user, host)| !user.is_empty() && !host.is_empty())
    {
        return Err(bad(PatronField::Email));
    }
    if fields
        .date_of_birth
        .is_some_and(|born| born > fields.enrolled)
    {
        return Err(bad(PatronField::DateOfBirth));
    }
    if fields.expires.is_some_and(|day| day < fields.enrolled) {
        return Err(bad(PatronField::Expires));
    }
    Ok(PatronFields {
        cardnumber,
        surname,
        first_name,
        email,
        phone: optional(fields.phone.as_deref(), MAX_PHONE)
            .map_err(|()| bad(PatronField::Phone))?,
        address: optional(fields.address.as_deref(), MAX_ADDRESS)
            .map_err(|()| bad(PatronField::Address))?,
        staff_note: optional(fields.staff_note.as_deref(), MAX_NOTE)
            .map_err(|()| bad(PatronField::StaffNote))?,
        ..fields
    })
}

/// Most patrons a search returns.
pub const MAX_SEARCH_ROWS: usize = 50;
/// Fewest characters a search by name needs.
pub const MIN_NAME_SEARCH: usize = 2;

/// A patron as the record shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Patron {
    /// The patron.
    pub id: PatronId,
    /// Version to send back with an edit.
    pub row_version: i64,
    /// Every field, with the expiry filled in.
    pub fields: PatronFields,
    /// Name of the home branch.
    pub branch_name: String,
    /// Name of the category.
    pub category_name: String,
    /// Whether the patron has an OPAC password.
    pub has_password: bool,
}

/// Shortest OPAC password (docs/specs/opac/conta-do-leitor.md, item 3).
pub const MIN_PASSWORD_CHARS: usize = 8;

/// Longest OPAC password; bounds the work of hashing.
pub const MAX_PASSWORD_CHARS: usize = 128;

/// Why a patron's OPAC password is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PasswordRefusal {
    /// Shorter than [`MIN_PASSWORD_CHARS`].
    #[error("the password is too short")]
    TooShort,
    /// Longer than [`MAX_PASSWORD_CHARS`].
    #[error("the password is too long")]
    TooLong,
    /// The same as the card number or the CPF.
    #[error("the password repeats the card number or the CPF")]
    Guessable,
}

/// Checks a new OPAC password for the patron with `fields` (spec, item 3).
///
/// # Errors
/// [`PasswordRefusal`] with the rule it breaks.
pub fn check_password(password: &str, fields: &PatronFields) -> Result<(), PasswordRefusal> {
    let length = password.chars().count();
    if length < MIN_PASSWORD_CHARS {
        return Err(PasswordRefusal::TooShort);
    }
    if length > MAX_PASSWORD_CHARS {
        return Err(PasswordRefusal::TooLong);
    }
    let digits: String = password.chars().filter(char::is_ascii_digit).collect();
    let same_cpf = fields
        .cpf
        .as_ref()
        .is_some_and(|cpf| digits.len() == password.len() && digits == cpf.digits());
    if password == fields.cardnumber || same_cpf {
        return Err(PasswordRefusal::Guessable);
    }
    Ok(())
}

/// One line of a patron search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatronRow {
    /// The patron.
    pub id: PatronId,
    /// Library card number.
    pub cardnumber: String,
    /// Surname.
    pub surname: String,
    /// First name.
    pub first_name: Option<String>,
    /// Name of the category.
    pub category_name: String,
    /// Last day of the enrolment.
    pub expires: Date,
    /// Name of the patron's home branch.
    pub branch_name: String,
    /// When each open loan is due.
    pub loans_due: Vec<Zoned>,
    /// Open holds.
    pub holds: u32,
    /// Holds set aside, waiting for the patron.
    pub holds_waiting: u32,
    /// Day of the enrolment.
    pub enrolled: Date,
    /// Whether returns are kept in the reading history.
    pub reading_history: ReadingHistory,
    /// What the patron owes.
    pub owed: Money,
}

/// A patron category with how many patrons it has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatronCategory {
    /// The category.
    pub id: PatronCategoryId,
    /// Its name.
    pub name: String,
    /// Its period.
    pub enrolment: Enrolment,
    /// How many patrons are in it.
    pub patrons: u64,
}

/// What a patron search looks for: a card number or a CPF exactly, and a name in part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatronQuery {
    /// The text trimmed, compared whole to the card number.
    pub cardnumber: String,
    /// The CPF, when the text is one.
    pub cpf: Option<Cpf>,
    /// The text to find inside names, when it is long enough.
    pub name: Option<String>,
}

impl PatronQuery {
    /// Reads what was typed; `None` when nothing was.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        Some(Self {
            cardnumber: text.to_owned(),
            cpf: Cpf::parse(text).ok(),
            name: (text.chars().count() >= MIN_NAME_SEARCH).then(|| text.to_owned()),
        })
    }
}

/// Why the patron desk could not answer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PatronDeskError {
    /// The command was refused.
    #[error(transparent)]
    Refused(#[from] PatronRefusal),
    /// The writer queue is full; try again.
    #[error("the writer is busy")]
    Busy,
    /// Storage failed.
    #[error("patrons unavailable: {0}")]
    Unavailable(String),
}

/// What the staff screens read and write about patrons. The web layer reaches patrons only
/// through this port.
pub trait PatronDesk: Send + Sync {
    /// Runs one command as `actor`.
    ///
    /// # Errors
    ///
    /// [`PatronDeskError::Refused`] with the refusal, or why storage failed.
    fn apply(
        &self,
        actor: StaffId,
        command: PatronCommand,
    ) -> Result<PatronOutput, PatronDeskError>;

    /// One patron.
    ///
    /// # Errors
    ///
    /// When storage fails.
    fn patron(&self, id: PatronId) -> Result<Option<Patron>, PatronDeskError>;

    /// The patrons whose card number or CPF is the query, or whose name holds it, by
    /// surname, at most [`MAX_SEARCH_ROWS`].
    ///
    /// # Errors
    ///
    /// When storage fails.
    fn search(&self, query: &PatronQuery) -> Result<Vec<PatronRow>, PatronDeskError>;

    /// Every category, by name.
    ///
    /// # Errors
    ///
    /// When storage fails.
    fn categories(&self) -> Result<Vec<PatronCategory>, PatronDeskError>;
}

/// `text` trimmed, when it is not empty and fits in `max` characters.
fn required(text: &str, max: usize) -> Option<String> {
    let trimmed = text.trim();
    (!trimmed.is_empty() && trimmed.chars().count() <= max).then(|| trimmed.to_owned())
}

/// `text` trimmed, `None` when empty; `Err` when longer than `max` characters.
fn optional(text: Option<&str>, max: usize) -> Result<Option<String>, ()> {
    match text.map(str::trim) {
        None | Some("") => Ok(None),
        Some(text) if text.chars().count() > max => Err(()),
        Some(text) => Ok(Some(text.to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    fn fields() -> PatronFields {
        PatronFields {
            cardnumber: " 000123 ".to_owned(),
            surname: "Silva".to_owned(),
            first_name: Some("  ".to_owned()),
            cpf: None,
            date_of_birth: Some(date(2010, 5, 1)),
            email: Some(" ana@example.org ".to_owned()),
            phone: None,
            address: None,
            branch: BranchId(1),
            category: PatronCategoryId(1),
            enrolled: date(2026, 11, 19),
            expires: None,
            staff_note: None,
            reading_history: ReadingHistory::default(),
        }
    }

    #[test]
    fn texts_are_trimmed_and_empty_ones_dropped() {
        let checked = checked_patron(fields()).unwrap();
        assert_eq!(checked.cardnumber, "000123");
        assert_eq!(checked.first_name, None);
        assert_eq!(checked.email.as_deref(), Some("ana@example.org"));
        assert_eq!(checked.reading_history, ReadingHistory::NotKept);
    }

    #[test]
    fn each_broken_rule_names_its_field() {
        let refused = |change: fn(&mut PatronFields)| {
            let mut fields = fields();
            change(&mut fields);
            checked_patron(fields).unwrap_err()
        };
        let bad = PatronRefusal::BadField;
        assert_eq!(
            refused(|f| f.cardnumber = " ".to_owned()),
            bad(PatronField::Cardnumber)
        );
        assert_eq!(
            refused(|f| f.cardnumber = "1".repeat(33)),
            bad(PatronField::Cardnumber)
        );
        assert_eq!(
            refused(|f| f.surname = String::new()),
            bad(PatronField::Surname)
        );
        assert_eq!(
            refused(|f| f.email = Some("ana@".to_owned())),
            bad(PatronField::Email)
        );
        assert_eq!(
            refused(|f| f.date_of_birth = Some(date(2026, 11, 20))),
            bad(PatronField::DateOfBirth)
        );
        assert_eq!(
            refused(|f| f.expires = Some(date(2026, 11, 18))),
            bad(PatronField::Expires)
        );
        assert_eq!(
            refused(|f| f.staff_note = Some("x".repeat(1001))),
            bad(PatronField::StaffNote)
        );
    }

    #[test]
    fn a_query_finds_a_cpf_and_names_of_two_characters_or_more() {
        assert_eq!(PatronQuery::parse("  "), None);
        let short = PatronQuery::parse("7").unwrap();
        assert_eq!(short.name, None);
        assert_eq!(short.cardnumber, "7");
        let cpf = PatronQuery::parse(" 529.982.247-25 ").unwrap();
        assert_eq!(
            cpf.cpf.map(|cpf| cpf.digits().to_owned()).as_deref(),
            Some("52998224725")
        );
        assert_eq!(PatronQuery::parse("Silva").unwrap().cpf, None);
    }

    #[test]
    fn a_category_needs_a_name_and_a_period_in_range() {
        let create = |name: &str, enrolment| {
            checked_category(CategoryChange::Create {
                name: name.to_owned(),
                enrolment,
            })
        };
        assert_eq!(
            create(" Estudante ", Enrolment::Months(12)),
            Ok(CategoryChange::Create {
                name: "Estudante".to_owned(),
                enrolment: Enrolment::Months(12),
            })
        );
        assert_eq!(
            create("", Enrolment::Months(12)),
            Err(PatronRefusal::BadCategory)
        );
        assert_eq!(
            create("Estudante", Enrolment::Months(0)),
            Err(PatronRefusal::BadCategory)
        );
        assert_eq!(
            create("Estudante", Enrolment::Months(1000)),
            Err(PatronRefusal::BadCategory)
        );
    }

    #[test]
    fn a_password_is_long_enough_and_not_the_card_or_the_cpf() {
        let mut fields = fields();
        fields.cardnumber = "12345678".to_owned();
        fields.cpf = Some(Cpf::parse("529.982.247-25").unwrap());
        assert_eq!(
            check_password("1234567", &fields),
            Err(PasswordRefusal::TooShort)
        );
        assert_eq!(
            check_password(&"a".repeat(MAX_PASSWORD_CHARS + 1), &fields),
            Err(PasswordRefusal::TooLong)
        );
        assert!(check_password(&"á".repeat(MAX_PASSWORD_CHARS), &fields).is_ok());
        assert_eq!(
            check_password("12345678", &fields),
            Err(PasswordRefusal::Guessable)
        );
        assert_eq!(
            check_password("52998224725", &fields),
            Err(PasswordRefusal::Guessable)
        );
        // The CPF inside a longer password is fine.
        assert!(check_password("cpf52998224725", &fields).is_ok());
        assert!(check_password("leitura-segura", &fields).is_ok());
    }
}
