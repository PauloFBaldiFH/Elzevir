// SPDX-License-Identifier: GPL-3.0-or-later
//! Every Koha system preference has a destination in Elzevir (task 0.11).
//!
//! Compares the inventory extracted from Koha (`data/koha-<series>/sysprefs.json`) with the
//! triage (`tools/sysprefs-triage.json`). A Koha upgrade that adds a preference fails CI
//! until someone decides where it goes.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use serde_json::Value;

pub(crate) const TRIAGE: &str = "tools/sysprefs-triage.json";
/// The page for Koha administrators, written by `cargo xtask codegen`.
pub(crate) const PAGE: &str = "docs/specs/migracao/sysprefs-do-koha.md";
const DESTINATIONS: [&str; 7] = [
    "config",
    "regra",
    "fixo",
    "interface",
    "plataforma",
    "fora-v1",
    "revisar",
];

pub(crate) fn run(root: &Path) -> Result<(), String> {
    let lock = read(&root.join("koha-lock.json"))?;
    let series = lock
        .get("series")
        .and_then(Value::as_str)
        .ok_or("koha-lock.json has no series")?;
    let inventory = read(&root.join(format!("data/koha-{series}/sysprefs.json")))?;
    let triage = read(&root.join(TRIAGE))?;
    let problems = check(&inventory, &triage)?;
    if problems.is_empty() {
        println!("check-sysprefs: every Koha preference has a destination in {TRIAGE}");
        Ok(())
    } else {
        Err(format!(
            "{TRIAGE} is out of date:\n  {}",
            problems.join("\n  ")
        ))
    }
}

/// Lists preferences without a valid destination and triage entries Koha no longer has.
pub(crate) fn check(inventory: &Value, triage: &Value) -> Result<Vec<String>, String> {
    let known: BTreeSet<&str> = inventory
        .get("sysprefs")
        .and_then(Value::as_array)
        .ok_or("inventory has no sysprefs array")?
        .iter()
        .filter_map(|p| p.get("name").and_then(Value::as_str))
        .collect();
    let entries = triage
        .get("preferencias")
        .and_then(Value::as_object)
        .ok_or("triage has no preferencias object")?;

    let mut problems = Vec::new();
    for name in &known {
        match entries
            .get(*name)
            .and_then(|e| e.get("destino"))
            .and_then(Value::as_str)
        {
            None => problems.push(format!("`{name}` has no destination")),
            Some(d) if !DESTINATIONS.contains(&d) => {
                problems.push(format!("`{name}` has unknown destination `{d}`"));
            }
            Some(_) => {}
        }
    }
    for name in entries.keys() {
        if !known.contains(name.as_str()) {
            problems.push(format!("`{name}` is not a Koha preference any more"));
        }
    }
    Ok(problems)
}

/// One preference as the generated table and page need it.
struct Syspref<'a> {
    name: &'a str,
    default: String,
    tab: &'a str,
    destination: &'a str,
    field: Option<&'a str>,
    secret: bool,
    note: &'a str,
}

/// The inventory joined to the triage, sorted by name, byte by byte.
fn sysprefs<'a>(inventory: &'a Value, triage: &'a Value) -> Result<Vec<Syspref<'a>>, String> {
    let entries = triage
        .get("preferencias")
        .and_then(Value::as_object)
        .ok_or("triage has no preferencias object")?;
    let mut all = Vec::new();
    for syspref in inventory
        .get("sysprefs")
        .and_then(Value::as_array)
        .ok_or("inventory has no sysprefs array")?
    {
        let text = |key: &str| syspref.get(key).and_then(Value::as_str);
        let name = text("name").ok_or("a syspref has no name")?;
        let entry = entries
            .get(name)
            .ok_or_else(|| format!("`{name}` has no destination"))?;
        let triaged = |key: &str| entry.get(key).and_then(Value::as_str);
        all.push(Syspref {
            name,
            default: unescape(text("default").unwrap_or_default()),
            tab: text("tab").unwrap_or_default(),
            destination: triaged("destino").ok_or_else(|| format!("`{name}` has no destino"))?,
            field: triaged("campo"),
            secret: entry
                .get("segredo")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            note: triaged("nota").unwrap_or_default(),
        });
    }
    all.sort_by(|a, b| a.name.cmp(b.name));
    Ok(all)
}

/// The contents of a `MySQL` string literal, as the database keeps them.
///
/// Koha's `sysprefs.sql` writes `\n` for a line break and `''` for a quote.
fn unescape(literal: &str) -> String {
    let mut out = String::with_capacity(literal.len());
    let mut chars = literal.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('0') => out.push('\0'),
                Some('b') => out.push('\u{8}'),
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('Z') => out.push('\u{1a}'),
                // MySQL keeps the backslash of `\%` and `\_`, the LIKE wildcards.
                Some(wildcard @ ('%' | '_')) => {
                    out.push('\\');
                    out.push(wildcard);
                }
                Some(other) => out.push(other),
                None => out.push('\\'),
            },
            '\'' => {
                out.push('\'');
                // `''` inside the literal is one quote.
                let mut rest = chars.clone();
                if rest.next() == Some('\'') {
                    chars = rest;
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// `koha_sysprefs.rs`: every preference with its default and its field.
pub(crate) fn generate(inventory: &Value, triage: &Value, header: &str) -> Result<String, String> {
    let all = sysprefs(inventory, triage)?;
    let mut out = header.to_owned();
    write!(
        out,
        "//! Koha's system preferences, their defaults and the `elzevir.toml` field of each.\n\n\
         use crate::KohaSyspref;\n\n\
         /// Every Koha system preference, sorted by name.\n\
         pub const KOHA_SYSPREFS: &[KohaSyspref] = &[\n"
    )
    .map_err(|e| e.to_string())?;
    for syspref in &all {
        writeln!(
            out,
            "KohaSyspref {{ name: {:?}, default: {:?}, field: {:?}, secret: {} }},",
            syspref.name, syspref.default, syspref.field, syspref.secret
        )
        .map_err(|e| e.to_string())?;
    }
    out.push_str("];\n");
    Ok(out)
}

/// What a destination means, for the page.
fn destination(name: &str) -> &'static str {
    match name {
        "config" => "Campo do `elzevir.toml`",
        "regra" => "Regra de circulação",
        "fixo" => "Comportamento fixo",
        "interface" => "Interface",
        "plataforma" => "Plataforma",
        "fora-v1" => "Fora da v1",
        _ => "A decidir",
    }
}

/// A table cell: no pipe and no line break.
fn cell(text: &str) -> String {
    text.replace('|', "\\|").replace(['\n', '\r'], " ")
}

/// A default value, shortened.
fn default_cell(value: &str) -> String {
    if value.is_empty() {
        return "(vazio)".to_owned();
    }
    let mut short: String = value.chars().take(60).collect();
    if short.len() < value.len() {
        short.push('…');
    }
    let short = cell(&short);
    if short.contains('`') {
        format!("`` {short} ``")
    } else {
        format!("`{short}`")
    }
}

/// The page "Para administradores do Koha: cada syspref ao lado do seu campo".
pub(crate) fn page(inventory: &Value, triage: &Value) -> Result<String, String> {
    let all = sysprefs(inventory, triage)?;
    let mut tabs: BTreeMap<&str, Vec<&Syspref>> = BTreeMap::new();
    for syspref in &all {
        tabs.entry(syspref.tab).or_default().push(syspref);
    }
    let mut out = format!(
        "<!-- Gerado por `cargo xtask codegen` a partir de data/koha-<série>/sysprefs.json e {TRIAGE}. Não edite à mão. -->\n\n\
         # Para administradores do Koha: cada syspref ao lado do seu campo\n\n\
         As {} preferências do sistema do Koha e o lugar de cada uma no Elzevir. As que têm campo no \
         `elzevir.toml` são trazidas por `elzevir import-koha-prefs` \
         ([preferencias-do-koha.md](preferencias-do-koha.md)); as outras, quando o valor da biblioteca \
         difere do padrão do Koha, vão para `[koha_legacy]`, para revisão.\n",
        all.len()
    );
    for (tab, sysprefs) in tabs {
        let tab = if tab.is_empty() { "(ocultas)" } else { tab };
        write!(
            out,
            "\n## {tab}\n\n| Preferência | Padrão do Koha | No Elzevir |\n|---|---|---|\n"
        )
        .map_err(|e| e.to_string())?;
        for syspref in sysprefs {
            let place = match syspref.field {
                Some(field) => format!("**`{field}`**. {}", cell(syspref.note)),
                None => format!(
                    "{}: {}",
                    destination(syspref.destination),
                    cell(syspref.note)
                ),
            };
            let place = if syspref.secret {
                format!("{place} Senha ou chave: não é copiada.")
            } else {
                place
            };
            writeln!(
                out,
                "| `{}` | {} | {place} |",
                syspref.name,
                default_cell(&syspref.default)
            )
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(out)
}

fn read(path: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{} is not JSON: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn sql_escapes_are_decoded() {
        assert_eq!(unescape(r"a\nb\\c\'d''e\%"), "a\nb\\c'd'e\\%");
    }

    #[test]
    fn missing_unknown_and_stale_entries_are_reported() {
        let inventory =
            json(r#"{ "sysprefs": [ { "name": "A" }, { "name": "B" }, { "name": "C" } ] }"#);
        let triage = json(
            r#"{ "preferencias": {
                "A": { "destino": "config" },
                "B": { "destino": "maybe" },
                "Old": { "destino": "fixo" }
            } }"#,
        );
        let problems = check(&inventory, &triage).unwrap();
        assert_eq!(
            problems,
            [
                "`B` has unknown destination `maybe`",
                "`C` has no destination",
                "`Old` is not a Koha preference any more"
            ]
        );
    }
}
