// SPDX-License-Identifier: GPL-3.0-or-later
//! The author notation of a call number from the library's own author table, such as
//! the Tabela PHA or a Cutter-Sanborn table (docs/specs/catalogo/numero-de-chamada.md).
//!
//! The tables are copyrighted and never ship with Elzevir: the library gives its copy as
//! text, one row per line, and [`AuthorTable::parse`] reads it. The rules follow the
//! explanation of the PHA table: the entry right before the name in alphabetical order,
//! the initial of the surname, the number and the initial of the title without its
//! initial article.

use std::collections::{BTreeMap, BTreeSet};

/// Most lines a table may have; the printed tables have a few thousand.
pub const MAX_TABLE_LINES: usize = 100_000;

/// Most bytes of one line of a table.
const MAX_LINE: usize = 512;

/// Most lines kept in each list of a [`TableReport`].
const MAX_REPORTED: usize = 30;

/// One entry of the table.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    /// The entry as the sort compares it ([`name_key`]).
    key: String,
    /// The number, as printed.
    number: String,
    /// The entry as printed.
    text: String,
}

/// An author table, its entries by initial letter and in key order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuthorTable {
    letters: BTreeMap<char, Vec<Entry>>,
}

/// What reading a table found, for the library to check against the book.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TableReport {
    /// Lines with an entry or a row (blank lines and `#` comments do not count).
    pub lines: usize,
    /// Entries kept.
    pub entries: usize,
    /// Lines with no number or no entry, by line number (the first 30).
    pub not_understood: Vec<usize>,
    /// Entries whose number comes before the number of the entry before them, which is a
    /// typing error in a decimal table (the first 30), by line number.
    pub out_of_order: Vec<usize>,
    /// Entries already in the table, by line number (the first 30); the first one stays.
    pub repeated: Vec<usize>,
}

/// Why a table could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TableError {
    /// More lines than [`MAX_TABLE_LINES`].
    #[error("the table has more than {MAX_TABLE_LINES} lines")]
    TooLong,
    /// No line gave an entry.
    #[error("the table has no entries")]
    Empty,
}

/// How the record is entered: the main entry decides the letter and the number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainEntry<'a> {
    /// A person or a family, as `Surname, Forename`.
    Person(&'a str),
    /// An organization, a meeting or a jurisdiction: its first word.
    Corporate(&'a str),
    /// No author: the first word of the title, with no mark of the title.
    Title,
}

/// The author notation of a record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notation {
    /// The notation, such as `A848d`.
    pub notation: String,
    /// The entry of the table it comes from, as printed.
    pub entry: String,
    /// The entry's number.
    pub number: String,
}

/// Why there is no notation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NotationError {
    /// The name (or the title) does not start with a letter.
    #[error("the name does not start with a letter")]
    NoLetter,
    /// The table has no entry for the name's letter.
    #[error("the table has no entry for the letter")]
    NoEntryForLetter,
}

impl AuthorTable {
    /// Reads a table given as text, one row per line: `entry number`, `number entry`, or
    /// a row of the PHA book, `entry number entry`; tabs or `;` may separate the parts,
    /// and a comma when the number is next to it (`Sampaio M.,184`).
    ///
    /// # Errors
    ///
    /// [`TableError`] when the text is too long or has no entries.
    pub fn parse(text: &str) -> Result<(Self, TableReport), TableError> {
        let mut report = TableReport::default();
        let mut seen: BTreeMap<char, BTreeSet<String>> = BTreeMap::new();
        let mut rows: BTreeMap<char, Vec<(Entry, usize)>> = BTreeMap::new();
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        for (index, line) in text.lines().enumerate() {
            let number = index + 1;
            if number > MAX_TABLE_LINES {
                return Err(TableError::TooLong);
            }
            let line = line.trim_end_matches('\r');
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            report.lines += 1;
            let Some((left, digits, right)) = split_row(line).filter(|_| line.len() <= MAX_LINE)
            else {
                note(&mut report.not_understood, number);
                continue;
            };
            for side in [left, right] {
                let side = side.trim();
                if side.is_empty() {
                    continue;
                }
                let key = name_key(side);
                let Some(letter) = key.chars().next().filter(char::is_ascii_lowercase) else {
                    note(&mut report.not_understood, number);
                    continue;
                };
                let keys = seen.entry(letter).or_default();
                if !keys.insert(key.clone()) {
                    note(&mut report.repeated, number);
                    continue;
                }
                rows.entry(letter).or_default().push((
                    Entry {
                        key,
                        number: digits.to_owned(),
                        text: side.to_owned(),
                    },
                    number,
                ));
            }
        }
        let mut letters = BTreeMap::new();
        for (letter, mut entries) in rows {
            // Stable: a key repeated is refused above, so the order is total.
            entries.sort_by(|a, b| a.0.key.cmp(&b.0.key));
            // Decimal numbering (1, 11, 111, 112 ... 12) compares as text.
            for pair in entries.windows(2) {
                if let [(before, _), (entry, line)] = pair
                    && entry.number < before.number
                {
                    note(&mut report.out_of_order, *line);
                }
            }
            report.entries += entries.len();
            letters.insert(
                letter,
                entries.into_iter().map(|(entry, _)| entry).collect(),
            );
        }
        if report.entries == 0 {
            return Err(TableError::Empty);
        }
        Ok((Self { letters }, report))
    }

    /// The notation of a record entered under `entry`, with `title` the title proper.
    ///
    /// # Errors
    ///
    /// [`NotationError`] when the name has no letter or the table none of its entries.
    pub fn notation(&self, entry: MainEntry<'_>, title: &str) -> Result<Notation, NotationError> {
        let (key, mark) = match entry {
            MainEntry::Person(name) => (author_key(name), work_mark(title)),
            MainEntry::Corporate(name) => (name_key(first_word(name)), work_mark(title)),
            MainEntry::Title => (name_key(first_word(title_rest(title))), String::new()),
        };
        let letter = key
            .chars()
            .next()
            .filter(char::is_ascii_lowercase)
            .ok_or(NotationError::NoLetter)?;
        let entries = self
            .letters
            .get(&letter)
            .ok_or(NotationError::NoEntryForLetter)?;
        // The last entry at or before the name; a name before them all takes the first.
        let found = entries
            .iter()
            .take_while(|candidate| candidate.key <= key)
            .last()
            .or_else(|| entries.first())
            .ok_or(NotationError::NoEntryForLetter)?;
        Ok(Notation {
            notation: format!("{}{}{mark}", letter.to_ascii_uppercase(), found.number),
            entry: found.text.clone(),
            number: found.number.clone(),
        })
    }
}

fn note(list: &mut Vec<usize>, line: usize) {
    if list.len() < MAX_REPORTED {
        list.push(line);
    }
}

/// The text before the number, the number, and the text after it.
fn split_row(line: &str) -> Option<(&str, &str, &str)> {
    let separated: Vec<&str> = if line.contains(['\t', ';']) {
        line.split(['\t', ';']).collect()
    } else if line.contains(',') && line.split(',').any(|part| is_number(part.trim())) {
        line.split(',').collect()
    } else {
        line.split(' ').filter(|part| !part.is_empty()).collect()
    };
    let at = separated.iter().position(|part| is_number(part.trim()))?;
    let number = separated.get(at)?.trim();
    let start = line.find(number)?;
    let left = line.get(..start)?;
    let right = line.get(start + number.len()..)?;
    let (left, right) = (clean(left), clean(right));
    (!left.is_empty() || !right.is_empty()).then_some((left, number, right))
}

/// A side of a row without the separators around it.
fn clean(side: &str) -> &str {
    side.trim().trim_matches([',', ';', '\t', '-']).trim()
}

fn is_number(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

/// A Latin letter without its accent; other characters as they are.
fn unaccent(c: char) -> char {
    match c {
        'á' | 'à' | 'â' | 'ã' | 'ä' | 'Á' | 'À' | 'Â' | 'Ã' | 'Ä' => 'a',
        'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => 'e',
        'í' | 'ì' | 'î' | 'ï' | 'Í' | 'Ì' | 'Î' | 'Ï' => 'i',
        'ó' | 'ò' | 'ô' | 'õ' | 'ö' | 'Ó' | 'Ò' | 'Ô' | 'Õ' | 'Ö' => 'o',
        'ú' | 'ù' | 'û' | 'ü' | 'Ú' | 'Ù' | 'Û' | 'Ü' => 'u',
        'ç' | 'Ç' => 'c',
        'ñ' | 'Ñ' => 'n',
        'ý' | 'ÿ' | 'Ý' => 'y',
        other => other,
    }
}

/// Lower case without accents, only letters, digits and single spaces.
fn folded(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars().map(unaccent).flat_map(char::to_lowercase) {
        if c == '\'' || c == '’' {
            continue;
        }
        let c = if c.is_ascii_lowercase() || c.is_ascii_digit() {
            c
        } else {
            ' '
        };
        if !(c == ' ' && (out.is_empty() || out.ends_with(' '))) {
            out.push(c);
        }
    }
    out.trim_end().to_owned()
}

/// Prefixes read as one word with the word after them (`Van der Berg`, `La Fonte`).
const JOINED: [&str; 8] = [
    "van der", "van den", "van de", "de la", "de las", "de los", "von der", "von",
];
const PREFIXES: [&str; 20] = [
    "la", "le", "les", "du", "des", "van", "ten", "ter", "del", "della", "delle", "di", "da", "de",
    "dos", "das", "do", "lo", "el", "al",
];

/// A name as the tables sort it: lower case, no accents, `M'` and `Mc` as `Mac`, a
/// prefix joined to the word after it, only letters and single spaces.
#[must_use]
pub fn name_key(name: &str) -> String {
    let trimmed = name.trim();
    let lower = trimmed.to_lowercase();
    let mac = lower
        .strip_prefix("m'")
        .or_else(|| lower.strip_prefix("m’"))
        .or_else(|| lower.strip_prefix("mc"))
        .map(|rest| format!("mac{rest}"));
    let mut key = folded(mac.as_deref().unwrap_or(trimmed));
    let joined = |key: &str, prefix: &str| {
        key.strip_prefix(prefix)
            .and_then(|rest| rest.strip_prefix(' '))
            .filter(|rest| rest.starts_with(|c: char| c.is_ascii_lowercase()))
            .map(|rest| format!("{}{rest}", prefix.replace(' ', "")))
    };
    if let Some(one) = JOINED.iter().find_map(|prefix| joined(&key, prefix)) {
        key = one;
    } else if let Some(one) = PREFIXES.iter().find_map(|prefix| joined(&key, prefix)) {
        key = one;
    }
    key
}

/// `Surname, Forename` as the key of the surname followed by the forename.
fn author_key(name: &str) -> String {
    match name.split_once(',') {
        None => name_key(name),
        Some((surname, forename)) => {
            let (surname, forename) = (name_key(surname), name_key(forename));
            if forename.is_empty() {
                surname
            } else {
                format!("{surname} {forename}")
            }
        }
    }
}

const ARTICLES: [&str; 26] = [
    "o", "a", "os", "as", "um", "uma", "uns", "umas", "the", "an", "el", "la", "los", "las", "lo",
    "le", "les", "un", "une", "il", "gli", "der", "die", "das", "ein", "eine",
];

/// The title without its initial article.
fn title_rest(title: &str) -> &str {
    let title = title.trim();
    if let Some(rest) = title
        .get(..2)
        .filter(|start| start.eq_ignore_ascii_case("l'"))
        .and_then(|_| title.get(2..))
    {
        return rest.trim();
    }
    match title.split_once([' ', '\t']) {
        Some((first, rest)) if ARTICLES.contains(&folded(first).as_str()) => rest.trim(),
        _ => title,
    }
}

fn first_word(text: &str) -> &str {
    text.trim()
        .split([' ', '\t', ','])
        .next()
        .unwrap_or_default()
}

/// The initial of the title without its article; a title in `l` gets a capital `L`, so
/// it is not read as the digit 1.
fn work_mark(title: &str) -> String {
    folded(title_rest(title))
        .chars()
        .find(|c| *c != ' ')
        .map(|c| if c == 'l' { 'L' } else { c })
        .map(String::from)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A made-up table in the layouts the books print; the real tables are copyrighted.
    const TABLE: &str = "\u{feff}# made up\r\n111 Aa\n  127     Abbot, J.\n  847     Ass\n  848     Assi\n  849     Ast\nEa 18 Ec\n19\tEd\nLem;588\nLent 589 Leo\nSampaio M.,184\nMac 15\nMad 16\nFontes 32\nLafo 31\nzz\n";

    fn table() -> AuthorTable {
        AuthorTable::parse(TABLE).unwrap().0
    }

    #[test]
    fn every_layout_of_a_row_is_read() {
        let (_, report) = AuthorTable::parse(TABLE).unwrap();
        assert_eq!(report.lines, 15);
        assert_eq!(report.entries, 16);
        assert_eq!(report.not_understood, [16]);
        assert!(report.repeated.is_empty());
    }

    #[test]
    fn the_notation_is_the_entry_before_the_name_and_the_title_initial() {
        let notation = table()
            .notation(MainEntry::Person("Assis, Machado de"), "Dom Casmurro")
            .unwrap();
        assert_eq!(notation.notation, "A848d");
        assert_eq!(notation.entry, "Assi");
        // The initial article does not count; a title in `l` gets a capital L.
        let rose = table()
            .notation(MainEntry::Person("Eco, Umberto"), "O nome da rosa")
            .unwrap();
        assert_eq!(rose.notation, "E18n");
        let lusiadas = table()
            .notation(MainEntry::Person("Assis, Ana"), "Os lusíadas")
            .unwrap();
        assert_eq!(lusiadas.notation, "A848L");
        // A name before every entry of its letter takes the first.
        let first = table()
            .notation(MainEntry::Person("A, B"), "Título")
            .unwrap();
        assert_eq!(first.notation, "A111t");
    }

    #[test]
    fn prefixes_mac_institutions_and_titles() {
        assert_eq!(name_key("McDonald"), "macdonald");
        assert_eq!(name_key("M'Intyre"), "macintyre");
        assert_eq!(name_key("La Fontaine"), "lafontaine");
        assert_eq!(name_key("Van der Berg"), "vanderberg");
        assert_eq!(name_key("O'Donnel"), "odonnel");
        assert_eq!(name_key("Ávila,  José"), "avila jose");
        assert_eq!(
            table()
                .notation(MainEntry::Person("McDonald, Ann"), "Avenida")
                .unwrap()
                .notation,
            "M15a"
        );
        assert_eq!(
            table()
                .notation(MainEntry::Person("La Fontaine, Jean de"), "Fábulas")
                .unwrap()
                .notation,
            "L31f"
        );
        assert_eq!(
            table()
                .notation(MainEntry::Corporate("Leopoldina Editora"), "Relatório")
                .unwrap()
                .notation,
            "L589r"
        );
        assert_eq!(
            table()
                .notation(MainEntry::Title, "A estatística")
                .unwrap()
                .notation,
            "E19"
        );
    }

    #[test]
    fn missing_letters_and_bad_tables_are_errors() {
        assert_eq!(
            table().notation(MainEntry::Person("Quintana, Mário"), "Poemas"),
            Err(NotationError::NoEntryForLetter)
        );
        assert_eq!(
            table().notation(MainEntry::Person("1984"), "x"),
            Err(NotationError::NoLetter)
        );
        assert_eq!(AuthorTable::parse("# nothing\n\n"), Err(TableError::Empty));
        let (_, report) = AuthorTable::parse("Ab 12\nAc 11\nAb 13\n").unwrap();
        assert_eq!(report.out_of_order, [2]);
        assert_eq!(report.repeated, [3]);
    }
}
