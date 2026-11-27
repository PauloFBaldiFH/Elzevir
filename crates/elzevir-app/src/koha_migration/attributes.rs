// SPDX-License-Identifier: GPL-3.0-or-later
//! Koha's patron attributes (`borrower_attribute_types`, `borrower_attributes` and the
//! `authorised_values` their types choose from) for the migration
//! (docs/specs/circulacao/patron-attributes.md, items 16 to 18). Koha's codes stay at this
//! edge: the writer keeps them in `koha_keys`.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::dump::{Row, Table};
use super::{KohaPlan, number, text};
use crate::patron_fields::{FieldKind, FieldOption, MAX_OPTIONS, PatronFieldSpec};
use crate::patrons::MAX_NAME;

/// Most attribute values in one write command.
pub const VALUES_PER_COMMAND: usize = 5_000;

/// Settings of `borrower_attribute_types` the Elzevir does not keep, counted by the report
/// when a type uses them (spec, item 18).
pub const SETTINGS_NOT_KEPT: &[&str] = &[
    "opac_editable",
    "opac_mandatory",
    "display_checkout",
    "searched_by_default",
    "class",
    "keep_for_pseudonymization",
];

/// A Koha attribute type, ready to become a field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KohaFieldType {
    /// `code`.
    pub code: String,
    /// The field, without its category, which the writer finds by `category`.
    pub spec: PatronFieldSpec,
    /// `category_code`.
    pub category: Option<String>,
    /// For a list field, each `authorised_value` and the position of its option among
    /// `spec.kind`'s options.
    pub option_codes: Vec<(String, usize)>,
}

/// A value a Koha patron holds; its text stays inside the migration and never reaches the
/// report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KohaAttributeValue {
    /// `borrowernumber`.
    pub borrowernumber: u64,
    /// `code` of the type.
    pub code: String,
    /// `attribute`, trimmed.
    pub value: String,
}

/// One write: the types (on the first) and a share of the values.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KohaAttributesBatch {
    /// Types to turn into fields.
    pub types: Vec<KohaFieldType>,
    /// Values to give the migrated patrons.
    pub values: Vec<KohaAttributeValue>,
}

/// Why a value does not come (spec, item 17).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ValueLeftover {
    /// The patron was not migrated.
    Patron,
    /// The type is not in the dump, or did not become a field.
    Type,
    /// Not in the list of the field, not a date, or too long.
    Invalid,
    /// A second value of a field that is not repeatable, or more than the field takes.
    Repeated,
    /// A value of a unique field that another patron already holds.
    Taken,
}

/// What a write of attributes did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KohaAttributesMigrated {
    /// Fields created.
    pub fields_created: usize,
    /// Types a former run already made fields.
    pub fields_existing: usize,
    /// Options created.
    pub options: usize,
    /// Values given.
    pub values: usize,
    /// Values a former run already gave.
    pub already: usize,
    /// Values left out, by field name and reason.
    pub left: BTreeMap<(String, ValueLeftover), usize>,
    /// Types left out because their name and code are both taken by other fields.
    pub names_taken: Vec<String>,
}

impl KohaAttributesMigrated {
    /// Adds what another write did.
    pub fn add(&mut self, other: Self) {
        self.fields_created += other.fields_created;
        self.fields_existing += other.fields_existing;
        self.options += other.options;
        self.values += other.values;
        self.already += other.already;
        for (key, count) in other.left {
            *self.left.entry(key).or_default() += count;
        }
        self.names_taken.extend(other.names_taken);
    }
}

/// What reading the attributes found, besides the types and values.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttributesRead {
    /// For each setting of [`SETTINGS_NOT_KEPT`] and `branch limits`, how many types use it.
    pub settings_not_kept: Vec<(&'static str, usize)>,
    /// Types of a list whose category has no value, which come as text (edge case 4).
    pub list_as_text: usize,
    /// List types with more options than a field takes; the rest are left out.
    pub options_cut: usize,
}

/// Reads the types, their options and the values into the plan.
pub(super) fn read(tables: &BTreeMap<Table, Vec<Row>>, plan: &mut KohaPlan) {
    let rows = |table| tables.get(&table).map_or(&[][..], Vec::as_slice);
    let mut options: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    for row in rows(Table::AuthorisedValues) {
        let (Some(category), Some(code)) = (text(row, "category"), text(row, "authorised_value"))
        else {
            continue;
        };
        let label = text(row, "lib").unwrap_or_else(|| code.clone());
        options.entry(category).or_default().push((code, label));
    }
    let mut not_kept: BTreeMap<&'static str, usize> = BTreeMap::new();
    for row in rows(Table::AttributeTypes) {
        let Some(code) = text(row, "code") else {
            continue;
        };
        for setting in SETTINGS_NOT_KEPT {
            if text(row, setting).is_some_and(|value| value != "0") {
                *not_kept.entry(setting).or_default() += 1;
            }
        }
        let field = field_type(row, code, &options, &mut plan.attributes_read);
        plan.attribute_types.push(field);
    }
    let limited: BTreeSet<String> = rows(Table::AttributeTypeBranches)
        .iter()
        .filter_map(|row| text(row, "bat_code"))
        .collect();
    if !limited.is_empty() {
        not_kept.insert("branch limits", limited.len());
    }
    plan.attributes_read.settings_not_kept = not_kept.into_iter().collect();
    for row in rows(Table::Attributes) {
        let (Some(borrowernumber), Some(code), Some(value)) = (
            number(text(row, "borrowernumber").as_deref()),
            text(row, "code"),
            text(row, "attribute"),
        ) else {
            continue;
        };
        plan.attribute_values.push(KohaAttributeValue {
            borrowernumber,
            code,
            value,
        });
    }
}

/// The active currency, cut to three characters as Koha's SIP2 server sends it in `BH`
/// (spec, item 20).
///
/// Koha: `C4::SIP::ILS::Patron::new`
pub(super) fn active_currency(tables: &BTreeMap<Table, Vec<Row>>) -> Option<String> {
    tables
        .get(&Table::Currencies)?
        .iter()
        .find(|row| flag(row, "active"))
        .and_then(|row| text(row, "currency"))
        .map(|currency| currency.chars().take(3).collect())
}

fn flag(row: &Row, column: &str) -> bool {
    text(row, column).is_some_and(|value| value != "0")
}

/// The field a type becomes (spec, item 16).
fn field_type(
    row: &Row,
    code: String,
    options: &BTreeMap<String, Vec<(String, String)>>,
    read: &mut AttributesRead,
) -> KohaFieldType {
    let name: String = text(row, "description")
        .unwrap_or_else(|| code.clone())
        .chars()
        .take(MAX_NAME)
        .collect();
    let list = text(row, "authorised_value_category")
        .map(|category| options.get(&category).cloned().unwrap_or_default());
    let mut cut = false;
    let (kind, option_codes) = match list {
        Some(values) if !values.is_empty() => {
            // Two codes with the same label become one option, which both codes name.
            let mut labels: Vec<String> = Vec::new();
            let mut codes = Vec::new();
            for (code, label) in values {
                let label: String = label.chars().take(MAX_NAME).collect();
                let index = match labels.iter().position(|seen| *seen == label) {
                    Some(index) => index,
                    None if labels.len() < MAX_OPTIONS => {
                        labels.push(label);
                        labels.len() - 1
                    }
                    None => {
                        cut = true;
                        continue;
                    }
                };
                codes.push((code, index));
            }
            read.options_cut += usize::from(cut);
            let field_options = labels
                .into_iter()
                .map(|label| FieldOption { id: None, label })
                .collect();
            (FieldKind::List(field_options), codes)
        }
        Some(_) => {
            read.list_as_text += 1;
            (FieldKind::Text, Vec::new())
        }
        None if flag(row, "is_date") => (FieldKind::Date, Vec::new()),
        None => (FieldKind::Text, Vec::new()),
    };
    KohaFieldType {
        spec: PatronFieldSpec {
            name,
            kind,
            repeatable: flag(row, "repeatable"),
            unique: flag(row, "unique_id"),
            required: flag(row, "mandatory"),
            category: None,
            opac_shown: flag(row, "opac_display"),
            searchable: flag(row, "staff_searchable"),
        },
        category: text(row, "category_code"),
        option_codes,
        code,
    }
}

impl KohaPlan {
    /// The writes of the attributes, at most [`VALUES_PER_COMMAND`] values each; the first
    /// carries the types, and there is one whenever there is a type.
    #[must_use]
    pub fn attribute_batches(&self) -> Vec<KohaAttributesBatch> {
        if self.attribute_types.is_empty() {
            return Vec::new();
        }
        let mut chunks = self.attribute_values.chunks(VALUES_PER_COMMAND);
        let mut batches = vec![KohaAttributesBatch {
            types: self.attribute_types.clone(),
            values: chunks.next().map(<[_]>::to_vec).unwrap_or_default(),
        }];
        batches.extend(chunks.map(|values| KohaAttributesBatch {
            types: Vec::new(),
            values: values.to_vec(),
        }));
        batches
    }
}
