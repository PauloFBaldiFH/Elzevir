// SPDX-License-Identifier: GPL-3.0-or-later
//! `elzevir switch-from-koha`: preferences, patrons, catalog, circulation and the search
//! index from one whole Koha database dump, in one order
//! (docs/specs/migracao/troca-do-koha.md).

use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use clap::Args;
use elzevir_app::catalog::{CatalogCommand, CatalogReader};
use elzevir_app::codes::{CodeTable, NamedCode};
use elzevir_app::commands::WriteCommand;
use elzevir_app::import::koha_sql::{DatabaseCatalog, read_database_catalog};
use elzevir_app::import::{ImportOptions, ImportReport, Parallelism, import_koha_database};
use elzevir_app::koha_migration::{KohaPlan, plan};
use elzevir_app::koha_prefs::{PrefsConfig, prefs_config, read_preferences};
use elzevir_app::koha_types::TypeTranslationSource;
use elzevir_store::ElzevirStore;

use crate::koha_migration_cli::{
    accounts_report, circulation_report, history_report, migrate_circulation, migrate_patrons,
    open, report as patrons_report,
};
use crate::open_store;

/// The origin of the records brought by this command (item 8).
const ORIGIN: &str = "koha-dump";
/// The library's configuration file.
const CONFIG: &str = "elzevir.toml";
/// Where the preferences go when the library already has a configuration (item 5).
const REVIEW_CONFIG: &str = "elzevir.koha.toml";

/// `elzevir switch-from-koha`.
#[derive(Args)]
pub(crate) struct SwitchFromKohaArgs {
    /// Institution data directory, made by `elzevir init` and still without a catalog.
    #[arg(long)]
    data_dir: PathBuf,
    /// The dump `koha-dump` wrote (`.sql` or `.sql.gz`).
    dump: PathBuf,
}

/// Everything read from the dump before the first write (item 4).
struct Read {
    config: PrefsConfig,
    config_path: PathBuf,
    catalog: DatabaseCatalog,
    plan: KohaPlan,
}

/// Reads the dump three times, then writes each step in order and prints the report
/// (items 2 to 12).
pub(crate) fn run(args: &SwitchFromKohaArgs) -> Result<(), String> {
    // Locked first: with a server running on the directory, nothing is read (item 2).
    let data = open_store(&args.data_dir)?;
    let store = &data.store;
    if store
        .holds_catalog_or_koha_patrons()
        .map_err(|e| e.to_string())?
    {
        return Err(format!(
            "{} already has a catalog or patrons from Koha: the switch starts from a new data \
             directory, so that nothing from Koha mixes with an existing catalog",
            args.data_dir.display()
        ));
    }
    let mut report = Vec::new();
    let started = Instant::now();
    let read = read_all(args)?;
    report.push(step("reading the dump", started.elapsed()));

    let started = Instant::now();
    write_config(&read.config_path, &read.config.toml)?;
    report.push(step("preferences", started.elapsed()));
    report.push(format!(
        "written to {}\n",
        read.config_path
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
    ));
    report.push(crate::koha_prefs_cli::report(&read.config.report));

    let started = Instant::now();
    let patrons = migrate_patrons(store, &read.plan).map_err(|e| format!("patrons: {e}"))?;
    report.push(step("branches, categories and patrons", started.elapsed()));
    report.push(patrons_report(&read.plan, &patrons));

    let started = Instant::now();
    let item_types =
        create_item_types(store, &read.catalog).map_err(|e| format!("item types: {e}"))?;
    report.push(step("item types", started.elapsed()));
    report.push(format!(
        "item types: {item_types} created with Koha's names\n"
    ));

    let started = Instant::now();
    let biblionumbers = read.catalog.biblionumbers();
    let imported = import_catalog(store, read.catalog).map_err(|e| format!("catalog: {e}"))?;
    report.push(step("catalog", started.elapsed()));
    report.push(catalog_report(&imported, &biblionumbers));

    let started = Instant::now();
    let (circulation, accounts, history) =
        migrate_circulation(store, &read.plan).map_err(|e| format!("circulation: {e}"))?;
    report.push(step("circulation", started.elapsed()));
    report.push(circulation_report(&read.plan, &circulation));
    report.push(accounts_report(&read.plan, &accounts));
    report.push(history_report(&read.plan, &history));

    let started = Instant::now();
    let indexed = build_index(store, &args.data_dir).map_err(|e| format!("index: {e}"))?;
    report.push(step("search index", started.elapsed()));
    report.push(format!("search index: {indexed} documents\n"));
    print!("{}", report.concat());
    Ok(())
}

fn step(name: &str, took: Duration) -> String {
    format!("== {name} ({:.1} s)\n", took.as_secs_f64())
}

/// The three readings of the dump, each keeping only its tables (item 4).
fn read_all(args: &SwitchFromKohaArgs) -> Result<Read, String> {
    let dumped =
        |e: elzevir_app::import::koha_sql::DumpError| format!("{}: {e}", args.dump.display());
    let preferences = read_preferences(open(&args.dump)?).map_err(dumped)?;
    let config = prefs_config(&preferences);
    // The importer only writes what the server reads; a failure here is a bug.
    crate::config::check(&config.toml)
        .map_err(|e| format!("the configuration written would not be read: {e}"))?;
    let library = args.data_dir.join(CONFIG);
    let review = args.data_dir.join(REVIEW_CONFIG);
    let (config_path, zone) = if exists(&library)? {
        if exists(&review)? {
            return Err(format!(
                "{} already exists: move it away before switching",
                review.display()
            ));
        }
        (review, crate::config::load(&args.data_dir)?.time_zone)
    } else {
        let loaded = crate::config::from_text(&args.data_dir, &library, Some(&config.toml))?;
        (library, loaded.time_zone)
    };
    let catalog = read_database_catalog(open(&args.dump)?).map_err(dumped)?;
    let today = jiff::Timestamp::now().to_zoned(zone.clone()).date();
    let plan = plan(open(&args.dump)?, today, &zone).map_err(dumped)?;
    Ok(Read {
        config,
        config_path,
        catalog,
        plan,
    })
}

fn exists(path: &Path) -> Result<bool, String> {
    path.try_exists()
        .map_err(|e| format!("cannot look for {}: {e}", path.display()))
}

/// Writes the preferences without ever replacing a file (item 5).
fn write_config(path: &Path, text: &str) -> Result<(), String> {
    let failed = |e: std::io::Error| format!("preferences: cannot write {}: {e}", path.display());
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(failed)?;
    file.write_all(text.as_bytes()).map_err(failed)?;
    file.sync_all().map_err(failed)
}

/// Creates the item types the Items use and the MARC edge does not know, named as Koha
/// names them, or by their code without a row in `itemtypes` (item 7).
fn create_item_types(store: &ElzevirStore, catalog: &DatabaseCatalog) -> Result<usize, String> {
    let known = store.codes().map_err(|e| e.to_string())?.edge;
    let names: &BTreeMap<String, String> = &catalog.item_types;
    let new: Vec<NamedCode> = catalog
        .item_type_codes()
        .into_iter()
        .filter(|code| {
            !known
                .iter()
                .any(|entry| entry.table == CodeTable::ItemType && entry.code == *code)
        })
        .map(|code| NamedCode {
            table: CodeTable::ItemType,
            name: names.get(&code).cloned().unwrap_or_else(|| code.clone()),
            code,
        })
        .collect();
    let created = new.len();
    if created > 0 {
        store
            .submit(WriteCommand::Catalog(CatalogCommand::NameCodes(new)))
            .map_err(|e| e.to_string())?
            .wait()
            .map_err(|e| e.to_string())?;
    }
    Ok(created)
}

/// The catalog through the path of the Koha SQL import (item 8).
fn import_catalog(store: &ElzevirStore, catalog: DatabaseCatalog) -> Result<ImportReport, String> {
    let cores = std::thread::available_parallelism().unwrap_or(NonZeroUsize::MIN);
    let switch = std::env::var("ELZEVIR_SERIAL").ok();
    let options = ImportOptions {
        parallelism: Parallelism::from_serial_switch(switch.as_deref(), "import", cores),
        origin: Some(ORIGIN.to_owned()),
        translations: store.type_translations().map_err(|e| e.to_string())?,
        ..ImportOptions::default()
    };
    import_koha_database(catalog, store, &options).map_err(|e| e.to_string())
}

/// The catalog counts, naming refused records and Items by Koha's `biblionumber` (item 11).
fn catalog_report(report: &ImportReport, biblionumbers: &[Option<String>]) -> String {
    let number = |index: u64| {
        usize::try_from(index)
            .ok()
            .and_then(|index| biblionumbers.get(index))
            .and_then(Option::as_deref)
            .map_or_else(
                || format!("record {}", index.saturating_add(1)),
                |number| format!("biblionumber {number}"),
            )
    };
    let mut lines = vec![format!(
        "records: {} read, {} stored, {} refused",
        report.read,
        report.imported,
        report.failures.len()
    )];
    for failed in &report.failures {
        lines.push(format!("  {}: {}", number(failed.index), failed.failure));
    }
    lines.push(format!(
        "Items: {} stored, {} refused",
        report.items,
        report.item_failures.len()
    ));
    for failed in &report.item_failures {
        lines.push(format!(
            "  {}, Item {}: {}",
            number(failed.index),
            failed.position.saturating_add(1),
            failed.refusal
        ));
    }
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// Indexes the whole catalog, so that the first `serve` does not start by reindexing
/// (item 10).
fn build_index(store: &ElzevirStore, data_dir: &Path) -> Result<u64, String> {
    let (index, parallelism) = crate::open_index(data_dir)?;
    let options = elzevir_app::search::IndexOptions {
        parallelism,
        batch: crate::INDEX_BATCH,
    };
    elzevir_app::search::index_pending(store, store, &index, &options)
        .map(|report| report.indexed)
        .map_err(|e| e.to_string())
}
