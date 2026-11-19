// SPDX-License-Identifier: GPL-3.0-or-later
//! Archive member choice and spreadsheet column mapping from the staff import form.
//!
//! Both write the same recipe fields (`archive` and `table`) as the JSON recipe,
//! so the batch revision, validation and preview stay the same for either path.

use elzevir_app::import::batches::TableSource;
use elzevir_app::import::containers::{Inventory, Selection};
use elzevir_app::import::edits::Recipe;
use elzevir_app::import::sheet::{
    Column, GroupBy, Profile, TableSettings, mapping_targets, suggested_target,
};
use elzevir_app::import::table::model_columns;
use elzevir_web_templates::{ImportColumn, ImportColumns};

/// Applies one spreadsheet action from the submitted form pairs.
///
/// # Errors
/// Returns the message key of the refusal; the recipe is then left unchanged.
pub(crate) fn apply(
    recipe: &mut Recipe,
    inventory: Option<&Inventory>,
    source: Option<&TableSource>,
    form: &[(String, String)],
) -> Result<(), &'static str> {
    match field(form, "action") {
        Some("archive") => choose(recipe, inventory.ok_or("import-sheet-missing")?, form),
        Some("columns") => map(recipe, source.ok_or("import-sheet-missing")?, form),
        _ => Err("import-sheet-missing"),
    }
}

/// The first value submitted under `name`.
pub(crate) fn field<'a>(form: &'a [(String, String)], name: &str) -> Option<&'a str> {
    form.iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

/// Selects a member, and a worksheet when the member is a workbook, by their
/// positions in the inventory (`entry` or `entry/sheet`).
fn choose(
    recipe: &mut Recipe,
    inventory: &Inventory,
    form: &[(String, String)],
) -> Result<(), &'static str> {
    let invalid = "import-sheet-choice";
    let choice = field(form, "choice").ok_or(invalid)?;
    let (entry, sheet) = match choice.split_once('/') {
        Some((entry, sheet)) => (entry, Some(sheet)),
        None => (choice, None),
    };
    let entry = inventory
        .entries
        .get(entry.parse::<usize>().map_err(|_| invalid)?)
        .ok_or(invalid)?;
    let sheet = match sheet {
        Some(sheet) => Some(
            entry
                .sheets
                .get(sheet.parse::<usize>().map_err(|_| invalid)?)
                .ok_or(invalid)?
                .clone(),
        ),
        None if entry.sheets.is_empty() => None,
        None => return Err(invalid),
    };
    let previous = recipe.archive.take();
    let format = previous
        .as_ref()
        .filter(|previous| previous.member == entry.member)
        .and_then(|previous| previous.format);
    let selection = Selection {
        member: entry.member.clone(),
        sheet,
        format,
    };
    // A mapping describes the columns of one sheet; another sheet needs its own.
    if previous.as_ref() != Some(&selection) {
        recipe.table = None;
    }
    recipe.archive = Some(selection);
    Ok(())
}

/// Saves a versioned mapping for every column of the selected sheet, keeping
/// the delimiter and encoding the sheet is read with.
fn map(
    recipe: &mut Recipe,
    source: &TableSource,
    form: &[(String, String)],
) -> Result<(), &'static str> {
    let targets = mapping_targets();
    let columns = source
        .header
        .iter()
        .enumerate()
        .map(|(index, header)| {
            let position = index + 1;
            let target = field(form, &format!("target-{position}"))
                .map(str::trim)
                .filter(|target| targets.iter().any(|allowed| allowed == target))
                .ok_or("import-columns-invalid")?;
            Ok(Column {
                position,
                header: header.clone(),
                target: target.to_owned(),
                trim: field(form, &format!("trim-{position}")).is_some(),
            })
        })
        .collect::<Result<Vec<_>, &'static str>>()?;
    let group_by = match field(form, "group_by").unwrap_or("none") {
        "none" => None,
        "isbn" => Some(GroupBy::Isbn),
        "title" => Some(GroupBy::Title),
        _ => return Err("import-columns-group-invalid"),
    };
    recipe.table = Some(TableSettings {
        delimiter: source.settings.delimiter,
        encoding: source.settings.encoding,
        profile: Some(Profile {
            version: 1,
            columns,
        }),
        group_by,
    });
    Ok(())
}

/// The mapping form for this sheet, or `None` when the sheet already uses the
/// generated model and needs no mapping.
pub(crate) fn columns_view(source: &TableSource) -> Option<ImportColumns> {
    if source.header == model_columns() {
        return None;
    }
    let saved = source.settings.profile.as_ref();
    let columns = source
        .header
        .iter()
        .enumerate()
        .map(|(index, header)| {
            let position = index + 1;
            let kept = saved
                .and_then(|profile| profile.columns.get(index))
                .filter(|column| column.position == position && &column.header == header);
            ImportColumn {
                position,
                header: header.clone(),
                target: kept.map_or_else(
                    || suggested_target(header).unwrap_or_default(),
                    |column| column.target.clone(),
                ),
                trim: kept.is_some_and(|column| column.trim),
            }
        })
        .collect();
    let group_by = match source.settings.group_by {
        None => "none",
        Some(GroupBy::Isbn) => "isbn",
        Some(GroupBy::Title) => "title",
    };
    Some(ImportColumns {
        columns,
        targets: mapping_targets(),
        group_by,
    })
}

#[cfg(test)]
mod tests {
    use elzevir_app::import::containers::{Entry, Kind};
    use elzevir_app::import::table::{Delimiter, Encoding};
    use elzevir_app::import_detection::ImportFormat;

    use super::*;

    fn pairs(values: &[(&str, &str)]) -> Vec<(String, String)> {
        values
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    }

    fn source(header: &[&str]) -> TableSource {
        TableSource {
            header: header.iter().map(|cell| (*cell).to_owned()).collect(),
            settings: TableSettings {
                delimiter: Delimiter::Semicolon,
                encoding: Encoding::Utf8,
                profile: None,
                group_by: None,
            },
        }
    }

    fn inventory() -> Inventory {
        Inventory {
            entries: vec![
                Entry {
                    member: vec!["notes.mrc".into()],
                    kind: Kind::File(ImportFormat::Iso2709),
                    sheets: vec![],
                },
                Entry {
                    member: vec!["catalog.xlsx".into()],
                    kind: Kind::Xlsx,
                    sheets: vec!["Books".into(), "Items".into()],
                },
            ],
        }
    }

    #[test]
    fn mapping_keeps_the_reader_settings_and_every_column() {
        let source = source(&["Título", "Tombo", "Notas"]);
        let mut recipe = Recipe::default();
        let form = pairs(&[
            ("action", "columns"),
            ("target-1", "Work.title"),
            ("target-2", " Item.catalog.inventory_number "),
            ("trim-2", "on"),
            ("target-3", "ignore"),
            ("group_by", "isbn"),
        ]);
        apply(&mut recipe, None, Some(&source), &form).unwrap();
        let table = recipe.table.unwrap();
        assert!(matches!(table.delimiter, Delimiter::Semicolon));
        assert!(matches!(table.group_by, Some(GroupBy::Isbn)));
        let columns = table.profile.unwrap().columns;
        assert_eq!(columns.len(), 3);
        assert_eq!(columns[1].target, "Item.catalog.inventory_number");
        assert!(columns[1].trim && !columns[0].trim);
        assert_eq!(columns[2].header, "Notas");
    }

    #[test]
    fn mapping_refuses_unknown_or_missing_destinations() {
        let source = source(&["Título", "Tombo"]);
        let mut recipe = Recipe::default();
        for form in [
            pairs(&[("action", "columns"), ("target-1", "Work.title")]),
            pairs(&[
                ("action", "columns"),
                ("target-1", "Work.title"),
                ("target-2", "Item.catalog.item_type"),
            ]),
        ] {
            assert_eq!(
                apply(&mut recipe, None, Some(&source), &form),
                Err("import-columns-invalid")
            );
        }
        let group = pairs(&[
            ("action", "columns"),
            ("target-1", "ignore"),
            ("target-2", "ignore"),
            ("group_by", "author"),
        ]);
        assert_eq!(
            apply(&mut recipe, None, Some(&source), &group),
            Err("import-columns-group-invalid")
        );
        assert!(recipe.table.is_none());
    }

    #[test]
    fn the_view_suggests_aliases_and_keeps_saved_choices() {
        let mut source = source(&["Título", "Código de barras", "Observação"]);
        let view = columns_view(&source).unwrap();
        let targets: Vec<_> = view.columns.iter().map(|c| c.target.as_str()).collect();
        assert_eq!(targets, ["Work.title", "Item.catalog.barcode", ""]);
        let mut recipe = Recipe::default();
        let form = pairs(&[
            ("action", "columns"),
            ("target-1", "Instance.title"),
            ("target-2", "Item.catalog.barcode"),
            ("target-3", "ignore"),
            ("trim-3", "on"),
        ]);
        apply(&mut recipe, None, Some(&source), &form).unwrap();
        source.settings = recipe.table.unwrap();
        let view = columns_view(&source).unwrap();
        assert_eq!(view.columns[0].target, "Instance.title");
        assert!(view.columns[2].trim);
    }

    #[test]
    fn the_generated_model_needs_no_mapping() {
        let mut source = source(&[]);
        source.header = model_columns();
        assert!(columns_view(&source).is_none());
    }

    #[test]
    fn choosing_a_sheet_resets_a_mapping_made_for_another_one() {
        let inventory = inventory();
        let mut recipe = Recipe {
            table: Some(source(&["x"]).settings),
            ..Recipe::default()
        };
        let form = pairs(&[("action", "archive"), ("choice", "1/1")]);
        apply(&mut recipe, Some(&inventory), None, &form).unwrap();
        let selection = recipe.archive.clone().unwrap();
        assert_eq!(selection.member, ["catalog.xlsx"]);
        assert_eq!(selection.sheet.as_deref(), Some("Items"));
        assert!(recipe.table.is_none());

        recipe.table = Some(source(&["x"]).settings);
        apply(&mut recipe, Some(&inventory), None, &form).unwrap();
        assert!(recipe.table.is_some(), "the same choice keeps the mapping");
    }

    #[test]
    fn a_choice_must_name_an_existing_member_and_sheet() {
        let inventory = inventory();
        let mut recipe = Recipe::default();
        for choice in ["1", "2", "1/2", "0/0", "x", "-1"] {
            let form = pairs(&[("action", "archive"), ("choice", choice)]);
            assert_eq!(
                apply(&mut recipe, Some(&inventory), None, &form),
                Err("import-sheet-choice"),
                "{choice}"
            );
        }
        let form = pairs(&[("action", "archive"), ("choice", "0")]);
        apply(&mut recipe, Some(&inventory), None, &form).unwrap();
        assert_eq!(recipe.archive.as_ref().unwrap().sheet, None);
        assert_eq!(
            apply(&mut recipe, None, None, &pairs(&[("action", "columns")])),
            Err("import-sheet-missing")
        );
    }
}
