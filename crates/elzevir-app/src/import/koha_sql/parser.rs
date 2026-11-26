// SPDX-License-Identifier: GPL-3.0-or-later
//! Only named catalog columns reach the assembled dump.

use std::collections::{BTreeMap, BTreeSet};
use std::io::BufRead;

use super::lexer::Lexer;
use super::{DumpError, DumpProblem, MAX_SQL_COLUMNS, Target};

type Cells<C> = Vec<(C, Option<Vec<u8>>)>;

pub(super) fn parse<R: BufRead, T: Target>(input: R, target: T) -> Result<T, DumpError> {
    let mut parser = Parser {
        lexer: Lexer::new(input, T::MAX_BYTES),
        schema: None,
        columns: BTreeMap::new(),
        target,
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
    parser
        .target
        .finish(any)
        .map_err(|problem| parser.lexer.error(problem))?;
    Ok(parser.target)
}

struct Parser<R, T: Target> {
    lexer: Lexer<R>,
    schema: Option<String>,
    columns: BTreeMap<T::Table, Vec<String>>,
    target: T,
}

impl<R: BufRead, T: Target> Parser<R, T> {
    fn statement(&mut self) -> Result<(), DumpError> {
        match self.lexer.keyword()?.as_str() {
            "create" => match self.lexer.keyword()?.as_str() {
                "table" => self.create(),
                // `mysqldump --databases` names the database before its tables.
                "database" if T::WHOLE_DATABASE => self.lexer.rest(),
                _ => Err(self.lexer.error(DumpProblem::Unsupported)),
            },
            // MariaDB 11 wraps each table's rows in a transaction with autocommit off, set
            // through session variables (`SET @OLD_AUTOCOMMIT=…`); Koha's tables are UTF-8.
            "use" | "commit" | "set" if T::WHOLE_DATABASE => self.lexer.rest(),
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

    fn table(&mut self, name: String, declared: bool) -> Result<Option<T::Table>, DumpError> {
        let name = if self.lexer.accept(b'.')? {
            if self.schema.as_ref().is_some_and(|schema| *schema != name) {
                return Err(self.lexer.syntax("tables from multiple schemas"));
            }
            self.schema = Some(name);
            self.lexer.name(true)?
        } else {
            name
        };
        self.target
            .table(&name.to_ascii_lowercase(), declared)
            .map_err(|problem| self.lexer.error(problem))
    }

    fn create(&mut self) -> Result<(), DumpError> {
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
        if let Some(table) = table
            && self.columns.insert(table, columns).is_some()
        {
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
        let schema = table.and_then(|table| self.columns.get(&table));
        if let (Some(explicit), Some(schema)) = (&explicit, schema)
            && explicit.iter().any(|column| !schema.contains(column))
        {
            return Err(self
                .lexer
                .syntax("INSERT column absent from the table declaration"));
        }
        let columns = explicit.as_ref().or(schema);
        let layout = match (table, columns) {
            (Some(table), Some(names)) => Some(
                names
                    .iter()
                    .map(|name| self.target.column(table, name))
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        };
        if let Some(table) = table
            && layout
                .as_ref()
                .is_none_or(|layout| !self.target.has_keys(table, layout))
        {
            return Err(self.lexer.error(DumpProblem::MissingColumns));
        }
        self.lexer.word("values")?;
        let mut width = layout.as_ref().map(Vec::len);
        loop {
            self.lexer.head()?;
            let offset = self.lexer.offset;
            let (count, values) = self.tuple(layout.as_deref(), width)?;
            width = Some(count);
            if let Some(table) = table {
                self.target.push(table, offset, values)?;
            }
            if !self.lexer.accept(b',')? {
                return Ok(());
            }
        }
    }

    fn tuple(
        &mut self,
        layout: Option<&[Option<T::Column>]>,
        width: Option<usize>,
    ) -> Result<(usize, Cells<T::Column>), DumpError> {
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
                .flatten();
            let value = self.lexer.literal(column.is_some())?;
            if let Some(column) = column {
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
