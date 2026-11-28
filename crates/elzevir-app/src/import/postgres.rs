// SPDX-License-Identifier: GPL-3.0-or-later
//! Read-only structural inventory of the text COPY subset emitted by `pg_dump`.
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, Read};

use serde::Serialize;
use thiserror::Error;

use super::preview::MAX_IMPORT_FILE_BYTES;

const MAX_LINE: usize = 4 * 1024 * 1024;
const MAX_VALUE: usize = 1024 * 1024;
const MAX_HEADER: usize = 64 * 1024;
const MAX_TABLES: usize = 1024;
const MAX_COLUMNS: usize = 256;

/// Counts for a source table, without any of its values.
#[derive(Debug, Default, Serialize)]
pub struct TableInventory {
    /// Column order in the COPY blocks. Empty for a declaration alone.
    pub columns: Vec<String>,
    /// Complete COPY rows.
    pub rows: u64,
    /// Whole fields encoded as `PostgreSQL`'s null marker.
    pub nulls: u64,
    /// Zero-length fields distinct from nulls.
    pub empty: u64,
}

/// A located diagnostic that can safely be shown to the operator.
#[derive(Debug, Error)]
#[error("PostgreSQL inventory: line {line}: {reason}")]
pub struct InventoryError {
    line: u64,
    reason: &'static str,
}

/// Reads a single source to completion, retaining only identifiers and totals.
/// A truncated upload or failed read discards the whole report.
///
/// # Errors
/// Rejects incomplete blocks, unsupported SQL, inconsistent layouts and limits.
pub fn inventory(input: impl BufRead) -> Result<BTreeMap<String, TableInventory>, InventoryError> {
    read_inventory(input, MAX_IMPORT_FILE_BYTES)
}

fn read_inventory(
    mut input: impl BufRead,
    byte_limit: usize,
) -> Result<BTreeMap<String, TableInventory>, InventoryError> {
    let mut tables = BTreeMap::new();
    let mut active: Option<(String, TableInventory)> = None;
    let mut statement = String::new();
    let mut line = Vec::new();
    let mut bytes = 0;
    let mut number = 0;
    loop {
        number += 1;
        let error = |reason| InventoryError {
            line: number,
            reason,
        };
        line.clear();
        let limit = MAX_LINE.min(byte_limit - bytes) + 1;
        let read = (&mut input)
            .take(limit as u64)
            .read_until(b'\n', &mut line)
            .map_err(|_| error("input interrupted"))?;
        if read == 0 {
            if active.is_some() || !statement.is_empty() {
                return Err(error("unexpected end of input"));
            }
            if tables.is_empty() {
                return Err(error("no supported table declaration or COPY block"));
            }
            return Ok(tables);
        }
        bytes += read;
        if read >= limit {
            return Err(error("source or line limit exceeded"));
        }
        let content = line.strip_suffix(b"\n").unwrap_or(&line);
        let content = content.strip_suffix(b"\r").unwrap_or(content);
        if let Some((name, mut table)) = active.take() {
            if content == br"\." {
                tables.insert(name, table);
            } else {
                count_row(&mut table, content).map_err(error)?;
                active = Some((name, table));
            }
            continue;
        }
        let text = std::str::from_utf8(content)
            .map_err(|_| error("invalid SQL header encoding"))?
            .trim();
        if text.is_empty() || text.starts_with("--") {
            continue;
        }
        if text.starts_with('\\') || statement.len() + text.len() + 1 > MAX_HEADER {
            return Err(error("unsupported directive or statement limit exceeded"));
        }
        if !complete_sql_line(text) {
            return Err(error("unsupported quoted or procedural SQL"));
        }
        if !statement.is_empty() {
            statement.push(' ');
        }
        statement.push_str(text);
        if !text.ends_with(';') {
            continue;
        }
        let sql = statement.strip_suffix(';').unwrap_or(&statement).trim_end();
        let header = parse_statement(sql).map_err(error)?;
        if let Some((name, columns)) = header {
            if tables.len() == MAX_TABLES && !tables.contains_key(&name) {
                return Err(error("table limit exceeded"));
            }

            if let Some(columns) = columns {
                let mut table = tables.remove(&name).unwrap_or_default();
                if !table.columns.is_empty() && table.columns != columns {
                    return Err(error("COPY column order changed"));
                }
                table.columns = columns;
                active = Some((name, table));
            } else if tables.insert(name, TableInventory::default()).is_some() {
                return Err(error("repeated table declaration"));
            }
        }
        statement.clear();
    }
}

fn count_row(table: &mut TableInventory, data: &[u8]) -> Result<(), &'static str> {
    let mut fields = 0;
    for value in data.split(|byte| *byte == b'\t') {
        fields += 1;
        if fields > table.columns.len() || value.len() > MAX_VALUE {
            return Err("extra field or value limit exceeded");
        }
        if value == br"\N" {
            table.nulls += 1;
        } else if value.is_empty() {
            table.empty += 1;
        } else {
            // Backslashes quote the next character in text COPY. A literal
            // backslash at the end must itself be escaped.
            let mut escaped = false;
            for &byte in value {
                if matches!(byte, 0 | b'\r') {
                    return Err("unsupported control byte");
                }
                escaped = !escaped && byte == b'\\';
            }
            if escaped {
                return Err("incomplete escape or unsupported physical continuation");
            }
        }
    }
    if fields != table.columns.len() {
        return Err("missing COPY field");
    }
    table.rows += 1;
    Ok(())
}

// COPY payloads bypass this check. Outside a block we accept single-line
// SQL literals and identifiers only, so text in a quoted body cannot be
// mistaken for the next COPY header.
fn complete_sql_line(text: &str) -> bool {
    let mut quote = None;
    for byte in text.bytes() {
        if byte == b'\\' || (quote.is_none() && byte == b'$') {
            return false;
        }
        if quote == Some(byte) {
            quote = None;
        } else if quote.is_none() && matches!(byte, b'\'' | b'"') {
            quote = Some(byte);
        }
    }
    quote.is_none()
}

fn identifier(text: &str) -> Result<String, &'static str> {
    let text = text.trim();
    let quoted = text.strip_prefix('"').and_then(|s| s.strip_suffix('"'));
    let name = quoted.unwrap_or(text);
    let mut chars = name.bytes();
    if name.len() > 63
        || !chars
            .next()
            .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        || !chars.all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return Err("unsupported SQL identifier");
    }
    Ok(if quoted.is_some() {
        name.to_owned()
    } else {
        name.to_ascii_lowercase()
    })
}

fn table_name(text: &str) -> Result<String, &'static str> {
    if let Some((schema, table)) = text.split_once('.') {
        Ok(format!("{}.{}", identifier(schema)?, identifier(table)?))
    } else {
        identifier(text)
    }
}

fn keyword<'a>(text: &'a str, expected: &str) -> Option<&'a str> {
    let (word, rest) = text.trim_start().split_once(char::is_whitespace)?;
    word.eq_ignore_ascii_case(expected)
        .then(|| rest.trim_start())
}

// Only these pg_dump metadata families may accompany the inventory. They
// are discarded. Embedded statements and procedural bodies are unsupported.
fn metadata(sql: &str) -> bool {
    let lower = sql.to_ascii_lowercase();
    if let Some(setting) = keyword(&lower, "set") {
        return setting.split_once('=').is_some_and(|(name, value)| {
            if name.trim() == "client_encoding" {
                return matches!(value.trim(), "'utf8'" | "'utf-8'");
            }
            matches!(
                name.trim(),
                "statement_timeout"
                    | "lock_timeout"
                    | "idle_in_transaction_session_timeout"
                    | "standard_conforming_strings"
                    | "check_function_bodies"
                    | "xmloption"
                    | "client_min_messages"
                    | "row_security"
                    | "default_tablespace"
                    | "default_with_oids"
            )
        });
    }
    if let Some(rest) = lower.strip_prefix("alter table ") {
        let rest = rest.strip_prefix("only ").unwrap_or(rest);
        return rest
            .split_once(char::is_whitespace)
            .is_some_and(|(name, action)| {
                table_name(name).is_ok()
                    && ["owner to ", "add constraint ", "alter column "]
                        .iter()
                        .any(|prefix| action.trim_start().starts_with(prefix))
            });
    }
    [
        "create sequence ",
        "alter sequence ",
        "create extension ",
        "comment on extension ",
        "select pg_catalog.setval(",
    ]
    .iter()
    .any(|start| lower.starts_with(start))
        || lower == "select pg_catalog.set_config('search_path', '', false)"
}

// The opening parenthesis was consumed with the table name. A trailing
// AS query or INHERITS clause can give the table rows from elsewhere.
fn single_definition(body: &str) -> bool {
    let mut depth = 1usize;
    let mut quote = None;
    for (index, byte) in body.bytes().enumerate() {
        if quote == Some(byte) {
            quote = None;
        } else if quote.is_none() {
            match byte {
                b'\'' | b'"' => quote = Some(byte),
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return index + 1 == body.len();
                    }
                }
                _ => {}
            }
        }
    }
    false
}

type Header = (String, Option<Vec<String>>);

fn parse_statement(sql: &str) -> Result<Option<Header>, &'static str> {
    if sql.contains([';', '\0']) || sql.contains("/*") || sql.contains("--") {
        return Err("unsupported compound statement");
    }
    if let Some(header) = keyword(sql, "copy") {
        let (name, rest) = header.split_once('(').ok_or("expected COPY column list")?;
        let (fields, source) = rest.split_once(')').ok_or("incomplete COPY header")?;
        if !keyword(source, "from").is_some_and(|s| s.trim().eq_ignore_ascii_case("stdin")) {
            return Err("only text COPY FROM stdin is supported");
        }
        let mut columns = Vec::new();
        let mut distinct = BTreeSet::new();
        for field in fields.split(',') {
            let id = identifier(field)?;
            if columns.len() == MAX_COLUMNS || !distinct.insert(id.clone()) {
                return Err("duplicate column or column limit exceeded");
            }
            columns.push(id);
        }
        return Ok(Some((table_name(name)?, Some(columns))));
    }
    if let Some(header) = keyword(sql, "create").and_then(|rest| keyword(rest, "table")) {
        let (name, definition) = header.split_once('(').ok_or("expected table declaration")?;
        if !single_definition(definition) {
            return Err("unsupported table declaration");
        }
        return Ok(Some((table_name(name)?, None)));
    }
    if metadata(sql) {
        Ok(None)
    } else {
        Err("unsupported SQL statement")
    }
}

#[cfg(test)]
mod tests {
    use super::read_inventory;

    #[test]
    fn byte_budget_includes_metadata_and_all_copy_blocks() {
        let source = b"-- header\nCOPY t (a) FROM stdin;\n1\n\\.\nCOPY q (b) FROM stdin;\n2\n\\.\n";
        assert!(read_inventory(&source[..], source.len()).is_ok());
        assert!(read_inventory(&source[..], source.len() - 1).is_err());
    }
}
