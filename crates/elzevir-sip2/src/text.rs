// SPDX-License-Identifier: GPL-3.0-or-later
//! How a response is written (spec item 14).

use jiff::civil::{Date, DateTime};

/// A response being written: the code, the fixed fields, then the variable fields.
pub(crate) struct Writer(String);

impl Writer {
    /// A response with message code `code`.
    pub(crate) fn new(code: &str) -> Self {
        Self(code.to_owned())
    }

    /// Fixed text, as it is.
    pub(crate) fn fixed(&mut self, text: &str) -> &mut Self {
        self.0.push_str(text);
        self
    }

    /// `Y` or `N`.
    ///
    /// Koha: `C4::SIP::Sip::sipbool`.
    pub(crate) fn yes_no(&mut self, yes: bool) -> &mut Self {
        self.0.push(if yes { 'Y' } else { 'N' });
        self
    }

    /// `Y` or a space.
    ///
    /// Koha: `C4::SIP::Sip::boolspace`.
    pub(crate) fn yes_blank(&mut self, yes: bool) -> &mut Self {
        self.0.push(if yes { 'Y' } else { ' ' });
        self
    }

    /// A count in four digits; a count over 9999 writes four spaces, as Koha does with
    /// one that does not fit.
    ///
    /// Koha: `C4::SIP::Sip::add_count`.
    pub(crate) fn count(&mut self, count: usize) -> &mut Self {
        if count > 9999 {
            self.0.push_str("    ");
        } else {
            self.0.push_str(&four_digits(count));
        }
        self
    }

    /// A date and time.
    ///
    /// Koha: `C4::SIP::Sip::timestamp`.
    pub(crate) fn timestamp(&mut self, at: DateTime) -> &mut Self {
        self.0.push_str(&timestamp(at));
        self
    }

    /// A field that is written even when empty.
    ///
    /// Koha: `C4::SIP::Sip::add_field`.
    pub(crate) fn field(&mut self, code: &str, value: &str) -> &mut Self {
        self.0.push_str(code);
        let mut chars = value.chars().peekable();
        while let Some(char) = chars.next() {
            match char {
                '\r' => {
                    chars.next_if_eq(&'\n');
                    self.0.push(' ');
                }
                '\n' => self.0.push(' '),
                '|' => self.0.push_str("&#124;"),
                other => self.0.push(other),
            }
        }
        self.0.push('|');
        self
    }

    /// A field that says `Y` or `N`.
    pub(crate) fn yes_no_field(&mut self, code: &str, yes: bool) -> &mut Self {
        self.field(code, if yes { "Y" } else { "N" })
    }

    /// A field that is left out when it has no value or an empty one.
    ///
    /// Koha: `C4::SIP::Sip::maybe_add`.
    pub(crate) fn maybe(&mut self, code: &str, value: Option<&str>) -> &mut Self {
        match value {
            Some(value) if !value.is_empty() => self.field(code, value),
            _ => self,
        }
    }

    /// The fields of every value, in order.
    pub(crate) fn each(&mut self, code: &str, values: &[String]) -> &mut Self {
        for value in values {
            self.field(code, value);
        }
        self
    }

    /// The text written.
    pub(crate) fn finish(&mut self) -> String {
        std::mem::take(&mut self.0)
    }
}

/// `count` in four digits.
fn four_digits(count: usize) -> String {
    format!("{count:04}")
}

/// `at` as `YYYYMMDD    HHMMSS`.
pub(crate) fn timestamp(at: DateTime) -> String {
    at.strftime("%Y%m%d    %H%M%S").to_string()
}

/// A due date: the day, always at `235900`, as Koha writes a date without a time.
pub(crate) fn day(date: Date) -> String {
    format!("{}    235900", date.strftime("%Y%m%d"))
}

#[cfg(test)]
mod tests {
    use jiff::civil::date;

    use super::*;

    #[test]
    fn fields_are_escaped_and_dates_padded() {
        let text = Writer::new("AA")
            .field("AF", "Line one\r\nLine two\nLine three\rfour")
            .field("AF", "")
            .maybe("AG", Some(""))
            .maybe("AG", None)
            .field("AE", "a|b")
            .count(3)
            .count(10_000)
            .timestamp(date(2011, 1, 12).at(9, 5, 7, 0))
            .finish();
        assert_eq!(
            text,
            "AAAFLine one Line two Line three four|AF|AEa&#124;b|0003    20110112    090507"
        );
        assert_eq!(day(date(2011, 1, 12)), "20110112    235900");
    }
}
