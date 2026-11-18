// SPDX-License-Identifier: GPL-3.0-or-later
//! Only named catalog columns reach the assembled dump.

use std::collections::{BTreeMap, BTreeSet};
use std::io::BufRead;

use super::lexer::Lexer;
use super::{CatalogRows, Cells, Column, DumpError, DumpProblem, MAX_SQL_COLUMNS, Table};
use crate::import_detection::{ImportFormat, sql::Inventory};

pub(super) fn parse<R: BufRead>(input: R) -> Result<CatalogRows, DumpError> {
    let mut parser = Parser {
        lexer: Lexer::new(input),
        schema: None,
        columns: BTreeMap::new(),
        inventory: Inventory::default(),
        rows: CatalogRows::default(),
    };
    parser.lexer.bom()?;
    let mut any = false;
    while parser.lexer.head()?.is_some() {
        if parser.lexer.accept(b';')? {
            continue;
        }
        any = true;
        parser.lexer.statement = Some(parser.lexer.offset);
        parser.statement()?;
        parser
            .lexer
            .expect(b';', "expected the statement terminator")?;
        parser.lexer.statement = None;
    }
    if any && parser.inventory.detection().format != ImportFormat::KohaSqlDump {
        return Err(parser.lexer.error(DumpProblem::MissingCatalog));
    }
    Ok(parser.rows)
}

struct Parser<R> {
    lexer: Lexer<R>,
    schema: Option<String>,
    columns: BTreeMap<Table, Vec<String>>,
    inventory: Inventory,
    rows: CatalogRows,
}

impl<R: BufRead> Parser<R> {
    fn statement(&mut self) -> Result<(), DumpError> {
        match self.lexer.keyword()?.as_str() {
            "create" => self.create(),
            "insert" => self.insert(),
            "drop" => {
                self.lexer.word("table")?;
                self.lexer.word("if")?;
                self.lexer.word("exists")?;
                let name = self.lexer.name(true)?;
                self.table(name, false)?;
                Ok(())
            }
            "lock" => {
                self.lexer.word("tables")?;
                loop {
                    let name = self.lexer.name(true)?;
                    self.table(name, false)?;
                    self.lexer.word("write")?;
                    if !self.lexer.accept(b',')? {
                        return Ok(());
                    }
                }
            }
            "unlock" => self.lexer.word("tables"),
            "set" => {
                self.lexer.word("names")?;
                let charset = self.lexer.keyword()?;
                if matches!(charset.as_str(), "utf8mb4" | "utf8mb3" | "utf8") {
                    Ok(())
                } else {
                    Err(self.lexer.syntax("only UTF-8 dump data is supported"))
                }
            }
            _ => Err(self.lexer.error(DumpProblem::Unsupported)),
        }
    }

    fn table(&mut self, name: String, declared: bool) -> Result<Table, DumpError> {
        let name = if self.lexer.accept(b'.')? {
            if self.schema.as_ref().is_some_and(|schema| *schema != name) {
                return Err(self.lexer.syntax("tables from multiple schemas"));
            }
            self.schema = Some(name);
            self.lexer.name(true)?
        } else {
            name
        };
        if declared {
            self.inventory.observe(name.as_bytes());
            if self.inventory.detection().format == ImportFormat::KohaFullBackup {
                return Err(self.lexer.error(DumpProblem::FullBackup));
            }
        }
        Ok(Table::from_name(&name.to_ascii_lowercase()))
    }

    fn create(&mut self) -> Result<(), DumpError> {
        self.lexer.word("table")?;
        let quoted = self.lexer.head()? == Some(b'`');
        let mut name = self.lexer.name(true)?;
        if !quoted && name.eq_ignore_ascii_case("if") {
            self.lexer.word("not")?;
            self.lexer.word("exists")?;
            name = self.lexer.name(true)?;
        }
        let table = self.table(name, true)?;
        self.lexer.expect(b'(', "expected table columns")?;
        let columns = self.column_names(true)?;
        self.options()?;
        if table.is_catalog() && self.columns.insert(table, columns).is_some() {
            return Err(self
                .lexer
                .error(DumpProblem::Duplicate("table declaration")));
        }
        Ok(())
    }

    fn column_names(&mut self, definitions: bool) -> Result<Vec<String>, DumpError> {
        let mut columns = Vec::new();
        let mut found = BTreeSet::new();
        loop {
            let quoted = self.lexer.head()? == Some(b'`');
            let mut name = self.lexer.name(true)?;
            name.make_ascii_lowercase();
            let constraint = definitions
                && !quoted
                && matches!(
                    name.as_str(),
                    "primary"
                        | "foreign"
                        | "constraint"
                        | "unique"
                        | "key"
                        | "index"
                        | "check"
                        | "fulltext"
                        | "spatial"
                );
            if !constraint {
                if columns.len() == MAX_SQL_COLUMNS {
                    return Err(self.lexer.error(DumpProblem::Limit("columns")));
                }
                if !found.insert(name.clone()) {
                    return Err(self.lexer.error(DumpProblem::Duplicate("column")));
                }
                columns.push(name);
                if definitions {
                    self.lexer.name(false)?;
                }
            }
            if definitions {
                self.lexer.definition_end()?;
            }
            if !self.lexer.accept(b',')? {
                break;
            }
        }
        self.lexer
            .expect(b')', "expected the end of the column list")?;
        if columns.is_empty() {
            return Err(self.lexer.syntax("the table has no columns"));
        }
        Ok(columns)
    }

    fn options(&mut self) -> Result<(), DumpError> {
        while self.lexer.head()? != Some(b';') {
            let mut option = self.lexer.keyword()?;
            if option == "default" {
                option = self.lexer.keyword()?;
            }
            match option.as_str() {
                "character" => self.lexer.word("set")?,
                "charset" | "collate" | "engine" | "auto_increment" | "row_format" | "comment" => {}
                _ => return Err(self.lexer.syntax("unsupported table option")),
            }
            self.lexer.accept(b'=')?;
            if option == "comment" {
                self.lexer.literal(false)?;
            } else {
                self.lexer.name(true)?;
            }
        }
        Ok(())
    }

    fn insert(&mut self) -> Result<(), DumpError> {
        self.lexer.word("into")?;
        let name = self.lexer.name(true)?;
        let table = self.table(name, true)?;
        let explicit = if self.lexer.accept(b'(')? {
            Some(self.column_names(false)?)
        } else {
            None
        };
        let schema = self.columns.get(&table);
        if let (Some(explicit), Some(schema)) = (&explicit, schema)
            && explicit.iter().any(|column| !schema.contains(column))
        {
            return Err(self
                .lexer
                .syntax("INSERT column absent from the table declaration"));
        }
        let columns = explicit.as_ref().or(schema);
        let layout = columns.map(|names| {
            names
                .iter()
                .map(|name| table.column(name))
                .collect::<Vec<_>>()
        });
        if table.is_catalog() && layout.as_ref().is_none_or(|layout| !table.has_keys(layout)) {
            return Err(self.lexer.error(DumpProblem::MissingColumns));
        }
        self.lexer.word("values")?;
        let mut width = layout.as_ref().map(Vec::len);
        loop {
            self.lexer.head()?;
            let offset = self.lexer.offset;
            let (count, values) = self.tuple(layout.as_deref(), width)?;
            width = Some(count);
            if table.is_catalog() {
                self.rows.push(table, offset, values)?;
            }
            if !self.lexer.accept(b',')? {
                return Ok(());
            }
        }
    }

    fn tuple(
        &mut self,
        layout: Option<&[Column]>,
        width: Option<usize>,
    ) -> Result<(usize, Cells), DumpError> {
        self.lexer.expect(b'(', "expected an INSERT row")?;
        let mut values = Vec::new();
        let mut count = 0;
        loop {
            if count == MAX_SQL_COLUMNS {
                return Err(self.lexer.error(DumpProblem::Limit("columns")));
            }
            if width == Some(count) {
                return Err(self
                    .lexer
                    .syntax("too many values for the declared columns"));
            }
            let column = layout
                .and_then(|layout| layout.get(count))
                .copied()
                .unwrap_or(Column::Ignored);
            let value = self.lexer.literal(column != Column::Ignored)?;
            if column != Column::Ignored {
                values.push((column, value));
            }
            count += 1;
            if !self.lexer.accept(b',')? {
                break;
            }
        }
        self.lexer
            .expect(b')', "expected the end of the INSERT row")?;
        if width.is_some_and(|width| width != count) {
            return Err(self.lexer.syntax("too few values for the declared columns"));
        }
        Ok((count, values))
    }
}
