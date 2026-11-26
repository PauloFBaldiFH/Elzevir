// SPDX-License-Identifier: GPL-3.0-or-later
//! RIS (docs/specs/catalogo/citacao-e-intercambio.md, items 6 and 7).
//!
//! Koha: `C4::Ris::marc2ris`, MARC21 branch, without `RisExportAdditionalFields`.

use elzevir_marc::Record;

use super::{DataField, fields, first_field};

/// Notes pooled into `N1`, each with the subfields Koha reads.
const NOTES: [(&[u8; 3], &[u8]); 24] = [
    (b"500", b"a"),
    (b"501", b"a"),
    (b"502", b"a"),
    (b"504", b"ab"),
    (b"505", b"agrtu"),
    (b"506", b"abcde"),
    (b"507", b"ab"),
    (b"508", b"a"),
    (b"510", b"abcx3"),
    (b"511", b"a"),
    (b"513", b"ab"),
    (b"514", b"abcdefghijkmuz"),
    (b"515", b"a"),
    (b"516", b"a"),
    (b"518", b"a3"),
    (b"521", b"ab3"),
    (b"522", b"a"),
    (b"524", b"a23"),
    (b"525", b"a"),
    (b"526", b"abcdixz5"),
    (b"530", b"abcdu3"),
    (b"533", b"abcdefmn3"),
    (b"534", b"abcefklmnptxz"),
    (b"535", b"abcdg3"),
];

/// Subject fields read for `KW`, in Koha's order.
const SUBJECTS: [&[u8; 3]; 12] = [
    b"600", b"610", b"611", b"630", b"650", b"651", b"653", b"654", b"655", b"656", b"657", b"658",
];

/// Subfields of a subject read as keywords: every letter and `2` to `4`, but `i` and `w`.
const KEYWORD_CODES: &[u8] = b"abcdefghjklmnopqrstuvxyz234";

pub(super) fn ris(record: &Record, deposit_url: Option<&str>) -> String {
    let mut out = Lines::default();
    out.line("TY", type_tag(&record.leader));
    for field in fields(record, *b"100").chain(fields(record, *b"700")) {
        out.line("AU", &author(field));
    }
    let editors: [&[u8; 3]; 4] = [b"110", b"111", b"710", b"711"];
    for tag in editors {
        for field in fields(record, *tag) {
            out.line("ED", &field.value(b'a'));
        }
    }
    if let Some(field) = first_field(record, *b"245") {
        out.line("TI", &title(field));
    }
    if let Some(field) = first_field(record, *b"490") {
        out.line("T2", &clean_title(&field.value(b'a')));
    }
    if let Some(field) = first_field(record, *b"020") {
        out.line("SN", &field.value(b'a'));
    }
    if let Some(field) = first_field(record, *b"022") {
        out.line("SN", &field.value(b'a').chars().take(9).collect::<String>());
    }
    call_numbers(record, &mut out);
    let publication = first_field(record, *b"264").or_else(|| first_field(record, *b"260"));
    if let Some(field) = publication {
        publication_lines(field, &mut out);
    }
    for keyword in keywords(record) {
        out.line("KW", &keyword);
    }
    out.line("N1", &notes(record).join("; "));
    out.line("N2", &abstracts(record).join("; "));
    for field in fields(record, *b"856") {
        out.line("UR", &field.value(b'u'));
    }
    if let Some(url) = deposit_url {
        out.line("UR", url);
    }
    out.text.push_str("ER  - \r\n");
    out.text
}

/// The RIS lines written so far.
#[derive(Default)]
struct Lines {
    text: String,
}

impl Lines {
    /// `TAG  - value`, unless the value is empty (item 7).
    fn line(&mut self, tag: &str, value: &str) {
        if value.trim().is_empty() {
            return;
        }
        self.text.push_str(tag);
        self.text.push_str("  - ");
        self.text.push_str(value);
        self.text.push_str("\r\n");
    }
}

/// Koha: `C4::Ris::print_typetag`, MARC21 table.
fn type_tag(leader: &[u8]) -> &'static str {
    let type_of_record = leader.get(6).copied().unwrap_or(b' ');
    let level = leader.get(7).copied().unwrap_or(b' ');
    if type_of_record == b'a' {
        return match level {
            b'm' => "BOOK",
            b'b' | b's' => "SER",
            b'i' => "DATA",
            // Component parts and collections; Koha writes no TY for other levels.
            _ => "GEN",
        };
    }
    match type_of_record {
        b'c' | b'd' => "MUSIC",
        b'e' | b'f' => "MAP",
        b'g' => "ADVS",
        b'i' | b'j' => "SOUND",
        b'k' | b'r' => "ART",
        b'm' => "DATA",
        b't' => "MANSCPT",
        _ => "GEN",
    }
}

/// Koha: `C4::Ris::get_author`.
fn author(field: DataField<'_>) -> String {
    normalized_name(&field.value(b'a'), &field.value(b'b'), field.indicators[0])
}

/// Koha: `C4::Ris::normalize_author`. A surname first (indicator 1) is tidied; any
/// other name stays as written.
fn normalized_name(name: &str, numeration: &str, indicator: u8) -> String {
    if indicator != b'1' {
        return name.to_owned();
    }
    let name = drop_abbreviation_periods(&strip_end(&squeeze_after_marks(name), ",;:/"));
    let numeration = strip_end(numeration, ",;:/");
    if numeration.is_empty() {
        name
    } else {
        format!("{name}, {numeration}")
    }
}

/// `s%([,.]+) *%$1%g`: no space after a comma or a period.
fn squeeze_after_marks(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut after_mark = false;
    for c in text.chars() {
        if after_mark && c == ' ' {
            continue;
        }
        after_mark = matches!(c, ',' | '.');
        out.push(c);
    }
    out
}

/// `s% *[<set>]*$%%`: the marks of `set` at the end, then the spaces before them.
fn strip_end(text: &str, set: &str) -> String {
    text.trim_end_matches(|c| set.contains(c))
        .trim_end_matches(' ')
        .to_owned()
}

/// `s%(\w{2,})\.%$1%g`: no period after a word of two letters or more.
fn drop_abbreviation_periods(text: &str) -> String {
    let word = |c: Option<&char>| c.is_some_and(|c| c.is_alphanumeric() || *c == '_');
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    for (position, c) in chars.iter().enumerate() {
        let after_word =
            position >= 2 && word(chars.get(position - 1)) && word(chars.get(position - 2));
        if *c == '.' && after_word {
            continue;
        }
        out.push(*c);
    }
    out
}

/// `s% *[/:;.]$%%`: one closing mark and the spaces before it.
fn clean_title(text: &str) -> String {
    strip_one(text, "/:;.")
}

/// Koha: `C4::Ris::print_title`.
fn title(field: DataField<'_>) -> String {
    let title = clean_title(&field.value(b'a'));
    let subtitle = field.value(b'b');
    // `s%^ *(.*) *[/:;.]$%$1%`: only a subtitle that ends with a mark changes; the space
    // Koha leaves before the mark goes too.
    let subtitle = match subtitle.chars().last() {
        Some('/' | ':' | ';' | '.') => {
            let mut chars = subtitle.trim_start_matches(' ').chars();
            chars.next_back();
            chars.as_str().trim_end().to_owned()
        }
        _ => subtitle,
    };
    if subtitle.is_empty() {
        title
    } else {
        format!("{title}: {subtitle}")
    }
}

/// Koha: `C4::Ris::print_loc_callno` and `print_dewey`.
fn call_numbers(record: &Record, out: &mut Lines) {
    if let Some(field) = first_field(record, *b"050") {
        let class = field.value(b'a');
        if !class.is_empty() {
            out.line("AV", format!("{class} {}", field.value(b'b')).trim_end());
        }
    }
    if let Some(field) = first_field(record, *b"082") {
        let class = field.value(b'a');
        if !class.is_empty() {
            out.line("U1", format!("{class} {}", field.value(b'2')).trim_end());
        }
    }
}

/// Koha: `C4::Ris::print_pubinfo`: a `PY` for each date as it comes, then the places
/// and the publishers.
fn publication_lines(field: DataField<'_>, out: &mut Lines) {
    let mut places = Vec::new();
    let mut publishers = Vec::new();
    for subfield in field.subfields {
        let value = super::text(&subfield.value);
        match subfield.code {
            b'a' => places.push(strip_one(&value, ",;:/")),
            b'b' => publishers.push(strip_one(&value, ",;:/")),
            b'c' => out.line("PY", &year(&value)),
            _ => {}
        }
    }
    out.line("CY", &places.join(", "));
    out.line("PB", &publishers.join(", "));
}

/// `s% *[<set>]$%%`: one closing mark of `set` and the spaces before it.
fn strip_one(text: &str, set: &str) -> String {
    match text.chars().last() {
        Some(last) if set.contains(last) => {
            let mut chars = text.chars();
            chars.next_back();
            chars.as_str().trim_end_matches(' ').to_owned()
        }
        _ => text.to_owned(),
    }
}

/// The RIS date: the four-digit year, `///` and the rest; `0000///` and the text when
/// there is no year.
fn year(date: &str) -> String {
    let date = strip_end(date, ".;:/");
    let chars: Vec<char> = date.chars().collect();
    let year_char = |c: &char| c.is_ascii_digit() || *c == '-';
    // `s%\D*([0-9\-]{4})(.*)%$1///$2%`, at the leftmost place it matches.
    let found = (0..chars.len()).find_map(|start| {
        let non_digits = chars
            .iter()
            .skip(start)
            .take_while(|c| !c.is_ascii_digit())
            .count();
        (0..=non_digits).rev().find_map(|skip| {
            let from = start + skip;
            let four = chars.get(from..from + 4)?;
            four.iter().all(year_char).then_some((start, from))
        })
    });
    let mut written: Vec<char> = match found {
        Some((start, from)) => {
            let mut out: Vec<char> = chars.iter().take(start).copied().collect();
            out.extend(chars.iter().skip(from).take(4));
            out.extend("///".chars());
            out.extend(chars.iter().skip(from + 4));
            out
        }
        None => chars,
    };
    if written.iter().position(|c| *c == '/') == Some(4) {
        for c in written.iter_mut().take(4) {
            if *c == '-' {
                *c = '0';
            }
        }
        written.into_iter().collect()
    } else {
        format!("0000///{}", written.into_iter().collect::<String>())
    }
}

/// Koha: `C4::Ris::get_keywords`, every subject field, without repeats.
fn keywords(record: &Record) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for tag in SUBJECTS {
        for field in fields(record, *tag) {
            let values: Vec<String> = if tag == b"600" {
                vec![author(field)]
            } else {
                field
                    .subfields
                    .iter()
                    .filter(|subfield| KEYWORD_CODES.contains(&subfield.code))
                    .map(|subfield| strip_end(&super::text(&subfield.value), ",;.:/"))
                    .collect()
            };
            for value in values {
                if !value.is_empty() && !found.contains(&value) {
                    found.push(value);
                }
            }
        }
    }
    found
}

/// Koha: `C4::Ris::pool_subx`, the notes in Koha's order of tags.
fn notes(record: &Record) -> Vec<String> {
    let mut pool = Vec::new();
    for (tag, codes) in NOTES {
        for field in fields(record, *tag) {
            for subfield in field.subfields {
                if codes.contains(&subfield.code) {
                    let value = strip_end(&super::text(&subfield.value), ",;.:/");
                    if !value.is_empty() {
                        pool.push(value);
                    }
                }
            }
        }
    }
    pool
}

/// Koha: `C4::Ris::print_abstract`, the first `$a` and `$b` of each `520`.
fn abstracts(record: &Record) -> Vec<String> {
    let mut found = Vec::new();
    for field in fields(record, *b"520") {
        for code in *b"ab" {
            let value = strip_end(&field.value(code), ";,:./");
            if !value.is_empty() {
                found.push(value);
            }
        }
    }
    found
}
