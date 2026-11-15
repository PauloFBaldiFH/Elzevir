// SPDX-License-Identifier: GPL-3.0-or-later
//! The translation of a library's Koha item types into BIBFRAME, kept at the MARC edge
//! (docs/specs/catalogo/bibframe-completo.md, items 23e and 32).
//!
//! It is data of one library, never code of Elzevir: a tab-separated file says, for a Koha
//! type code, which classes and which coded statements a record of that type has. The
//! import applies it to what the MARC record does not already say.

use std::collections::BTreeMap;

use elzevir_domain::statement::{Description, EntityKind, Statement, Value, check_description};
use serde::{Deserialize, Serialize};

use crate::description::concept;

/// Largest translation file read.
pub const MAX_BYTES: usize = 1024 * 1024;
/// Most lines of a translation file.
pub const MAX_LINES: usize = 10_000;

/// What one Koha type adds to a record.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeTranslation {
    /// The Work: classes and coded statements.
    pub work: Description,
    /// The Instance: coded statements.
    pub instance: Description,
}

/// Every translated Koha type of a library, by code.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeTranslations(pub BTreeMap<String, TypeTranslation>);

/// Why a line of the file was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TranslationError {
    /// The file is larger than [`MAX_BYTES`] or has more than [`MAX_LINES`] lines.
    #[error("the file is too large")]
    TooLarge,
    /// A line does not have four columns.
    #[error("line {0}: expected type, entity, property and value separated by tabs")]
    Columns(usize),
    /// The entity is neither `work` nor `instance`.
    #[error("line {0}: the entity is `work` or `instance`")]
    Entity(usize),
    /// The value is not `scheme:code`.
    #[error("line {0}: the value is `scheme:code`, such as `contentTypes:txt`")]
    Value(usize),
    /// The ontology or the vocabularies refuse the statement.
    #[error("line {line}: {reason}")]
    Refused {
        /// The line, from 1.
        line: usize,
        /// What the check said.
        reason: String,
    },
}

/// Reads a translation file: one statement per line, `type<TAB>entity<TAB>property<TAB>value`,
/// where `property` is `class` (the value is a BIBFRAME class, for the Work) or a property
/// whose value is `scheme:code`. Blank lines and lines starting with `#` are skipped. Each
/// statement is checked against the ontology and the vocabularies.
///
/// # Errors
///
/// The first line that cannot be read or is refused.
pub fn parse(text: &str) -> Result<TypeTranslations, TranslationError> {
    if text.len() > MAX_BYTES || text.lines().count() > MAX_LINES {
        return Err(TranslationError::TooLarge);
    }
    let mut translations = TypeTranslations::default();
    for (index, line) in text.lines().enumerate() {
        let number = index + 1;
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let columns: Vec<&str> = line.split('\t').map(str::trim).collect();
        let [code, entity, property, value] = columns.as_slice() else {
            return Err(TranslationError::Columns(number));
        };
        if code.is_empty() || property.is_empty() || value.is_empty() {
            return Err(TranslationError::Columns(number));
        }
        let translation = translations.0.entry((*code).to_owned()).or_default();
        let (kind, description) = match *entity {
            "work" => (EntityKind::Work, &mut translation.work),
            "instance" => (EntityKind::Instance, &mut translation.instance),
            _ => return Err(TranslationError::Entity(number)),
        };
        let mut candidate = description.clone();
        if *property == "class" {
            if !candidate.classes.iter().any(|class| class == value) {
                candidate.classes.push((*value).to_owned());
            }
        } else {
            let (scheme, concept_code) = value
                .split_once(':')
                .ok_or(TranslationError::Value(number))?;
            let statement = concept(property, scheme, concept_code);
            if !candidate.statements.contains(&statement) {
                candidate.statements.push(statement);
            }
        }
        check_description(kind, &candidate).map_err(|refusal| TranslationError::Refused {
            line: number,
            reason: refusal.to_string(),
        })?;
        *description = candidate;
    }
    Ok(translations)
}

impl TypeTranslations {
    /// Adds to `work` and `instance` what the type `code` translates to and they do not
    /// already say: a class the Work lacks, a property the record has no value for. What
    /// the MARC record states always wins.
    pub fn apply(&self, code: &str, work: &mut Description, instance: &mut Description) {
        let Some(translation) = self.0.get(code) else {
            return;
        };
        for class in &translation.work.classes {
            if !work.classes.contains(class) {
                work.classes.push(class.clone());
            }
        }
        add_missing(work, &translation.work.statements);
        add_missing(instance, &translation.instance.statements);
    }
}

fn add_missing(description: &mut Description, statements: &[Statement]) {
    let stated: Vec<String> = description
        .statements
        .iter()
        .map(|statement| statement.property.clone())
        .collect();
    for statement in statements {
        if !stated.contains(&statement.property) && !description.statements.contains(statement) {
            description.statements.push(statement.clone());
        }
    }
}

/// Port: the translation a library stored.
pub trait TypeTranslationSource {
    /// The stored translation; empty when the library has none.
    ///
    /// # Errors
    ///
    /// When storage fails.
    fn type_translations(&self) -> Result<TypeTranslations, crate::catalog::CatalogReadError>;
}

/// The scheme and code of a coded statement, for listing a translation.
#[must_use]
pub fn coded(statement: &Statement) -> Option<(&str, &str)> {
    match &statement.value {
        Value::Concept { scheme, code } => Some((scheme, code)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "# A sample library\n\
        T1\twork\tclass\tText\n\
        T1\twork\tcontent\tcontentTypes:txt\n\
        T1\tinstance\tmedia\tmediaTypes:n\n\
        T1\tinstance\tcarrier\tcarriers:nc\n\
        \n\
        T2\twork\tcontent\tcontentTypes:tdi\n";

    #[test]
    fn a_file_reads_into_checked_statements_per_type() {
        let translations = parse(FILE).unwrap();
        let first = translations.0.get("T1").unwrap();
        assert_eq!(first.work.classes, ["Text"]);
        assert_eq!(
            first.work.statements,
            [concept("content", "contentTypes", "txt")]
        );
        assert_eq!(
            first.instance.statements,
            [
                concept("media", "mediaTypes", "n"),
                concept("carrier", "carriers", "nc")
            ]
        );
        assert!(translations.0.contains_key("T2"));
    }

    #[test]
    fn bad_lines_name_their_number() {
        assert_eq!(parse("T1\twork\tclass"), Err(TranslationError::Columns(1)));
        assert_eq!(
            parse("# x\nT1\titem\tclass\tText"),
            Err(TranslationError::Entity(2))
        );
        assert_eq!(
            parse("T1\twork\tcontent\ttxt"),
            Err(TranslationError::Value(1))
        );
        assert!(matches!(
            parse("T1\twork\tclass\tNotAClass"),
            Err(TranslationError::Refused { line: 1, .. })
        ));
        assert!(matches!(
            parse("T1\twork\tcontent\tcontentTypes:nothing"),
            Err(TranslationError::Refused { line: 1, .. })
        ));
    }

    #[test]
    fn the_record_wins_and_the_type_fills_the_rest() {
        let translations = parse(FILE).unwrap();
        let mut work = Description {
            classes: vec!["Text".to_owned()],
            statements: vec![concept("content", "contentTypes", "prm")],
        };
        let mut instance = Description::default();
        translations.apply("T1", &mut work, &mut instance);
        assert_eq!(work.classes, ["Text"]);
        assert_eq!(work.statements, [concept("content", "contentTypes", "prm")]);
        let mut instance_with_media = Description {
            classes: Vec::new(),
            statements: vec![concept("media", "mediaTypes", "s")],
        };
        translations.apply("T1", &mut work, &mut instance_with_media);
        assert_eq!(
            instance.statements,
            [
                concept("media", "mediaTypes", "n"),
                concept("carrier", "carriers", "nc")
            ]
        );
        assert_eq!(
            instance_with_media.statements,
            [
                concept("media", "mediaTypes", "s"),
                concept("carrier", "carriers", "nc")
            ]
        );
        // An unknown type changes nothing.
        let before = work.clone();
        translations.apply("T9", &mut work, &mut instance);
        assert_eq!(work, before);
    }
}
