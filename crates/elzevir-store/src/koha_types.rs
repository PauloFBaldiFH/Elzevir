// SPDX-License-Identifier: GPL-3.0-or-later
//! The library's translation of its Koha item types, stored at the MARC edge
//! (docs/specs/catalogo/bibframe-completo.md, item 23e). A class is stored as the
//! property `class` with an empty scheme.

use elzevir_app::catalog::{CatalogOutput, CatalogReadError};
use elzevir_app::commands::WriteRefusal;
use elzevir_app::description::concept;
use elzevir_app::koha_types::{TypeTranslationSource, TypeTranslations, coded};
use elzevir_domain::statement::Description;
use rusqlite::{Connection, params};

use crate::{CommandFailed, ElzevirStore, StoreError, WriteTx};

const WORK: i64 = 1;
const INSTANCE: i64 = 2;

/// `catalog.set_type_translations`: the stored translation becomes `translations`.
pub(crate) fn set(
    tx: &WriteTx<'_>,
    translations: &TypeTranslations,
) -> Result<CatalogOutput, CommandFailed<WriteRefusal>> {
    let connection = tx.connection();
    connection
        .execute("DELETE FROM koha_type_statements", [])
        .map_err(StoreError::from)?;
    for (code, translation) in &translations.0 {
        store(connection, code, WORK, &translation.work)?;
        store(connection, code, INSTANCE, &translation.instance)?;
    }
    Ok(CatalogOutput::TypeTranslationsSet(translations.0.len()))
}

fn store(
    connection: &Connection,
    code: &str,
    entity: i64,
    description: &Description,
) -> Result<(), StoreError> {
    let classes = description
        .classes
        .iter()
        .map(|class| ("class", "", class.as_str()));
    let statements = description.statements.iter().filter_map(|statement| {
        coded(statement).map(|(scheme, value)| (statement.property.as_str(), scheme, value))
    });
    for (position, (property, scheme, value)) in (0_i64..).zip(classes.chain(statements)) {
        connection.execute(
            "INSERT INTO koha_type_statements (code, entity, position, property, scheme, value)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![code, entity, position, property, scheme, value],
        )?;
    }
    Ok(())
}

impl TypeTranslationSource for ElzevirStore {
    fn type_translations(&self) -> Result<TypeTranslations, CatalogReadError> {
        self.readers()
            .read(|c| {
                let mut statement = c.prepare(
                    "SELECT code, entity, property, scheme, value FROM koha_type_statements
                     ORDER BY code, entity, position",
                )?;
                let rows = statement
                    .query_map([], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                        ))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                let mut translations = TypeTranslations::default();
                for (code, entity, property, scheme, value) in rows {
                    let translation = translations.0.entry(code).or_default();
                    let description = if entity == WORK {
                        &mut translation.work
                    } else {
                        &mut translation.instance
                    };
                    if property == "class" {
                        description.classes.push(value);
                    } else {
                        description
                            .statements
                            .push(concept(&property, &scheme, &value));
                    }
                }
                Ok(translations)
            })
            .map_err(|e: rusqlite::Error| CatalogReadError(e.to_string()))
    }
}
