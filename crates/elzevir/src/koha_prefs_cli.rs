// SPDX-License-Identifier: GPL-3.0-or-later
//! `elzevir import-koha-prefs`: Koha's system preferences as an `elzevir.toml`
//! (docs/specs/migracao/preferencias-do-koha.md).

use std::path::PathBuf;

use clap::Args;
use elzevir_app::koha_prefs::{PrefsReport, prefs_config, read_preferences};

/// `elzevir import-koha-prefs`.
#[derive(Args)]
pub(crate) struct ImportKohaPrefsArgs {
    /// A Koha database dump (`.sql` or `.sql.gz`) with `systempreferences`.
    dump: PathBuf,
}

/// Writes the configuration on the standard output and the report on the standard error
/// (items 1 to 8).
pub(crate) fn run(args: &ImportKohaPrefsArgs) -> Result<(), String> {
    let input = crate::koha_migration_cli::open(&args.dump)?;
    let preferences =
        read_preferences(input).map_err(|e| format!("{}: {e}", args.dump.display()))?;
    let config = prefs_config(&preferences);
    // The importer only writes what the server reads; a failure here is a bug.
    crate::config::check(&config.toml)
        .map_err(|e| format!("the configuration written would not be read: {e}"))?;
    print!("{}", config.toml);
    eprint!("{}", report(&config.report));
    Ok(())
}

/// The report, by name only: preferences can hold addresses and keys (item 8).
pub(crate) fn report(report: &PrefsReport) -> String {
    let mut lines = vec![format!(
        "system preferences: {} read, {} at Koha's default left out, {} in fields, {} in [koha_legacy] ({} not from Koha 26.05), {} passwords or keys not copied",
        report.read,
        report.at_default,
        report.placed.len(),
        report.legacy.len() + report.unknown.len(),
        report.unknown.len(),
        report.secrets.len()
    )];
    for (name, field) in &report.placed {
        lines.push(format!("  {name} -> {field}"));
    }
    if !report.unknown.is_empty() {
        lines.push(format!(
            "not from Koha 26.05: {}",
            report.unknown.join(", ")
        ));
    }
    if !report.secrets.is_empty() {
        lines.push(format!("not copied: {}", report.secrets.join(", ")));
    }
    if report.oai_to_review {
        lines.push(
            "OAI-PMH was on in Koha: [oai] is written turned off; check it and set enabled = true"
                .to_owned(),
        );
    }
    let mut text = lines.join("\n");
    text.push('\n');
    text
}
