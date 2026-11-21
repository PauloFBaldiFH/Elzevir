// SPDX-License-Identifier: GPL-3.0-or-later
//! Preparation of native records from explicitly mapped CSV and TSV sheets.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::BufRead;

use elzevir_data::bibframe::{self, Property, PropertyKind};
use elzevir_domain::catalog::{BranchId, HubId, IdentifierKind};
use elzevir_domain::statement::{Description, EntityKind, Statement, Value, check_description};
use serde::{Deserialize, Serialize};

use super::preview::MAX_IMPORT_RECORDS;
use super::table::{
    Delimiter, Encoding, ITEM_CATALOG_COLUMNS, TableError, TableReader, TableRow, model_columns,
};
use super::{ImportError, ImportReport, RawPlace, RecordSink, collect};
use crate::catalog::{
    ClassChoice, CodeLists, InstanceChange, Named, NativeItem, NewIdentifier, NewRecord, Place,
    RecordItems, WorkChange,
};
use crate::creating::new_record;
use crate::items::{ItemColumn, RowProblem, TypedRow, item_rows};
use crate::matching::{Matching, identifier_key};

/// Delimiter, encoding and mapping agreed for this revision.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TableSettings {
    /// Explicit separator.
    pub delimiter: Delimiter,
    /// Explicit encoding.
    pub encoding: Encoding,
    /// Mapping of a custom sheet. Omit for the generated model.
    pub profile: Option<Profile>,
    /// Optional explicit grouping, with conflict checks.
    pub group_by: Option<GroupBy>,
}

/// The explicit key for forming a record with multiple items.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupBy {
    /// A unique normalized ISBN per input row.
    Isbn,
    /// The prepared Instance title.
    Title,
}

/// Versioned positional mapping, suitable for saving as TOML.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    /// Mapping format version.
    pub version: u32,
    /// All sheet columns, including explicit exclusions.
    pub columns: Vec<Column>,
}

/// One column, identified by both its number and its original header.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Column {
    /// One-based column.
    pub position: usize,
    /// Exact header in this position.
    pub header: String,
    /// Qualified destination or the explicit value "ignore".
    pub target: String,
    /// Trim surrounding whitespace before interpretation.
    #[serde(default)]
    pub trim: bool,
}

/// Preparation terminated before any write.
#[derive(Debug, thiserror::Error)]
pub enum SheetError {
    /// Source parse failure.
    #[error(transparent)]
    Input(#[from] TableError),
    /// The selected profile is incompatible with this header or this reader.
    #[error("{0}")]
    Profile(String),
    /// Located invalid or conflicting input.
    #[error("row {row}, column {column} (byte {offset}): {reason}")]
    Cell {
        /// One-based logical spreadsheet row.
        row: u64,
        /// One-based column, or 1 for a row-wide problem.
        column: usize,
        /// Byte offset of the row.
        offset: u64,
        /// Validation or mapping reason.
        reason: CellReason,
    },
    /// Duplicate barcode somewhere in the selected sheet.
    #[error("row {row}: item barcode {barcode:?} is repeated from row {first}")]
    DuplicateBarcode {
        /// Duplicate source row.
        row: u64,
        /// First source row with the barcode.
        first: u64,
        /// The exact duplicated barcode.
        barcode: String,
    },
}

/// Why a row or cell of the sheet was refused, so that each cause has its own message.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CellReason {
    /// A data row came before an earlier one.
    #[error("rows must follow source order")]
    OutOfOrder,
    /// The sheet has data rows but an empty header.
    #[error("the sheet has data but no column headers")]
    NoHeader,
    /// The row has more or fewer cells than the header.
    #[error("column count differs from the header")]
    ColumnCount,
    /// A version column of the generated model has a value on a data row.
    #[error("model metadata columns must be empty on data rows")]
    ModelMarker,
    /// Two columns of the row fill a field that takes one value.
    #[error("more than one value for {field}")]
    Repeated {
        /// The qualified destination.
        field: String,
    },
    /// A cell that holds JSON values could not be read.
    #[error("{detail}")]
    Json {
        /// The parser message, with the position in the cell.
        detail: String,
    },
    /// `expressionOf` does not name exactly one existing Hub.
    #[error("expressionOf requires one resolved Hub")]
    ExpressionNeedsHub,
    /// A Hub description has no stored Hub to describe.
    #[error("Hub descriptions require an existing identity")]
    HubNeedsIdentity,
    /// The prepared record fails the catalog validation.
    #[error("{detail}")]
    Description {
        /// Field and message key of each failure.
        detail: String,
    },
    /// A JSON title has more than one `mainTitle` or qualifiers.
    #[error("a title requires one mainTitle without additional qualifiers")]
    TitleShape,
    /// An identifier is not an ISBN, ISSN or DOI with one value.
    #[error("identifier requires a typed ISBN, ISSN or DOI resource with one value")]
    Identifier,
    /// An item without a barcode was not accepted explicitly.
    #[error("an item without a barcode requires explicit acceptance")]
    MissingBarcode,
    /// A registry name is empty.
    #[error("{field} name is required")]
    NameRequired {
        /// Which registry.
        field: Registry,
    },
    /// No registry entry has this name.
    #[error("unknown {field} {name:?}")]
    UnknownName {
        /// Which registry.
        field: Registry,
        /// The name in the cell.
        name: String,
    },
    /// More than one registry entry has this name.
    #[error("ambiguous {field} {name:?}")]
    AmbiguousName {
        /// Which registry.
        field: Registry,
        /// The name in the cell.
        name: String,
    },
    /// An item field cannot be stored as typed.
    #[error("item field {} refused: {problem:?}", column.name())]
    Item {
        /// The refused field.
        column: ItemColumn,
        /// Why.
        problem: RowProblem,
    },
    /// Grouping by ISBN needs exactly one normalized ISBN in the row.
    #[error("ISBN grouping requires one normalized ISBN per row")]
    IsbnGrouping,
    /// The row describes the record of its group differently.
    #[error("descriptions conflict with grouped row {row}")]
    GroupConflict {
        /// First row of the group.
        row: u64,
    },
    /// The sheet has more records than one batch accepts.
    #[error("prepared record count limit exceeded")]
    RecordLimit,
}

impl CellReason {
    /// The `Item.catalog` field the refusal is about, when one is.
    fn item_field(&self) -> Option<&'static str> {
        match self {
            Self::NameRequired { field }
            | Self::UnknownName { field, .. }
            | Self::AmbiguousName { field, .. } => Some(field.item_field()),
            Self::Item { column, .. } => Some(column.name()),
            Self::MissingBarcode => Some("barcode"),
            _ => None,
        }
    }
}

/// The registries an item names by their readable name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Registry {
    /// Home branch of the item.
    #[error("home branch")]
    HomeBranch,
    /// Branch that currently holds the item.
    #[error("holding branch")]
    HoldingBranch,
    /// Circulation policy.
    #[error("circulation policy")]
    Policy,
    /// Shelving location of the home branch.
    #[error("shelving location")]
    Location,
    /// Collection of the home branch.
    #[error("collection")]
    Collection,
}

impl Registry {
    const fn item_field(self) -> &'static str {
        match self {
            Self::HomeBranch => "home_branch",
            Self::HoldingBranch => "holding_branch",
            Self::Policy => "policy",
            Self::Location => "location",
            Self::Collection => "collection",
        }
    }
}

/// One prepared native record.
pub struct PreparedRow {
    /// One-based logical source row of the record or first row of a group.
    pub row: u64,
    /// Byte of that logical row.
    pub offset: u64,
    /// Validated record ready for the existing writer.
    pub record: NewRecord,
}

/// Whole-sheet preparation. Only the reader below can construct it.
pub struct PreparedTable {
    records: Vec<PreparedRow>,
    /// Nonempty data rows read, including the excluded ones.
    pub read: u64,
    /// Nonempty data rows explicitly excluded.
    pub excluded: u64,
}

impl PreparedTable {
    /// Records in the order of their first source row.
    #[must_use]
    pub fn records(&self) -> &[PreparedRow] {
        &self.records
    }

    /// Binds native records to the same confirmation evidence as MARC records.
    pub fn apply_review(&mut self, options: &crate::import::ImportOptions) {
        for (index, row) in self.records.iter_mut().enumerate() {
            row.record.instance.origin.clone_from(&options.origin);
            for note in &mut row.record.instance.notes {
                if note.kind == elzevir_domain::catalog::NoteKind::Local {
                    note.visibility = options.local_note_visibility;
                }
            }
            row.record.instance.reviewed = options.reviewed.get(index).cloned().map(Box::new);
        }
    }

    /// Submits the validated sheet in order through the normal writer queue.
    ///
    /// # Errors
    /// Reports when the writer stops accepting requests.
    pub fn submit<S: RecordSink>(
        self,
        sink: &S,
        matching: Option<Matching>,
    ) -> Result<ImportReport, ImportError> {
        let mut report = ImportReport::default();
        let mut waiting = VecDeque::new();
        for row in self.records {
            let place = RawPlace {
                index: row.row - 1,
                offset: row.offset,
            };
            waiting.push_back((place, sink.submit(row.record, matching)?));
            report.read += 1;
            // Keep pending responses bounded even when many rows enter the queue.
            if waiting.len() == 256 {
                collect(&mut waiting, &mut report);
            }
        }
        collect(&mut waiting, &mut report);
        Ok(report)
    }
}

/// Parses and validates the whole selected sheet before it can be submitted.
///
/// # Errors
/// Reports invalid encoding, mapping, names, values, grouping or repeated barcodes.
pub fn prepare(
    input: impl BufRead,
    settings: &TableSettings,
    codes: &CodeLists,
    exclude: &BTreeSet<u64>,
    accept_missing_barcodes: &BTreeSet<u64>,
) -> Result<PreparedTable, SheetError> {
    let reader = TableReader::new(input, settings.delimiter, settings.encoding)?;
    prepare_rows(reader, settings, codes, exclude, accept_missing_barcodes)
}

/// Applies the existing native mapping to rows from a validated spreadsheet.
///
/// # Errors
/// Reports invalid rows, mapping, names, values, grouping or repeated barcodes.
pub fn prepare_rows(
    rows: impl IntoIterator<Item = Result<TableRow, TableError>>,
    settings: &TableSettings,
    codes: &CodeLists,
    exclude: &BTreeSet<u64>,
    accept_missing_barcodes: &BTreeSet<u64>,
) -> Result<PreparedTable, SheetError> {
    let mut reader = rows.into_iter();
    let mut groups = Groups::default();
    let (mut read, mut excluded) = (0, 0);
    let mut last = 0;
    if let Some(header) = reader.next().transpose()? {
        let bindings = if empty(&header) {
            None
        } else {
            Some(bindings(&header.cells, settings.profile.as_ref())?)
        };
        for row in reader {
            let row = row?;
            let data_position = row
                .number
                .checked_sub(header.number)
                .filter(|position| *position > last)
                .ok_or_else(|| cell(&row, 1, CellReason::OutOfOrder))?;
            last = data_position;
            if empty(&row) {
                continue;
            }
            read += 1;
            // Selections refer to logical data positions, excluding the header.
            if exclude.contains(&data_position) {
                excluded += 1;
                continue;
            }
            let Some(bindings) = &bindings else {
                return Err(cell(&header, 1, CellReason::NoHeader));
            };
            if row.cells.len() != bindings.len() {
                return Err(cell(&row, 1, CellReason::ColumnCount));
            }
            let mut draft = Draft::default();
            for (position, (binding, value)) in bindings.iter().zip(&row.cells).enumerate() {
                let input = if binding.trim { value.trim() } else { value };
                if !input.is_empty() {
                    draft
                        .apply(binding.target, input)
                        .map_err(|reason| cell(&row, position + 1, reason))?;
                }
            }
            let (record, item) = draft
                .finish(codes, accept_missing_barcodes.contains(&data_position))
                .map_err(|reason| {
                    // Point at the column of the refused item field, when the
                    // sheet maps one; record-level refusals stay on column 1.
                    let column = reason
                        .item_field()
                        .and_then(|field| {
                            bindings.iter().position(
                                |binding| matches!(binding.target, Target::Item(item) if item == field),
                            )
                        })
                        .map_or(1, |index| index + 1);
                    cell(&row, column, reason)
                })?;
            groups.push(&row, record, item, settings.group_by)?;
        }
    }
    if let Some(position) = exclude
        .iter()
        .chain(accept_missing_barcodes)
        .find(|position| !(1..=last).contains(position))
    {
        return Err(SheetError::Profile(format!(
            "data row {position} is outside this sheet"
        )));
    }
    Ok(PreparedTable {
        records: groups.finish(),
        read,
        excluded,
    })
}

fn empty(row: &TableRow) -> bool {
    row.cells.iter().all(|cell| cell.trim().is_empty())
}

fn cell(row: &TableRow, column: usize, reason: CellReason) -> SheetError {
    SheetError::Cell {
        row: row.number,
        column,
        offset: row.offset,
        reason,
    }
}

#[derive(Clone, Copy)]
struct Binding {
    target: Target,
    trim: bool,
}

#[derive(Clone, Copy)]
enum Target {
    Ignore,
    Marker,
    Classes(EntityKind),
    Property(EntityKind, &'static Property),
    Isbn,
    Item(&'static str),
}

fn bindings(header: &[String], profile: Option<&Profile>) -> Result<Vec<Binding>, SheetError> {
    let invalid = |detail: &str| SheetError::Profile(detail.to_owned());
    if let Some(profile) = profile {
        if profile.version != 1 || profile.columns.len() != header.len() {
            return Err(invalid(
                "mapping version or column count differs from the sheet",
            ));
        }
        let mut selected = vec![None; header.len()];
        for column in &profile.columns {
            let position = column
                .position
                .checked_sub(1)
                .ok_or_else(|| invalid("column positions start at 1"))?;
            let slot = selected
                .get_mut(position)
                .ok_or_else(|| invalid("mapped column is absent"))?;
            if slot.is_some() || header.get(position) != Some(&column.header) {
                return Err(invalid("mapped column is repeated or its header changed"));
            }
            *slot = Some(Binding {
                target: target(&column.target)?,
                trim: column.trim,
            });
        }
        return selected
            .into_iter()
            .collect::<Option<_>>()
            .ok_or_else(|| invalid("every column needs a choice"));
    }

    if header != model_columns() {
        return Err(SheetError::Profile(mapping_help(header)));
    }
    header
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let target = if index < 2 {
                Target::Marker
            } else {
                target(name)?
            };
            Ok(Binding {
                target,
                trim: false,
            })
        })
        .collect()
}

/// Every destination a custom column can map to: `ignore`, the ISBN shortcut and
/// the qualified columns of the generated model, in model order.
#[must_use]
pub fn mapping_targets() -> Vec<String> {
    let mut targets = vec!["ignore".to_owned(), "Instance.catalog.isbn".to_owned()];
    // The first two model columns are version markers, not destinations.
    targets.extend(model_columns().into_iter().skip(2));
    targets
}

/// The destination of a header that already names one, or of a known alias
/// such as `Título` or `código de barras`.
#[must_use]
pub fn suggested_target(header: &str) -> Option<String> {
    const ALIASES: &str = include_str!("../../data/import-header-aliases.tsv");
    let name = header.trim();
    if name.len() > 128 {
        return None;
    }
    if target(name).is_ok() {
        return Some(name.to_owned());
    }
    let lower = name.to_lowercase();
    ALIASES
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .find_map(|(alias, destination)| (alias == lower).then(|| destination.to_owned()))
}

fn mapping_help(header: &[String]) -> String {
    let mut message =
        "select a versioned column mapping or use the generated BIBFRAME model".to_owned();
    let suggestions = header.iter().enumerate().filter_map(|(index, name)| {
        let destination = suggested_target(name)?;
        Some(format!("column {} -> {destination}", index + 1))
    });
    for suggestion in suggestions.take(8) {
        message.push('\n');
        message.push_str(&suggestion);
    }
    message
}

fn target(name: &str) -> Result<Target, SheetError> {
    let invalid = || SheetError::Profile(format!("unknown destination {name:?}"));
    if name == "ignore" {
        return Ok(Target::Ignore);
    }
    if name == "Instance.catalog.isbn" {
        return Ok(Target::Isbn);
    }
    if let Some(field) = name.strip_prefix("Item.catalog.") {
        return ITEM_CATALOG_COLUMNS
            .iter()
            .copied()
            .find(|allowed| *allowed == field)
            .map(Target::Item)
            .ok_or_else(invalid);
    }
    let (kind, name) = name.split_once('.').ok_or_else(invalid)?;
    let entity = match kind {
        "Hub" => EntityKind::Hub,
        "Work" => EntityKind::Work,
        "Instance" => EntityKind::Instance,
        "Item" => EntityKind::Item,
        _ => return Err(invalid()),
    };
    if name == "@type" {
        return Ok(Target::Classes(entity));
    }
    bibframe::property(name)
        .filter(|property| property.applies_to(entity.bibframe()))
        .map(|property| Target::Property(entity, property))
        .ok_or_else(invalid)
}

#[derive(Default)]
struct Draft {
    work: Vec<WorkChange>,
    instance: Vec<InstanceChange>,
    descriptions: BTreeMap<EntityKind, Description>,
    item: BTreeMap<&'static str, String>,
    identifiers: Vec<NewIdentifier>,
    hub: Option<HubId>,
    single: BTreeSet<(EntityKind, &'static str)>,
}

impl Draft {
    fn apply(&mut self, target: Target, value: &str) -> Result<(), CellReason> {
        match target {
            Target::Ignore => {}
            Target::Marker => {
                return Err(CellReason::ModelMarker);
            }
            Target::Item(field) => {
                if self.item.insert(field, value.to_owned()).is_some() {
                    return Err(CellReason::Repeated {
                        field: format!("Item.catalog.{field}"),
                    });
                }
            }
            Target::Isbn => self
                .identifiers
                .push(identifier(IdentifierKind::Isbn, value.to_owned())),
            Target::Classes(entity) => {
                let classes: Vec<String> = if value.trim_start().starts_with('[') {
                    serde_json::from_str(value).map_err(json_error)?
                } else {
                    vec![value.to_owned()]
                };
                self.descriptions
                    .entry(entity)
                    .or_default()
                    .classes
                    .extend(classes);
            }
            Target::Property(entity, property) => self.property(entity, property, value)?,
        }
        Ok(())
    }

    fn property(
        &mut self,
        entity: EntityKind,
        property: &'static Property,
        input: &str,
    ) -> Result<(), CellReason> {
        match (entity, property.name) {
            (EntityKind::Work, "title") => {
                self.once(entity, property.name)?;
                self.work.push(WorkChange::Title(title(input)?));
            }
            (EntityKind::Instance, "title") => {
                self.once(entity, property.name)?;
                self.instance.push(InstanceChange::Title(title(input)?));
            }
            (EntityKind::Work, "summary") if !input.trim_start().starts_with('[') => {
                self.once(entity, property.name)?;
                self.work.push(WorkChange::Summary(Some(input.to_owned())));
            }
            (EntityKind::Work, "intendedAudience") if !input.trim_start().starts_with('[') => {
                self.once(entity, property.name)?;
                self.work.push(WorkChange::Audience(Some(input.to_owned())));
            }
            (EntityKind::Work, "expressionOf") => {
                let values = json_values(input)?;
                if let [Value::Entity(entity)] = values.as_slice()
                    && entity.entity == EntityKind::Hub
                    && entity.id > 0
                    && self.hub.is_none()
                {
                    self.hub = Some(HubId(entity.id));
                } else {
                    return Err(CellReason::ExpressionNeedsHub);
                }
            }
            (EntityKind::Instance, "identifiedBy") => {
                for value in json_values(input)? {
                    self.identifiers.push(from_identifier(value)?);
                }
            }
            (EntityKind::Instance, "editionStatement") if !input.trim_start().starts_with('[') => {
                self.once(entity, property.name)?;
                self.instance
                    .push(InstanceChange::Edition(Some(input.to_owned())));
            }
            (EntityKind::Instance, "seriesStatement") if !input.trim_start().starts_with('[') => {
                self.once(entity, property.name)?;
                self.instance
                    .push(InstanceChange::Series(Some(input.to_owned())));
            }
            _ => {
                let values = if property.kind == PropertyKind::Datatype
                    && !input.trim_start().starts_with('[')
                {
                    vec![Value::Literal {
                        text: input.to_owned(),
                        language: None,
                    }]
                } else {
                    json_values(input)?
                };
                self.descriptions
                    .entry(entity)
                    .or_default()
                    .statements
                    .extend(values.into_iter().map(|value| Statement {
                        property: property.name.to_owned(),
                        value,
                    }));
            }
        }
        Ok(())
    }

    fn once(&mut self, entity: EntityKind, property: &'static str) -> Result<(), CellReason> {
        self.single
            .insert((entity, property))
            .then_some(())
            .ok_or_else(|| CellReason::Repeated {
                field: format!("{entity:?}.{property}"),
            })
    }

    fn finish(
        mut self,
        codes: &CodeLists,
        allow_missing: bool,
    ) -> Result<(NewRecord, Option<NativeItem>), CellReason> {
        if self.descriptions.contains_key(&EntityKind::Hub) {
            return Err(CellReason::HubNeedsIdentity);
        }
        let work = self
            .descriptions
            .remove(&EntityKind::Work)
            .unwrap_or_default();
        let instance = self
            .descriptions
            .remove(&EntityKind::Instance)
            .unwrap_or_default();
        let item = self
            .descriptions
            .remove(&EntityKind::Item)
            .unwrap_or_default();
        let classes = |description: &Description| {
            description
                .classes
                .iter()
                .cloned()
                .map(|class| ClassChoice { class })
                .collect()
        };
        self.work.push(WorkChange::Classes(classes(&work)));
        self.instance
            .push(InstanceChange::Classes(classes(&instance)));
        self.instance
            .push(InstanceChange::Identifiers(self.identifiers));
        let mut record =
            new_record(self.work, self.instance).map_err(|errors| CellReason::Description {
                detail: errors
                    .iter()
                    .map(|error| format!("{}: {}", error.field, error.message_key))
                    .collect::<Vec<_>>()
                    .join(", "),
            })?;
        record.work_description.statements = work.statements;
        record.instance.description.statements = instance.statements;
        record.hub = self.hub;
        for (kind, description) in [
            (EntityKind::Work, &record.work_description),
            (EntityKind::Instance, &record.instance.description),
            (EntityKind::Item, &item),
        ] {
            check_description(kind, description).map_err(|error| CellReason::Description {
                detail: error.to_string(),
            })?;
        }
        if self.item.is_empty() && item == Description::default() {
            return Ok((record, None));
        }
        let fields = resolve_item(&self.item, codes)?;
        if fields.barcode.is_none() && !allow_missing {
            return Err(CellReason::MissingBarcode);
        }
        Ok((
            record,
            Some(NativeItem {
                fields,
                description: item,
            }),
        ))
    }
}

fn json_values(input: &str) -> Result<Vec<Value>, CellReason> {
    serde_json::from_str(input).map_err(json_error)
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "used directly as the error mapper of serde_json results"
)]
fn json_error(error: serde_json::Error) -> CellReason {
    CellReason::Json {
        detail: error.to_string(),
    }
}

fn title(input: &str) -> Result<String, CellReason> {
    if !input.trim_start().starts_with('[') {
        return Ok(input.to_owned());
    }
    let mut values = json_values(input)?;
    if values.len() == 1
        && let Some(Value::Resource(resource)) = values.pop()
        && resource.class == "Title"
        && resource.label.is_none()
        && let [
            Statement {
                property,
                value:
                    Value::Literal {
                        text,
                        language: None,
                    },
            },
        ] = resource.statements.as_slice()
        && property == "mainTitle"
    {
        return Ok(text.clone());
    }
    Err(CellReason::TitleShape)
}

fn identifier(kind: IdentifierKind, value: String) -> NewIdentifier {
    NewIdentifier {
        kind,
        value,
        qualifier: None,
    }
}

fn from_identifier(value: Value) -> Result<NewIdentifier, CellReason> {
    if let Value::Resource(resource) = value {
        let kind = match resource.class.as_str() {
            "Isbn" => IdentifierKind::Isbn,
            "Issn" => IdentifierKind::Issn,
            "Doi" => IdentifierKind::Doi,
            _ => return Err(CellReason::Identifier),
        };
        if resource.statements.is_empty()
            && let Some(value) = resource.label
        {
            return Ok(identifier(kind, value));
        }
        if resource.label.is_none()
            && let [
                Statement {
                    property,
                    value:
                        Value::Literal {
                            text,
                            language: None,
                        },
                },
            ] = resource.statements.as_slice()
            && property == "value"
        {
            return Ok(identifier(kind, text.clone()));
        }
    }
    Err(CellReason::Identifier)
}

fn resolve_item(
    values: &BTreeMap<&str, String>,
    codes: &CodeLists,
) -> Result<crate::items::ItemFields, CellReason> {
    let get = |key| values.get(key).map_or("", |value| value.trim());
    let own = |key| get(key).to_owned();
    let home = named(&codes.branches, get("home_branch"), Registry::HomeBranch)?;
    let holding = named(
        &codes.branches,
        get("holding_branch"),
        Registry::HoldingBranch,
    )?;
    let policy = named(&codes.policies, get("policy"), Registry::Policy)?;
    let location = place(&codes.locations, home, get("location"), Registry::Location)?;
    let collection = place(
        &codes.collections,
        home,
        get("collection"),
        Registry::Collection,
    )?;
    let typed = TypedRow {
        id: None,
        barcode: own("barcode"),
        inventory_number: own("inventory_number"),
        home_branch: home.0.to_string(),
        holding_branch: holding.0.to_string(),
        policy: policy.0.to_string(),
        call_number: own("call_number"),
        location: location.map_or_else(String::new, |location| location.0.to_string()),
        collection: collection.map_or_else(String::new, |collection| collection.0.to_string()),
        copy_number: own("copy_number"),
        price: own("price"),
    };
    // One row in gives one row or at least one refusal out; the first names the cause.
    let mut rows = item_rows(vec![typed]).map_err(|errors| {
        errors.first().map_or(
            CellReason::Item {
                column: ItemColumn::Row,
                problem: RowProblem::Required,
            },
            |refusal| CellReason::Item {
                column: refusal.column,
                problem: refusal.problem,
            },
        )
    })?;
    rows.pop().map(|row| row.fields).ok_or(CellReason::Item {
        column: ItemColumn::Row,
        problem: RowProblem::Required,
    })
}

fn named<Id: Copy>(rows: &[Named<Id>], name: &str, field: Registry) -> Result<Id, CellReason> {
    one(
        rows.iter().filter(|row| row.name == name).map(|row| row.id),
        field,
        name,
    )
}

fn place<Id: Copy>(
    rows: &[Place<Id>],
    home: BranchId,
    name: &str,
    field: Registry,
) -> Result<Option<Id>, CellReason> {
    if name.is_empty() {
        return Ok(None);
    }
    one(
        rows.iter()
            .filter(|row| row.branch == home && row.name == name)
            .map(|row| row.id),
        field,
        name,
    )
    .map(Some)
}

fn one<T>(
    mut candidates: impl Iterator<Item = T>,
    field: Registry,
    name: &str,
) -> Result<T, CellReason> {
    match (name.is_empty(), candidates.next(), candidates.next()) {
        (false, Some(id), None) => Ok(id),
        (true, _, _) => Err(CellReason::NameRequired { field }),
        (_, None, _) => Err(CellReason::UnknownName {
            field,
            name: name.to_owned(),
        }),
        _ => Err(CellReason::AmbiguousName {
            field,
            name: name.to_owned(),
        }),
    }
}

#[derive(Default)]
struct Groups {
    records: Vec<(PreparedRow, Vec<NativeItem>)>,
    keys: BTreeMap<String, usize>,
    barcodes: BTreeMap<String, u64>,
}

impl Groups {
    fn push(
        &mut self,
        row: &TableRow,
        record: NewRecord,
        item: Option<NativeItem>,
        group: Option<GroupBy>,
    ) -> Result<(), SheetError> {
        if let Some(barcode) = item.as_ref().and_then(|item| item.fields.barcode.as_ref())
            && let Some(first) = self.barcodes.insert(barcode.clone(), row.number)
        {
            return Err(SheetError::DuplicateBarcode {
                row: row.number,
                first,
                barcode: barcode.clone(),
            });
        }
        let key = match group {
            Some(GroupBy::Title) => Some(record.instance.title.clone()),
            Some(GroupBy::Isbn) => {
                let keys: BTreeSet<_> = record
                    .instance
                    .identifiers
                    .iter()
                    .filter(|id| id.kind == IdentifierKind::Isbn)
                    .filter_map(|id| identifier_key(id.kind, &id.value))
                    .collect();
                if keys.len() != 1 {
                    return Err(cell(row, 1, CellReason::IsbnGrouping));
                }
                keys.into_iter().next().map(|key| key.key)
            }
            None => None,
        };
        if let Some(position) = key.as_ref().and_then(|key| self.keys.get(key)).copied()
            && let Some((previous, items)) = self.records.get_mut(position)
        {
            if previous.record != record {
                return Err(cell(
                    row,
                    1,
                    CellReason::GroupConflict { row: previous.row },
                ));
            }
            items.extend(item);
            return Ok(());
        }
        if self.records.len() as u64 == MAX_IMPORT_RECORDS {
            return Err(cell(row, 1, CellReason::RecordLimit));
        }
        if let Some(key) = key {
            self.keys.insert(key, self.records.len());
        }
        self.records.push((
            PreparedRow {
                row: row.number,
                offset: row.offset,
                record,
            },
            item.into_iter().collect(),
        ));
        Ok(())
    }

    fn finish(self) -> Vec<PreparedRow> {
        self.records
            .into_iter()
            .map(|(mut row, native)| {
                row.record.items = RecordItems::Native { native };
                row
            })
            .collect()
    }
}
