// SPDX-License-Identifier: GPL-3.0-or-later
//! Printed labels (docs/specs/catalogo/etiquetas.md).

/// The lines a Dewey call number takes on a spine label: the class, with any letters before
/// it on a line of their own, then each word of the rest.
///
/// Koha: `C4::ClassSplitRoutine::Dewey::split_callnumber`, after trimming the call number;
/// slashes are dropped, and a call number that does not start with a class is split at its
/// spaces.
#[must_use]
pub fn split_dewey_call_number(call_number: &str) -> Vec<String> {
    let text: String = call_number.trim().chars().filter(|c| *c != '/').collect();
    let Some(end) = class_end(&text) else {
        return text.split_whitespace().map(str::to_owned).collect();
    };
    let (class, rest) = text.split_at(end);
    let mut lines = match prefixed_class(class) {
        Some((prefix, number)) => vec![prefix.to_owned(), number.to_owned()],
        None => vec![class.to_owned()],
    };
    lines.extend(rest.split_whitespace().map(str::to_owned));
    lines
}

/// Where the leading part ends: letters and hyphens, at most one space, then maybe a Dewey
/// number of three digits or more, followed by a space (Koha's
/// `^([-a-zA-Z]*\s?(?:\d{3,}(?:\.\d+)?)?)\s+(.+)`, with its backtracking).
fn class_end(text: &str) -> Option<usize> {
    let letters = text
        .bytes()
        .take_while(|b| b.is_ascii_alphabetic() || *b == b'-')
        .count();
    let space = text
        .get(letters..)
        .and_then(|after| after.chars().next())
        .filter(|c| c.is_whitespace())
        .map_or(0, char::len_utf8);
    let mut tries = vec![(space, true), (space, false)];
    if space > 0 {
        tries.extend([(0, true), (0, false)]);
    }
    tries.into_iter().find_map(|(space, with_number)| {
        let mut end = letters + space;
        if with_number {
            end += text.get(end..).and_then(dewey_number)?;
        }
        let rest = text.get(end..)?;
        (rest.starts_with(char::is_whitespace) && !rest.trim().is_empty()).then_some(end)
    })
}

/// The length of the Dewey number at the start of `text`: three digits or more, then maybe a
/// period and digits.
fn dewey_number(text: &str) -> Option<usize> {
    let digits = |s: &str| s.bytes().take_while(u8::is_ascii_digit).count();
    let whole = digits(text);
    if whole < 3 {
        return None;
    }
    let fraction = text
        .get(whole..)
        .and_then(|rest| rest.strip_prefix('.'))
        .map_or(0, digits);
    Some(if fraction > 0 {
        whole + 1 + fraction
    } else {
        whole
    })
}

/// Letters before a Dewey number and the number, when the class is only that
/// (`^([-a-zA-Z]+)\s?(\d{3,}(?:\.\d+)?)$`).
fn prefixed_class(class: &str) -> Option<(&str, &str)> {
    let letters = class
        .bytes()
        .take_while(|b| b.is_ascii_alphabetic() || *b == b'-')
        .count();
    if letters == 0 {
        return None;
    }
    let (prefix, rest) = class.split_at(letters);
    let number = rest
        .strip_prefix(|c: char| c.is_whitespace())
        .unwrap_or(rest);
    (dewey_number(number) == Some(number.len())).then_some((prefix, number))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(call_number: &str) -> Vec<String> {
        split_dewey_call_number(call_number)
    }

    #[test]
    fn dewey_call_numbers_split_as_in_kohas_tests() {
        // reference/koha/t/Labels_split_ddcn.t
        assert_eq!(
            split("R220.3 H2793Z H32 c.2"),
            ["R", "220.3", "H2793Z", "H32", "c.2"]
        );
        assert_eq!(split("CD-ROM 787.87 EAS"), ["CD-ROM", "787.87", "EAS"]);
        assert_eq!(split("252.051 T147 v.1-2"), ["252.051", "T147", "v.1-2"]);
    }

    #[test]
    fn a_pha_call_number_takes_a_line_per_part() {
        assert_eq!(split(" 869.3 A474d "), ["869.3", "A474d"]);
        assert_eq!(
            split("869.3 A474d v.2 ex.3"),
            ["869.3", "A474d", "v.2", "ex.3"]
        );
        assert_eq!(split("J 869.3 A474d"), ["J", "869.3", "A474d"]);
        assert_eq!(split("869.3/A474d"), ["869.3A474d"]);
    }

    #[test]
    fn what_does_not_start_with_a_class_is_split_at_spaces() {
        assert_eq!(split("Ficção A474d"), ["Ficção", "A474d"]);
        assert_eq!(split("82-31  C"), ["82-31", "C"]);
        assert_eq!(split("A474d"), ["A474d"]);
        assert!(split("  ").is_empty());
        // Two spaces after the letters: the first stays with them, as in Koha.
        assert_eq!(split("REF  x"), ["REF ", "x"]);
    }
}
