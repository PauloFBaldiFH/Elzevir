// SPDX-License-Identifier: GPL-3.0-or-later
//! The names of the code tables: what `catalog.name_codes` stores and how Koha's REST
//! answers become its input (docs/specs/catalogo/tabelas-de-codigos.md).

use serde::{Deserialize, Serialize};

/// Longest code a table accepts (item 3).
pub const MAX_CODE: usize = 64;
/// Longest name a table accepts (item 3).
pub const MAX_NAME: usize = 256;
/// Largest Koha answer read (item 10).
pub const MAX_ANSWER_BYTES: usize = 4 * 1024 * 1024;
/// Most entries read from one Koha answer (item 10).
pub const MAX_ENTRIES: usize = 10_000;

/// One of the four code tables (item 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CodeTable {
    /// Branches.
    Branch,
    /// Item types.
    ItemType,
    /// Shelving locations.
    Location,
    /// Collections.
    Collection,
}

impl CodeTable {
    /// Every table, in the order reports list them.
    pub const ALL: [Self; 4] = [
        Self::Branch,
        Self::ItemType,
        Self::Location,
        Self::Collection,
    ];

    /// The table's name in reports.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Branch => "branches",
            Self::ItemType => "item types",
            Self::Location => "locations",
            Self::Collection => "collections",
        }
    }
}

/// A code and the name it should have.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamedCode {
    /// The table.
    pub table: CodeTable,
    /// The code, as records carry it.
    pub code: String,
    /// The name people read.
    pub name: String,
}

/// What `catalog.name_codes` did, per table (item 4).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NamingCount {
    /// Codes created with their name.
    pub created: usize,
    /// Codes whose name changed.
    pub renamed: usize,
    /// Codes that already had this name.
    pub unchanged: usize,
}

/// The codes trimmed as the command stores them, or the position of the first one it
/// refuses (item 3).
///
/// # Errors
///
/// The zero-based position of a code or name that is empty or too long.
pub fn checked(codes: &[NamedCode]) -> Result<Vec<NamedCode>, usize> {
    codes
        .iter()
        .enumerate()
        .map(|(position, entry)| {
            let code = entry.code.trim();
            let name = entry.name.trim();
            let fits = |text: &str, most: usize| !text.is_empty() && text.chars().count() <= most;
            if fits(code, MAX_CODE) && fits(name, MAX_NAME) {
                Ok(NamedCode {
                    table: entry.table,
                    code: code.to_owned(),
                    name: name.to_owned(),
                })
            } else {
                Err(position)
            }
        })
        .collect()
}

/// Where each table is read from Koha and which fields hold the code and the name
/// (item 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KohaSource {
    /// The table filled.
    pub table: CodeTable,
    /// The REST path, under the staff URL.
    pub path: &'static str,
    /// The field with the code.
    pub code_field: &'static str,
    /// The field with the name.
    pub name_field: &'static str,
}

/// The four Koha answers read (item 8).
pub const KOHA_SOURCES: [KohaSource; 4] = [
    KohaSource {
        table: CodeTable::Branch,
        path: "/api/v1/libraries?_per_page=-1",
        code_field: "library_id",
        name_field: "name",
    },
    KohaSource {
        table: CodeTable::ItemType,
        path: "/api/v1/item_types?_per_page=-1",
        code_field: "item_type_id",
        name_field: "description",
    },
    KohaSource {
        table: CodeTable::Location,
        path: "/api/v1/authorised_value_categories/LOC/authorised_values?_per_page=-1",
        code_field: "value",
        name_field: "description",
    },
    KohaSource {
        table: CodeTable::Collection,
        path: "/api/v1/authorised_value_categories/CCODE/authorised_values?_per_page=-1",
        code_field: "value",
        name_field: "description",
    },
];

/// One Koha answer read: the codes with a name, and how many entries were skipped.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KohaTable {
    /// Codes and names, in Koha's order.
    pub codes: Vec<NamedCode>,
    /// Entries without a code or a name (item 10).
    pub skipped: usize,
}

/// A Koha answer that cannot be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KohaAnswerError {
    /// Larger than [`MAX_ANSWER_BYTES`].
    #[error("the answer has more than {MAX_ANSWER_BYTES} bytes")]
    TooLarge,
    /// More entries than [`MAX_ENTRIES`].
    #[error("the answer has more than {MAX_ENTRIES} entries")]
    TooManyEntries,
    /// Not a JSON list of objects.
    #[error("the answer is not a JSON list: {0}")]
    NotAList(String),
}

/// Reads one Koha answer for `source` (items 8 to 10).
///
/// # Errors
///
/// When the answer is too large or is not a JSON list of objects.
pub fn koha_table(source: &KohaSource, answer: &str) -> Result<KohaTable, KohaAnswerError> {
    if answer.len() > MAX_ANSWER_BYTES {
        return Err(KohaAnswerError::TooLarge);
    }
    let entries: Vec<serde_json::Map<String, serde_json::Value>> =
        serde_json::from_str(answer).map_err(|e| KohaAnswerError::NotAList(e.to_string()))?;
    if entries.len() > MAX_ENTRIES {
        return Err(KohaAnswerError::TooManyEntries);
    }
    let mut table = KohaTable::default();
    for entry in &entries {
        let text = |field: &str| {
            entry
                .get(field)
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
        };
        match (text(source.code_field), text(source.name_field)) {
            (Some(code), Some(name)) => table.codes.push(NamedCode {
                table: source.table,
                code: code.to_owned(),
                name: name.to_owned(),
            }),
            _ => table.skipped += 1,
        }
    }
    Ok(table)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The oracle's answers on 2026-11-14, reduced to the fields read plus one other.
    const LIBRARIES: &str = include_str!("../tests/data/koha-libraries.json");
    const ITEM_TYPES: &str = include_str!("../tests/data/koha-item-types.json");
    const LOCATIONS: &str = include_str!("../tests/data/koha-loc.json");
    const COLLECTIONS: &str = include_str!("../tests/data/koha-ccode.json");

    fn read(table: CodeTable, answer: &str) -> KohaTable {
        let source = KOHA_SOURCES
            .iter()
            .find(|source| source.table == table)
            .unwrap();
        koha_table(source, answer).unwrap()
    }

    fn pairs(table: &KohaTable) -> Vec<(&str, &str)> {
        table
            .codes
            .iter()
            .map(|entry| (entry.code.as_str(), entry.name.as_str()))
            .collect()
    }

    #[test]
    fn the_oracle_answers_give_every_name() {
        assert_eq!(
            pairs(&read(CodeTable::Branch, LIBRARIES)),
            [("MAIN", "Castro Alves")]
        );
        let item_types = read(CodeTable::ItemType, ITEM_TYPES);
        assert_eq!(item_types.codes.len(), 8);
        assert!(pairs(&item_types).contains(&("BK", "Livros")));
        assert!(
            pairs(&read(CodeTable::Location, LOCATIONS)).contains(&("GEN", "(Estantes gerais)"))
        );
        assert_eq!(
            pairs(&read(CodeTable::Collection, COLLECTIONS)),
            [
                ("FIC", "Ficção"),
                ("NFIC", "Não ficção"),
                ("REF", "Referência")
            ]
        );
    }

    #[test]
    fn entries_without_code_or_name_are_skipped() {
        let table = read(
            CodeTable::Location,
            r#"[{"value": " A ", "description": " Acervo "}, {"value": "B", "description": null},
                {"value": "", "description": "Vazio"}, {"description": "Sem código"}]"#,
        );
        assert_eq!(pairs(&table), [("A", "Acervo")]);
        assert_eq!(table.skipped, 3);
    }

    #[test]
    fn an_answer_that_is_not_a_list_or_too_large_is_refused() {
        let source = &KOHA_SOURCES[0];
        assert!(matches!(
            koha_table(source, r#"{"error": "Authentication failure."}"#),
            Err(KohaAnswerError::NotAList(_))
        ));
        let large = format!("[{}]", " ".repeat(MAX_ANSWER_BYTES));
        assert_eq!(koha_table(source, &large), Err(KohaAnswerError::TooLarge));
        let many = format!("[{}{{}}]", "{},".repeat(MAX_ENTRIES));
        assert_eq!(
            koha_table(source, &many),
            Err(KohaAnswerError::TooManyEntries)
        );
    }

    #[test]
    fn codes_are_trimmed_and_empty_or_long_ones_refused() {
        let entry = |code: &str, name: &str| NamedCode {
            table: CodeTable::Branch,
            code: code.to_owned(),
            name: name.to_owned(),
        };
        assert_eq!(
            checked(&[entry(" MAIN ", " Castro Alves ")]),
            Ok(vec![entry("MAIN", "Castro Alves")])
        );
        assert_eq!(checked(&[entry("MAIN", "x"), entry("PAL", "  ")]), Err(1));
        assert_eq!(checked(&[entry(&"C".repeat(MAX_CODE + 1), "x")]), Err(0));
        assert_eq!(checked(&[entry("C", &"n".repeat(MAX_NAME + 1))]), Err(0));
        assert_eq!(
            checked(&[entry("C", &"ç".repeat(MAX_NAME))]).map(|c| c.len()),
            Ok(1)
        );
    }
}
