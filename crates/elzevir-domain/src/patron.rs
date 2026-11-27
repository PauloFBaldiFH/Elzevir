// SPDX-License-Identifier: GPL-3.0-or-later
//! Patrons and their categories: identities, the CPF and the enrolment period
//! (docs/specs/circulacao/leitores.md).

use jiff::ToSpan;
use jiff::civil::Date;
use serde::{Deserialize, Serialize};

/// A patron of the library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PatronId(pub i64);

/// A patron category, created freely by the library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PatronCategoryId(pub i64);

/// An additional patron field the library defined (docs/specs/circulacao/patron-attributes.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PatronFieldId(pub i64);

/// One option of an additional patron field chosen from a list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PatronFieldOptionId(pub i64);

/// Most months an enrolment period may last.
pub const MAX_ENROLMENT_MONTHS: u16 = 999;

/// How long a patron of a category is enrolled for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Enrolment {
    /// This many months from the day of enrolment.
    Months(u16),
    /// Until this fixed day, whatever the day of enrolment.
    Until(Date),
}

impl Enrolment {
    /// The expiry date of a patron enrolled on `enrolled`. A month past the end of the
    /// shorter month stops at its last day (31 January + 1 month = end of February).
    /// `None` when the date falls outside the calendar.
    ///
    /// Koha: `Koha::Patron::Category::get_expiry_date`.
    #[must_use]
    pub fn expiry(self, enrolled: Date) -> Option<Date> {
        match self {
            // jiff clamps the day to the end of the month, as Koha's `end_of_month => 'limit'`.
            Self::Months(months) => enrolled.checked_add(i64::from(months).months()).ok(),
            Self::Until(day) => Some(day),
        }
    }
}

/// Whether a patron whose enrolment ends on `expiry` is expired on `today`.
///
/// Koha: `Koha::Patron::is_expired`.
#[must_use]
pub fn is_expired(expiry: Date, today: Date) -> bool {
    expiry < today
}

/// A Brazilian taxpayer number: 11 digits whose last two check the first nine.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Cpf(String);

/// Why a text is not a CPF.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CpfError {
    /// Not 11 digits once dots, dashes and spaces are taken out.
    #[error("a CPF has 11 digits")]
    Length,
    /// A character other than a digit, dot, dash or space.
    #[error("a CPF holds only digits, dots and a dash")]
    Character,
    /// The check digits do not match, or all digits are the same.
    #[error("the CPF check digits do not match")]
    CheckDigits,
}

impl Cpf {
    /// Reads a CPF typed with or without punctuation.
    ///
    /// # Errors
    ///
    /// [`CpfError`] when the text is not a valid CPF.
    pub fn parse(text: &str) -> Result<Self, CpfError> {
        let mut digits = Vec::with_capacity(11);
        for c in text.chars() {
            match c {
                '0'..='9' => digits.push(u32::from(c) - u32::from('0')),
                '.' | '-' | ' ' => {}
                _ => return Err(CpfError::Character),
            }
        }
        if digits.len() != 11 {
            return Err(CpfError::Length);
        }
        let first = digits.first().copied();
        if digits.iter().all(|d| Some(*d) == first) {
            return Err(CpfError::CheckDigits);
        }
        let ninth = check_digit(digits.iter().take(9));
        let tenth = check_digit(digits.iter().take(10));
        if digits.get(9) != Some(&ninth) || digits.get(10) != Some(&tenth) {
            return Err(CpfError::CheckDigits);
        }
        Ok(Self(
            digits
                .iter()
                .filter_map(|d| char::from_digit(*d, 10))
                .collect(),
        ))
    }

    /// The 11 digits, without punctuation, as stored.
    #[must_use]
    pub fn digits(&self) -> &str {
        &self.0
    }

    /// The CPF as shown on screen: only the middle six digits, `***.456.789-**`.
    #[must_use]
    pub fn masked(&self) -> String {
        let middle = self.0.get(3..9).unwrap_or_default();
        let (left, right) = middle.split_at(middle.len().min(3));
        format!("***.{left}.{right}-**")
    }

    /// The whole CPF with its punctuation, `123.456.789-09`, for the explicit "Show CPF"
    /// (interface §18.4).
    #[must_use]
    pub fn formatted(&self) -> String {
        let part = |range: std::ops::Range<usize>| self.0.get(range).unwrap_or_default();
        format!(
            "{}.{}.{}-{}",
            part(0..3),
            part(3..6),
            part(6..9),
            part(9..11)
        )
    }
}

/// The check digit of `digits`: weights from `len + 1` down to 2, sum × 10 mod 11, and
/// 10 counts as 0.
fn check_digit<'a>(digits: impl ExactSizeIterator<Item = &'a u32>) -> u32 {
    let mut weight = u32::try_from(digits.len()).unwrap_or(0) + 1;
    let mut sum = 0;
    for digit in digits {
        sum += digit * weight;
        weight = weight.saturating_sub(1);
    }
    match sum * 10 % 11 {
        10 => 0,
        rest => rest,
    }
}

impl TryFrom<String> for Cpf {
    type Error = CpfError;

    fn try_from(text: String) -> Result<Self, CpfError> {
        Self::parse(&text)
    }
}

impl From<Cpf> for String {
    fn from(cpf: Cpf) -> Self {
        cpf.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    #[test]
    fn a_cpf_with_or_without_punctuation_keeps_its_digits() {
        let cpf = Cpf::parse("529.982.247-25").unwrap();
        assert_eq!(cpf.digits(), "52998224725");
        assert_eq!(Cpf::parse("52998224725").unwrap(), cpf);
        assert_eq!(cpf.masked(), "***.982.247-**");
        assert_eq!(cpf.formatted(), "529.982.247-25");
    }

    #[test]
    fn a_wrong_cpf_is_refused() {
        assert_eq!(Cpf::parse("529.982.247-26"), Err(CpfError::CheckDigits));
        assert_eq!(Cpf::parse("111.111.111-11"), Err(CpfError::CheckDigits));
        assert_eq!(Cpf::parse("5299822472"), Err(CpfError::Length));
        assert_eq!(Cpf::parse("529/982/247-25"), Err(CpfError::Character));
        // Check digit 10 counts as 0.
        assert!(Cpf::parse("000.000.006-04").is_ok());
    }

    #[test]
    fn months_stop_at_the_end_of_a_shorter_month() {
        assert_eq!(
            Enrolment::Months(1).expiry(date(2026, 1, 31)),
            Some(date(2026, 2, 28))
        );
        assert_eq!(
            Enrolment::Months(1).expiry(date(2028, 1, 31)),
            Some(date(2028, 2, 29))
        );
        assert_eq!(
            Enrolment::Months(12).expiry(date(2026, 11, 19)),
            Some(date(2027, 11, 19))
        );
        assert_eq!(
            Enrolment::Until(date(2026, 12, 31)).expiry(date(2026, 11, 19)),
            Some(date(2026, 12, 31))
        );
    }

    #[test]
    fn a_patron_expires_the_day_after_the_expiry_date() {
        assert!(!is_expired(date(2026, 11, 19), date(2026, 11, 19)));
        assert!(is_expired(date(2026, 11, 18), date(2026, 11, 19)));
    }
}
