// SPDX-License-Identifier: GPL-3.0-or-later
//! `elzevir.toml`, the typed configuration of a data directory (doc 04, decision 8, and
//! docs/specs/catalogo/numero-de-chamada.md). Every key is optional; an unknown key is an
//! error, so a typo is not silently ignored.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use elzevir_domain::call_number::{AuthorTable, MAX_TABLE_LINES, TableReport};
use serde::Deserialize;

/// Most bytes of `elzevir.toml`.
const MAX_CONFIG: u64 = 1 << 20;

/// Most bytes of the author table: the printed tables are a few hundred kilobytes.
const MAX_TABLE: u64 = 8 << 20;

/// What `elzevir.toml` says.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Config {
    /// How call numbers are suggested.
    #[serde(default)]
    call_number: CallNumber,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct CallNumber {
    /// The library's author table (PHA, Cutter-Sanborn), relative to the data directory.
    author_table: Option<PathBuf>,
}

/// The configuration as the server uses it.
#[derive(Debug)]
pub(crate) struct Loaded {
    /// The author table, when one is configured.
    pub(crate) author_table: Option<Arc<AuthorTable>>,
    /// What reading the table found, to show at start.
    pub(crate) table_report: Option<(PathBuf, TableReport)>,
}

/// Reads `data_dir/elzevir.toml` and what it points to; no file is no configuration.
pub(crate) fn load(data_dir: &Path) -> Result<Loaded, String> {
    let path = data_dir.join("elzevir.toml");
    let config: Config = match read_limited(&path, MAX_CONFIG)? {
        None => Config::default(),
        Some(text) => toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?,
    };
    let Some(table) = config.call_number.author_table else {
        return Ok(Loaded {
            author_table: None,
            table_report: None,
        });
    };
    let table_path = data_dir.join(table);
    let text = read_limited(&table_path, MAX_TABLE)?
        .ok_or_else(|| format!("{}: the author table does not exist", table_path.display()))?;
    let (table, report) = AuthorTable::parse(&text).map_err(|e| {
        format!(
            "{}: {e} (at most {MAX_TABLE_LINES} lines)",
            table_path.display()
        )
    })?;
    Ok(Loaded {
        author_table: Some(Arc::new(table)),
        table_report: Some((table_path, report)),
    })
}

/// The file as text, `None` when it does not exist.
fn read_limited(path: &Path, limit: u64) -> Result<Option<String>, String> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let mut text = String::new();
    file.take(limit + 1)
        .read_to_string(&mut text)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if u64::try_from(text.len()).unwrap_or(u64::MAX) > limit {
        return Err(format!("{}: larger than {limit} bytes", path.display()));
    }
    Ok(Some(text))
}

/// One line on what the table gave, and the lines to check against the book.
pub(crate) fn summary(path: &Path, report: &TableReport) -> String {
    let lines = |list: &[usize]| {
        list.iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut out = format!(
        "elzevir: author table {}: {} entries from {} lines",
        path.display(),
        report.entries,
        report.lines
    );
    for (what, list) in [
        ("not understood", &report.not_understood),
        ("numbers out of order", &report.out_of_order),
        ("repeated", &report.repeated),
    ] {
        if !list.is_empty() {
            out.push_str("; ");
            out.push_str(what);
            out.push_str(" at lines ");
            out.push_str(&lines(list));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_read_relative_to_the_data_directory() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(dir.path()).unwrap().author_table.is_none());
        std::fs::write(
            dir.path().join("elzevir.toml"),
            "[call_number]\nauthor_table = \"autores.txt\"\n",
        )
        .unwrap();
        assert!(load(dir.path()).unwrap_err().contains("does not exist"));
        std::fs::write(dir.path().join("autores.txt"), "Assi 848\nAb 12\nAc 11\n").unwrap();
        let loaded = load(dir.path()).unwrap();
        assert!(loaded.author_table.is_some());
        let (path, report) = loaded.table_report.unwrap();
        assert!(
            summary(&path, &report)
                .ends_with("3 entries from 3 lines; numbers out of order at lines 3")
        );
        std::fs::write(
            dir.path().join("elzevir.toml"),
            "[call_number]\nauthor_tabel = \"x\"\n",
        )
        .unwrap();
        assert!(load(dir.path()).unwrap_err().contains("unknown field"));
    }
}
