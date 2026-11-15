// SPDX-License-Identifier: GPL-3.0-or-later
//! The time period of the content (`045 $a`, docs/specs/catalogo/bibframe-completo.md,
//! item 29, part 5f), after the Library of Congress conversions (`marc2bibframe2`:
//! `ConvSpec-010-048.xsl`; `bibframe2marc`: `02-010-048.xml`).
//!
//! Each `045 $a` becomes a `bf:temporalCoverage` of the Work in EDTF: a code of four
//! characters names two periods, each a century and a decade (`x9` → `199X`, `x-` →
//! `19XX`) or a span before the Common Era (`d5` → `-04XX`); two equal periods give one
//! date, two different ones a range. The LC conversion writes `19-X` for a code without a
//! decade, which is not EDTF; here it is `19XX`, which its way back reads. The formatted
//! dates of `$b` and `$c` stay in the source record.

use elzevir_domain::statement::{Statement, Value};

use super::{Marc, text};

/// The span before 3000 B.C.E. (`a0`), a range of its own.
const BEFORE_3000: &str = "-XXXX/-3000";

/// The centuries of the Common Era, from `e` (00) to `y` (20).
const CENTURIES: &[u8; 21] = b"efghijklmnopqrstuvwxy";

/// The EDTF dates of `045 $a`, in field order.
pub(super) fn of_work(marc: &Marc<'_>) -> Vec<Statement> {
    marc.data(b"045")
        .flat_map(|field| field.values(b'a'))
        .filter_map(text::trimmed)
        .filter_map(|code| edtf(&code))
        .map(|date| Statement {
            property: "temporalCoverage".to_owned(),
            value: Value::Literal {
                text: date,
                language: None,
            },
        })
        .collect()
}

/// `x9x9` → `199X`; `x8x9` → `198X/199X`; `None` for anything that is not a code.
pub(super) fn edtf(code: &str) -> Option<String> {
    let code = code.to_ascii_lowercase();
    let (first, second) = (period(code.get(..2)?)?, period(code.get(2..4)?)?);
    if code.len() != 4 {
        return None;
    }
    Some(if first == second {
        first
    } else {
        format!("{first}/{second}")
    })
}

/// `199X/200X` → `x9y0`; `None` for a date that no code gives.
pub(super) fn code(date: &str) -> Option<String> {
    if date == BEFORE_3000 {
        return Some("a0a0".to_owned());
    }
    let (first, second) = date.split_once('/').unwrap_or((date, date));
    Some(format!("{}{}", period_code(first)?, period_code(second)?))
}

/// One period of two characters.
fn period(code: &str) -> Option<String> {
    let [letter, digit] = <[u8; 2]>::try_from(code.as_bytes()).ok()?;
    match letter {
        b'a' if digit == b'0' => Some(BEFORE_3000.to_owned()),
        // `b0` is the 29th century B.C.E.; `d9`, the first.
        b'b'..=b'd' if digit.is_ascii_digit() => {
            let century = 29 - 10 * u32::from(letter - b'b') - u32::from(digit - b'0');
            Some(format!("-{century:02}XX"))
        }
        _ => {
            let century = CENTURIES.iter().position(|c| *c == letter)?;
            match digit {
                b'-' => Some(format!("{century:02}XX")),
                b'0'..=b'9' => Some(format!("{century:02}{}X", char::from(digit))),
                _ => None,
            }
        }
    }
}

/// One EDTF date back to its period.
fn period_code(date: &str) -> Option<String> {
    if let Some(span) = date.strip_prefix('-') {
        let century: u32 = span.strip_suffix("XX")?.parse().ok()?;
        let back = 29u32.checked_sub(century)?;
        let letter = b'b' + u8::try_from(back / 10).ok()?;
        return (letter <= b'd').then(|| format!("{}{}", char::from(letter), back % 10));
    }
    let bytes = date.as_bytes();
    let (Some(tens), Some(units), Some(decade), Some(b'X'), 4) = (
        bytes.first(),
        bytes.get(1),
        bytes.get(2),
        bytes.get(3),
        bytes.len(),
    ) else {
        return None;
    };
    let century = usize::from(tens.checked_sub(b'0')?) * 10 + usize::from(units.checked_sub(b'0')?);
    let letter = char::from(*CENTURIES.get(century)?);
    match decade {
        b'X' => Some(format!("{letter}-")),
        b'0'..=b'9' => Some(format!("{letter}{}", char::from(*decade))),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_periods_become_edtf_dates_and_come_back() {
        for (code, date) in [
            ("x9x9", "199X"),
            ("x-x-", "19XX"),
            ("x8x9", "198X/199X"),
            ("d5d5", "-04XX"),
            ("b0d9", "-29XX/-00XX"),
            ("a0a0", BEFORE_3000),
            ("e0y2", "000X/202X"),
        ] {
            assert_eq!(edtf(code).as_deref(), Some(date), "{code}");
            assert_eq!(super::code(date).as_deref(), Some(code), "{date}");
        }
        assert_eq!(edtf("x9"), None);
        assert_eq!(edtf("z9z9"), None);
        assert_eq!(code("1995"), None);
    }
}
