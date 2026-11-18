// SPDX-License-Identifier: GPL-3.0-or-later
//! The `import` and `export` roles (docs/specs/catalogo/linha-de-comando.md).

use elzevir_app::codes::{CodeTable, NamedCode};
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use clap::{Args, ValueEnum};
use elzevir_app::catalog::{CatalogCommand, CatalogOutput, CatalogReader};
use elzevir_app::commands::{WriteCommand, WriteOutput};
use elzevir_app::export::{ExportOptions, export_marc21, item_codes};
use elzevir_app::import::{ImportOptions, ImportReport, Parallelism, import_marc21};
use elzevir_app::import_detection::{Evidence, ImportFormat, MAX_DETECTION_BYTES, detect_format};
use elzevir_app::koha_types::{self, TypeTranslationSource};
use elzevir_app::matching::{self, Matching, OnMatch, OnNoMatch};
use elzevir_app::rdf::{IriBase, RdfFormat, export_rdf_catalog};
use elzevir_domain::catalog::MatchRule;
use elzevir_marc::Limits;
use elzevir_store::ElzevirStore;

use crate::open_store;

/// `elzevir import`.
#[derive(Args)]
pub(crate) struct ImportArgs {
    /// Institution data directory (created if missing).
    #[arg(long)]
    data_dir: PathBuf,
    /// Rule that finds a stored record equal to an imported one; without it every record
    /// is stored.
    #[arg(long = "match", value_enum)]
    rule: Option<Rule>,
    /// What to do when a record matches a stored one.
    #[arg(long, value_enum, default_value = "ignore", requires = "rule")]
    on_match: MatchAction,
    /// What to do when a record matches nothing.
    #[arg(long, value_enum, default_value = "create-new", requires = "rule")]
    on_no_match: NoMatchAction,
    /// What to do with the Items of each record.
    #[arg(long, value_enum, default_value = "always-add", requires = "rule")]
    items: ItemAction,
    /// Create the branches and item types the file refers to that do not exist yet,
    /// with their codes as names.
    #[arg(long)]
    create_codes: bool,
    /// Inspect a MARC file using a read-only view of the catalog.
    #[arg(long)]
    preview: bool,
    /// The MARC21 file (ISO 2709, or XML and mnemonic text in preview).
    file: PathBuf,
}

/// `elzevir export`.
#[derive(Args)]
pub(crate) struct ExportArgs {
    /// Institution data directory.
    #[arg(long)]
    data_dir: PathBuf,
    /// Where to write the file; standard output when absent.
    #[arg(long)]
    output: Option<PathBuf>,
    /// What to write: MARC21 (ISO 2709), or BIBFRAME as Turtle or RDF/XML.
    #[arg(long, value_enum, default_value_t = ExportFormat::Marc21)]
    format: ExportFormat,
    /// Where the IRIs of the BIBFRAME export start, such as
    /// `https://biblioteca.example/bibframe`; needed for `turtle` and `rdfxml`.
    #[arg(long)]
    iri_base: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ExportFormat {
    Marc21,
    Turtle,
    Rdfxml,
}

#[derive(Clone, Copy, ValueEnum)]
enum Rule {
    Isbn,
    Issn,
    KohaBiblionumber,
}

#[derive(Clone, Copy, ValueEnum)]
enum MatchAction {
    Ignore,
    CreateNew,
    Replace,
}

#[derive(Clone, Copy, ValueEnum)]
enum NoMatchAction {
    CreateNew,
    Ignore,
}

#[derive(Clone, Copy, ValueEnum)]
enum ItemAction {
    AlwaysAdd,
    AddOnlyForMatches,
    AddOnlyForNew,
    Replace,
    Ignore,
}

impl ImportArgs {
    fn matching(&self) -> Option<Matching> {
        let rule = match self.rule? {
            Rule::Isbn => MatchRule::Isbn,
            Rule::Issn => MatchRule::Issn,
            Rule::KohaBiblionumber => MatchRule::KohaBiblionumber,
        };
        Some(Matching {
            rule,
            on_match: match self.on_match {
                MatchAction::Ignore => OnMatch::Ignore,
                MatchAction::CreateNew => OnMatch::CreateNew,
                MatchAction::Replace => OnMatch::Replace,
            },
            on_no_match: match self.on_no_match {
                NoMatchAction::CreateNew => OnNoMatch::CreateNew,
                NoMatchAction::Ignore => OnNoMatch::Ignore,
            },
            items: match self.items {
                ItemAction::AlwaysAdd => matching::ItemAction::AlwaysAdd,
                ItemAction::AddOnlyForMatches => matching::ItemAction::AddOnlyForMatches,
                ItemAction::AddOnlyForNew => matching::ItemAction::AddOnlyForNew,
                ItemAction::Replace => matching::ItemAction::Replace,
                ItemAction::Ignore => matching::ItemAction::Ignore,
            },
        })
    }
}

fn open_input(path: &Path) -> Result<BufReader<File>, String> {
    File::open(path)
        .map(|file| BufReader::with_capacity(MAX_DETECTION_BYTES, file))
        .map_err(|e| format!("cannot open {}: {e}", path.display()))
}

pub(crate) fn import(args: &ImportArgs) -> Result<(), String> {
    let mut input = open_input(&args.file)?;
    if args.preview {
        return preview(args, input);
    }
    require_iso2709(&mut input)?;
    let data = open_store(&args.data_dir)?;
    if args.create_codes {
        create_codes(&data.store, &args.file)?;
    }
    let cores = std::thread::available_parallelism().unwrap_or(NonZeroUsize::MIN);
    let switch = std::env::var("ELZEVIR_SERIAL").ok();
    let options = ImportOptions {
        parallelism: Parallelism::from_serial_switch(switch.as_deref(), "import", cores),
        matching: args.matching(),
        translations: data.store.type_translations().map_err(|e| e.to_string())?,
        ..ImportOptions::default()
    };
    let report = import_marc21(input, &data.store, &options).map_err(|e| e.to_string())?;
    print_import(&report);
    Ok(())
}

fn preview(args: &ImportArgs, input: BufReader<File>) -> Result<(), String> {
    if args.create_codes {
        return Err("--create-codes is incompatible with --preview".into());
    }
    if crate::restore_journal::pending(&args.data_dir) {
        return Err("interrupted restore requires backup recover-restore before preview".into());
    }
    let options = ImportOptions {
        matching: args.matching(),
        ..ImportOptions::default()
    };
    let report = elzevir_store::preview_import(&args.data_dir.join("elzevir.db"), input, options)
        .map_err(|error| error.to_string())?;
    print_preview(&report);
    if report.issues.is_empty() {
        Ok(())
    } else {
        Err("preview requires review".into())
    }
}

pub(crate) fn print_preview(report: &elzevir_app::import::preview::PreviewReport) {
    println!(
        "elzevir: {:?} ({:?}) preview: {} records read, {} prepared, {} blocking diagnostics",
        report.detection.format,
        report.detection.evidence,
        report.read,
        report.prepared,
        report.issues.len()
    );
    for sample in &report.samples {
        println!(
            "record {}: Work: {:?} | Instance: {:?} | Items: {}",
            sample.index + 1,
            sample.work,
            sample.instance,
            sample.items
        );
    }
    for (index, instance) in &report.matches {
        println!(
            "record {}: possible match with instance {}",
            index + 1,
            instance.0
        );
    }
    for issue in &report.issues {
        let item = issue
            .item
            .map_or_else(String::new, |position| format!(", item {}", position + 1));
        eprintln!(
            "elzevir: record {} (byte {}){item}: {}",
            issue.index + 1,
            issue.offset,
            issue.reason
        );
    }
}

fn require_iso2709(input: &mut impl BufRead) -> Result<(), String> {
    let detected = detect_format(
        input
            .fill_buf()
            .map_err(|error| format!("cannot inspect input: {error}"))?,
    );
    let format = match (detected.format, detected.evidence) {
        (ImportFormat::Iso2709, _) => return Ok(()),
        (ImportFormat::KohaFullBackup, _) => {
            return Err(
                "complete Koha backup: use a database restoration workflow, not catalog import"
                    .to_owned(),
            );
        }
        (ImportFormat::MarcXml, _) => "MARCXML",
        (ImportFormat::MarcText, _) => "MarcEdit",
        (ImportFormat::Delimited, _) => "CSV/TSV",
        (ImportFormat::KohaSqlDump, _) => "Koha SQL",
        (ImportFormat::Unknown, Evidence::ZipSignature) => "ZIP",
        (ImportFormat::Unknown, Evidence::GzipSignature) => "gzip",
        (ImportFormat::Unknown, _) => "unknown or ambiguous",
    };
    Err(format!(
        "{format} input is not supported yet; only MARC21 ISO 2709 can be imported"
    ))
}

#[cfg(test)]
mod detection_tests {
    use std::cell::Cell;
    use std::io::{self, Cursor, Read};
    use std::rc::Rc;

    use super::{BufReader, MAX_DETECTION_BYTES, require_iso2709};

    struct Counted<R> {
        input: R,
        bytes: Rc<Cell<usize>>,
    }

    impl<R: Read> Read for Counted<R> {
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            let count = self.input.read(output)?;
            self.bytes.set(self.bytes.get() + count);
            Ok(count)
        }
    }

    #[test]
    fn detection_reads_only_the_prefix_and_leaves_every_byte_for_import() {
        let mut source = b"00080nam a2200037 a 4500".to_vec();
        source.resize(MAX_DETECTION_BYTES * 2, b'x');
        let count = Rc::new(Cell::new(0));
        let counted = Counted {
            input: Cursor::new(&source),
            bytes: Rc::clone(&count),
        };
        let mut input = BufReader::with_capacity(MAX_DETECTION_BYTES, counted);
        require_iso2709(&mut input).unwrap();
        assert_eq!(count.get(), MAX_DETECTION_BYTES);
        let mut imported = Vec::new();
        input.read_to_end(&mut imported).unwrap();
        assert_eq!(imported, source);
        assert_eq!(count.get(), source.len());
    }
}

/// `elzevir koha-types`.
#[derive(Args)]
pub(crate) struct KohaTypesArgs {
    /// Institution data directory.
    #[arg(long)]
    data_dir: PathBuf,
    /// Tab-separated translation of this library's Koha item types into BIBFRAME classes
    /// and statements (docs/specs/catalogo/bibframe-completo.md, item 23e).
    file: PathBuf,
}

/// Replaces the stored translation of Koha item types with the one in the file.
pub(crate) fn koha_types(args: &KohaTypesArgs) -> Result<(), String> {
    let text = std::fs::read_to_string(&args.file)
        .map_err(|e| format!("cannot read {}: {e}", args.file.display()))?;
    let translations =
        koha_types::parse(&text).map_err(|e| format!("{}: {e}", args.file.display()))?;
    let data = open_store(&args.data_dir)?;
    let answer = data
        .store
        .submit(WriteCommand::Catalog(CatalogCommand::SetTypeTranslations(
            translations,
        )))
        .map_err(|e| e.to_string())?
        .wait()
        .map_err(|e| e.to_string())?;
    let WriteOutput::Catalog(CatalogOutput::TypeTranslationsSet(count)) = answer else {
        return Err("unexpected answer from the store".to_owned());
    };
    println!("{count} Koha item types translated");
    Ok(())
}

/// Creates the branches and circulation policies whose Koha codes the file uses and the
/// MARC edge does not know yet, named by their code. Known codes keep their names.
fn create_codes(store: &ElzevirStore, file: &Path) -> Result<(), String> {
    let codes = item_codes(open_input(file)?, Limits::default())
        .map_err(|e| format!("cannot read {}: {e}", file.display()))?;
    let known = store.codes().map_err(|e| e.to_string())?.edge;
    let missing = |table: CodeTable, code: &String| {
        !known
            .iter()
            .any(|entry| entry.table == table && entry.code == *code)
    };
    let new: Vec<NamedCode> = codes
        .branches
        .into_iter()
        .map(|code| (CodeTable::Branch, code))
        .chain(
            codes
                .item_types
                .into_iter()
                .map(|code| (CodeTable::ItemType, code)),
        )
        .filter(|(table, code)| missing(*table, code))
        .map(|(table, code)| NamedCode {
            table,
            name: code.clone(),
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
    println!("elzevir: {created} branch and item type codes created");
    Ok(())
}

fn print_import(report: &ImportReport) {
    for failed in &report.failures {
        eprintln!(
            "elzevir: record {} (byte {}) not imported: {}",
            failed.index.saturating_add(1),
            failed.offset,
            failed.failure
        );
    }
    for failed in &report.item_failures {
        eprintln!(
            "elzevir: record {}, item {} not stored: {}",
            failed.index.saturating_add(1),
            failed.position.saturating_add(1),
            failed.refusal
        );
    }
    println!(
        "elzevir: {} records read, {} stored, {} matched, {} replaced, {} skipped, {} failed",
        report.read,
        report.imported,
        report.matched.len(),
        report.replaced,
        report.skipped,
        report.failures.len()
    );
    println!(
        "elzevir: {} items stored, {} replaced, {} refused",
        report.items,
        report.replaced_items,
        report.item_failures.len()
    );
}

pub(crate) fn export(args: &ExportArgs) -> Result<(), String> {
    let rdf = match args.format {
        ExportFormat::Marc21 => None,
        ExportFormat::Turtle => Some(RdfFormat::Turtle),
        ExportFormat::Rdfxml => Some(RdfFormat::RdfXml),
    };
    let base = match (rdf, &args.iri_base) {
        (Some(_), None) => return Err("--iri-base is needed for a BIBFRAME export".to_owned()),
        (_, base) => base.as_deref().map(IriBase::new),
    };
    let data = open_store(&args.data_dir)?;
    let mut out: BufWriter<Box<dyn Write>> = BufWriter::new(match &args.output {
        Some(path) => Box::new(
            File::create(path).map_err(|e| format!("cannot create {}: {e}", path.display()))?,
        ),
        None => Box::new(std::io::stdout().lock()),
    });
    // Both exports flush what they wrote, so nothing is left in the buffers.
    if let (Some(format), Some(base)) = (rdf, base) {
        let options = ExportOptions::default();
        let written = export_rdf_catalog(&data.store, &mut out, format, &base, options.batch)
            .map_err(|e| e.to_string())?;
        eprintln!("elzevir: {written} records exported");
        return Ok(());
    }
    let report = export_marc21(&data.store, &mut out, &ExportOptions::default())
        .map_err(|e| e.to_string())?;
    for failed in &report.failures {
        eprintln!(
            "elzevir: record {} not exported: {}",
            failed.instance.0, failed.failure
        );
    }
    eprintln!(
        "elzevir: {} records exported, {} failed",
        report.written,
        report.failures.len()
    );
    Ok(())
}
