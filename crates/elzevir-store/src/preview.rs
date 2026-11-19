// SPDX-License-Identifier: GPL-3.0-or-later
//! Catalog observations against a read-only SQLite snapshot.

use std::io::{BufRead, ErrorKind};
use std::num::NonZeroUsize;
use std::path::Path;

use elzevir_app::catalog::{CatalogReadError, CatalogRefusal, CodeLists, ItemData};
use elzevir_app::commands::WriteRefusal;
use elzevir_app::import::ImportOptions;
use elzevir_app::import::edits::Recipe;
use elzevir_app::import::preview::{
    PreviewCatalog, PreviewError, PreviewReport, preview, preview_page, preview_table,
};
use elzevir_app::import::sheet::{PreparedTable, SheetError, TableSettings, prepare};
use elzevir_app::matching::MatchKey;
use elzevir_domain::catalog::InstanceId;
use rusqlite::{Connection, params};

use crate::{CommandFailed, ReadPool, catalog, koha_types, schema};

/// Prepares an input for a catalog file, treating a missing catalog as empty.
///
/// # Errors
/// Reports read, preparation and schema errors.
pub fn preview_import(
    path: &Path,
    input: impl BufRead,
    options: ImportOptions,
) -> Result<PreviewReport, PreviewError> {
    match std::fs::metadata(path) {
        Err(error) if error.kind() == ErrorKind::NotFound => preview(input, None, &options),
        Err(error) => Err(read_error(error).into()),
        Ok(_) => ReadPool::open(path, NonZeroUsize::MIN)
            .map_err(read_error)?
            .preview_import(input, options),
    }
}

/// Prepares a sheet using registry names and collision checks from one read snapshot.
///
/// # Errors
/// Reports read, preparation and schema errors.
pub fn preview_sheet(
    path: &Path,
    input: impl BufRead,
    settings: &TableSettings,
    recipe: &Recipe,
    from: u64,
) -> Result<(PreparedTable, PreviewReport), PreviewError> {
    preview_table_source(path, recipe, from, |codes| {
        prepare(
            input,
            settings,
            codes,
            &recipe.exclude,
            &recipe.accept_missing_barcodes,
        )
    })
}

/// Resolves any validated tabular source against a read-only catalog snapshot.
///
/// # Errors
/// Reports read, preparation and schema errors.
pub fn preview_table_source(
    path: &Path,
    recipe: &Recipe,
    from: u64,
    source: impl FnOnce(&CodeLists) -> Result<PreparedTable, SheetError>,
) -> Result<(PreparedTable, PreviewReport), PreviewError> {
    match std::fs::metadata(path) {
        Err(error) if error.kind() == ErrorKind::NotFound => sheet(source, recipe, from, None),
        Err(error) => Err(read_error(error).into()),
        Ok(_) => ReadPool::open(path, NonZeroUsize::MIN)
            .map_err(read_error)?
            .preview_table_source(recipe, from, source),
    }
}

fn sheet(
    source: impl FnOnce(&CodeLists) -> Result<PreparedTable, SheetError>,
    recipe: &Recipe,
    from: u64,
    connection: Option<&Connection>,
) -> Result<(PreparedTable, PreviewReport), PreviewError> {
    let codes = connection
        .map(crate::items::code_lists)
        .transpose()
        .map_err(read_error)?
        .unwrap_or_default();
    let table = source(&codes).map_err(|error| PreviewError::Edit(error.to_string()))?;
    let snapshot = connection.map(Snapshot);
    let catalog = snapshot
        .as_ref()
        .map(|snapshot| snapshot as &dyn PreviewCatalog);
    let options = ImportOptions {
        matching: recipe.matching,
        ..ImportOptions::default()
    };
    let report = preview_table(&table, catalog, &options, from)?;
    Ok((table, report))
}

impl ReadPool {
    /// Prepares a spreadsheet from the server's existing read pool.
    ///
    /// # Errors
    /// Reports read, preparation and schema errors.
    pub fn preview_sheet(
        &self,
        input: impl BufRead,
        settings: &TableSettings,
        recipe: &Recipe,
        from: u64,
    ) -> Result<(PreparedTable, PreviewReport), PreviewError> {
        self.preview_table_source(recipe, from, |codes| {
            prepare(
                input,
                settings,
                codes,
                &recipe.exclude,
                &recipe.accept_missing_barcodes,
            )
        })
    }

    /// Uses the same registry and matching snapshot for a decoded tabular source.
    ///
    /// # Errors
    /// Reports read, preparation and schema errors.
    pub fn preview_table_source(
        &self,
        recipe: &Recipe,
        from: u64,
        source: impl FnOnce(&CodeLists) -> Result<PreparedTable, SheetError>,
    ) -> Result<(PreparedTable, PreviewReport), PreviewError> {
        self.read(|connection| {
            let transaction = connection.unchecked_transaction().map_err(read_error)?;
            check_schema(&transaction)?;
            sheet(source, recipe, from, Some(&transaction))
        })
    }

    /// Prepares an input using the current read snapshot and its stored translations.
    ///
    /// # Errors
    /// Reports read, preparation and schema errors.
    pub fn preview_import(
        &self,
        input: impl BufRead,
        options: ImportOptions,
    ) -> Result<PreviewReport, PreviewError> {
        self.preview_import_page(input, options, 0)
    }

    /// Uses a single catalog snapshot to validate the whole stream and sample a page.
    ///
    /// # Errors
    /// Reports read, preparation and schema errors.
    pub fn preview_import_page(
        &self,
        input: impl BufRead,
        mut options: ImportOptions,
        from: u64,
    ) -> Result<PreviewReport, PreviewError> {
        self.read(|connection| {
            let transaction = connection.unchecked_transaction().map_err(read_error)?;
            check_schema(&transaction)?;
            options.translations = koha_types::read(&transaction)?;
            preview_page(input, Some(&Snapshot(&transaction)), &options, from)
        })
    }
}

fn check_schema(connection: &Connection) -> Result<(), CatalogReadError> {
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(read_error)?;
    if version == schema::latest_version() {
        Ok(())
    } else {
        Err(CatalogReadError(
            "preview requires the current catalog schema".into(),
        ))
    }
}

struct Snapshot<'a>(&'a Connection);

fn read_error(error: impl std::fmt::Display) -> CatalogReadError {
    CatalogReadError(error.to_string())
}

impl PreviewCatalog for Snapshot<'_> {
    fn candidate(&self, key: &MatchKey) -> Result<Option<InstanceId>, CatalogReadError> {
        let mut query = self
            .0
            .prepare_cached("SELECT min(instance_id) FROM match_keys WHERE rule = ?1 AND key = ?2")
            .map_err(read_error)?;
        let id: Option<i64> = query
            .query_row(params![key.rule.code(), key.key], |row| row.get(0))
            .map_err(read_error)?;
        Ok(id.map(InstanceId))
    }

    fn barcode_exists(&self, barcode: &str) -> Result<bool, CatalogReadError> {
        let mut query = self
            .0
            .prepare_cached("SELECT EXISTS(SELECT 1 FROM items WHERE barcode = ?1)")
            .map_err(read_error)?;
        query
            .query_row([barcode], |row| row.get(0))
            .map_err(read_error)
    }

    fn item_refusal(&self, item: &ItemData) -> Result<Option<CatalogRefusal>, CatalogReadError> {
        match catalog::required_item_codes(self.0, item) {
            Ok(_) => Ok(None),
            Err(CommandFailed::Rejected(WriteRefusal::Catalog(refusal))) => Ok(Some(refusal)),
            Err(_) => Err(CatalogReadError(
                "cannot look up item registry codes".into(),
            )),
        }
    }
}
