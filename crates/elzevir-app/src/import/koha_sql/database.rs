// SPDX-License-Identifier: GPL-3.0-or-later
//! The catalog of a whole Koha database dump, for `switch-from-koha`
//! (docs/specs/migracao/troca-do-koha.md, items 7 and 8).

use std::collections::{BTreeMap, BTreeSet};
use std::io::BufRead;

use elzevir_marc::{Field, Tag};

use super::{
    CatalogRows, Cells, Column, DumpError, DumpProblem, MAX_DATABASE_DUMP_BYTES, MetadataRecord,
    Table, Target,
};

/// Most rows of `itemtypes`.
const MAX_ITEM_TYPES: usize = 10_000;
/// Most characters of an item type code or description kept.
const MAX_ITEM_TYPE_CHARS: usize = 256;
const ITEM_TAG: Tag = Tag::new(*b"952");
const BIBLIO_TAG: Tag = Tag::new(*b"999");

/// The records and item types of a whole Koha database.
pub struct DatabaseCatalog {
    pub(in crate::import) records: Vec<MetadataRecord>,
    /// `itemtypes`: each code and its description.
    pub item_types: BTreeMap<String, String>,
}

impl std::fmt::Debug for DatabaseCatalog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DatabaseCatalog")
            .field("records", &self.records.len())
            .field("item_types", &self.item_types)
            .finish()
    }
}

impl DatabaseCatalog {
    /// How many bibliographic records the dump has.
    #[must_use]
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Whether the dump has no bibliographic record.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// The item type codes the records' Items use (`952$y`), each once.
    #[must_use]
    pub fn item_type_codes(&self) -> BTreeSet<String> {
        let mut codes = BTreeSet::new();
        for record in &self.records {
            for field in &record.record.fields {
                if let Field::Data { tag, subfields, .. } = field
                    && *tag == ITEM_TAG
                {
                    codes.extend(
                        subfields
                            .iter()
                            .filter(|subfield| subfield.code == b'y')
                            .filter_map(|subfield| std::str::from_utf8(&subfield.value).ok())
                            .map(str::trim)
                            .filter(|code| !code.is_empty())
                            .map(str::to_owned),
                    );
                }
            }
        }
        codes
    }

    /// Each record's Koha numbers and the Item values the verifier compares, in import
    /// order: the `biblionumber` (first `999 $c`) and, for each `952`, `$9`, `$p`, `$a`, `$b`
    /// and `$y` (verificacao-do-koha.md, items 7 and 8).
    #[must_use]
    pub fn koha_records(&self) -> Vec<KohaCatalogRecord> {
        self.records
            .iter()
            .map(|record| {
                let mut entry = KohaCatalogRecord::default();
                for field in &record.record.fields {
                    let Field::Data { tag, subfields, .. } = field else {
                        continue;
                    };
                    let value = |code: u8| {
                        subfields
                            .iter()
                            .find(|subfield| subfield.code == code)
                            .and_then(|subfield| std::str::from_utf8(&subfield.value).ok())
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .map(str::to_owned)
                    };
                    if *tag == BIBLIO_TAG && entry.biblionumber.is_none() {
                        entry.biblionumber = value(b'c');
                    } else if *tag == ITEM_TAG {
                        entry.items.push(KohaCatalogItem {
                            itemnumber: value(b'9'),
                            barcode: value(b'p'),
                            home: value(b'a'),
                            holding: value(b'b'),
                            item_type: value(b'y'),
                        });
                    }
                }
                entry
            })
            .collect()
    }
}

/// A record of the dump by its Koha numbers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KohaCatalogRecord {
    /// `biblionumber`, as `999 $c` carries it.
    pub biblionumber: Option<String>,
    /// Its Items, in `952` order.
    pub items: Vec<KohaCatalogItem>,
}

/// An Item of the dump, as its `952` carries it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KohaCatalogItem {
    /// `itemnumber` (`$9`).
    pub itemnumber: Option<String>,
    /// `barcode` (`$p`).
    pub barcode: Option<String>,
    /// `homebranch` (`$a`).
    pub home: Option<String>,
    /// `holdingbranch` (`$b`).
    pub holding: Option<String>,
    /// `itype` (`$y`).
    pub item_type: Option<String>,
}

/// Reads `biblio_metadata`, `items` and `itemtypes` from a whole Koha database dump,
/// skipping the other tables without keeping their values (item 8).
///
/// # Errors
/// A located, value-free grammar, budget, inventory or MARCXML failure, or a dump
/// without `biblio_metadata`.
pub fn read_database_catalog(input: impl BufRead) -> Result<DatabaseCatalog, DumpError> {
    let target = super::read(input, DatabaseTarget::default())?;
    Ok(DatabaseCatalog {
        records: super::assemble::records(target.rows)?,
        item_types: target.item_types,
    })
}

#[derive(Default)]
struct DatabaseTarget {
    rows: CatalogRows,
    item_types: BTreeMap<String, String>,
    declares_metadata: bool,
}

impl Target for DatabaseTarget {
    type Table = Table;
    type Column = Column;
    const WHOLE_DATABASE: bool = true;
    const MAX_BYTES: u64 = MAX_DATABASE_DUMP_BYTES;

    fn table(&mut self, name: &str, declared: bool) -> Result<Option<Table>, DumpProblem> {
        let table = Table::from_name(name);
        self.declares_metadata |= declared && table == Table::Metadata;
        Ok(Some(table).filter(|table| *table != Table::Ignored))
    }

    fn column(&self, table: Table, name: &str) -> Option<Column> {
        Some(table.column(name)).filter(|column| *column != Column::Ignored)
    }

    fn has_keys(&self, table: Table, layout: &[Option<Column>]) -> bool {
        let layout: Vec<Column> = layout
            .iter()
            .map(|column| column.unwrap_or(Column::Ignored))
            .collect();
        table.has_keys(&layout)
    }

    fn push(&mut self, table: Table, offset: u64, cells: Cells) -> Result<(), DumpError> {
        if table != Table::ItemTypes {
            return self.rows.push(table, offset, cells);
        }
        let error = |problem| DumpError { offset, problem };
        if self.item_types.len() == MAX_ITEM_TYPES {
            return Err(error(DumpProblem::Limit("item types")));
        }
        let mut code = None;
        let mut description = None;
        for (column, value) in cells {
            let text = value.and_then(|value| String::from_utf8(value).ok());
            match column {
                Column::ItemType => code = text,
                Column::Description => description = text,
                _ => {}
            }
        }
        let code = code
            .map(|code| code.trim().to_owned())
            .filter(|code| !code.is_empty() && code.chars().count() <= MAX_ITEM_TYPE_CHARS)
            .ok_or_else(|| error(DumpProblem::Identifier("itemtype")))?;
        let description: String = description
            .map(|description| description.trim().to_owned())
            .filter(|description| !description.is_empty())
            .unwrap_or_else(|| code.clone())
            .chars()
            .take(MAX_ITEM_TYPE_CHARS)
            .collect();
        if self.item_types.insert(code, description).is_some() {
            return Err(error(DumpProblem::Duplicate("itemtype")));
        }
        Ok(())
    }

    fn finish(&self, _any: bool) -> Result<(), DumpProblem> {
        if self.declares_metadata {
            Ok(())
        } else {
            Err(DumpProblem::MissingCatalog)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = "<record xmlns=\"http://www.loc.gov/MARC21/slim\"><leader>00000nam a2200000 a 4500</leader><datafield tag=\"245\" ind1=\"0\" ind2=\"0\"><subfield code=\"a\">Livro</subfield></datafield><datafield tag=\"999\" ind1=\" \" ind2=\" \"><subfield code=\"c\">1</subfield><subfield code=\"d\">1</subfield></datafield></record>";

    fn dump() -> String {
        format!(
            "CREATE DATABASE `koha_library`;\nUSE `koha_library`;\n\
             CREATE TABLE `borrowers` (`borrowernumber` int, `surname` text);\n\
             INSERT INTO `borrowers` VALUES (1,'Fictícia');\n\
             CREATE TABLE `issues` (`issue_id` int);\n\
             CREATE TABLE `systempreferences` (`variable` text, `value` text);\n\
             CREATE TABLE `itemtypes` (`itemtype` varchar(10), `description` text, `rentalcharge` decimal(28,6));\n\
             INSERT INTO `itemtypes` VALUES ('LIV','Livros',0.000000),('DVD',NULL,0.000000);\n\
             CREATE TABLE `biblio_metadata` (`id` int, `biblionumber` int, `format` text, `schema` text, `metadata` longtext);\n\
             INSERT INTO `biblio_metadata` VALUES (1,1,'marcxml','MARC21','{XML}');\n\
             CREATE TABLE `items` (`itemnumber` int, `biblionumber` int, `barcode` text, `homebranch` text, `holdingbranch` text, `itype` text);\n\
             INSERT INTO `items` VALUES (10,1,'0001','BR1','BR1','LIV');\n"
        )
    }

    #[test]
    fn the_catalog_of_a_whole_database_is_read_and_the_rest_skipped() {
        let catalog = read_database_catalog(dump().as_bytes()).unwrap();
        assert_eq!(catalog.len(), 1);
        assert_eq!(
            catalog.item_types,
            BTreeMap::from([
                ("DVD".to_owned(), "DVD".to_owned()),
                ("LIV".to_owned(), "Livros".to_owned())
            ])
        );
        assert_eq!(
            catalog.item_type_codes(),
            BTreeSet::from(["LIV".to_owned()])
        );
        assert_eq!(
            catalog.koha_records(),
            vec![KohaCatalogRecord {
                biblionumber: Some("1".to_owned()),
                items: vec![KohaCatalogItem {
                    itemnumber: Some("10".to_owned()),
                    barcode: Some("0001".to_owned()),
                    home: Some("BR1".to_owned()),
                    holding: Some("BR1".to_owned()),
                    item_type: Some("LIV".to_owned()),
                }],
            }]
        );
        // The partial-dump reader still refuses a whole database.
        assert!(super::super::prepare(dump().as_bytes()).is_err());
    }

    #[test]
    fn a_database_without_records_or_with_a_repeated_type_is_refused() {
        let without = "CREATE TABLE `borrowers` (`borrowernumber` int);\n";
        assert_eq!(
            read_database_catalog(without.as_bytes())
                .unwrap_err()
                .problem,
            DumpProblem::MissingCatalog
        );
        let repeated = dump().replace("('DVD',NULL", "('LIV',NULL");
        assert_eq!(
            read_database_catalog(repeated.as_bytes())
                .unwrap_err()
                .problem,
            DumpProblem::Duplicate("itemtype")
        );
    }
}
