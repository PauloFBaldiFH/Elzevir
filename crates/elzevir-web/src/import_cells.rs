// SPDX-License-Identifier: GPL-3.0-or-later
//! Translated explanations of refused spreadsheet rows and cells.
//!
//! The command line prints the English `Display` of these refusals; the staff
//! screen says the same thing through Fluent, with the row and column first.

use elzevir_app::import::batches::BatchRefusal;
use elzevir_app::import::sheet::{CellReason, Registry};
use elzevir_web_templates::I18n;

/// The sentence for a refusal that carries a location, or `None` when the
/// message key alone says everything.
pub(crate) fn explain(t: &I18n, refusal: &BatchRefusal) -> Option<String> {
    match refusal {
        BatchRefusal::Cell {
            row,
            column,
            reason,
        } => {
            let place = t.get_numbers(
                "import-cell-place",
                &[("row", *row), ("column", *column as u64)],
            );
            Some(format!("{place} {}", reason_text(t, reason)))
        }
        BatchRefusal::RepeatedBarcode {
            row,
            first,
            barcode,
        } => Some(t.get_texts(
            "import-cell-repeated-barcode",
            &[
                ("row", &row.to_string()),
                ("first", &first.to_string()),
                ("barcode", barcode),
            ],
        )),
        _ => None,
    }
}

fn reason_text(t: &I18n, reason: &CellReason) -> String {
    match reason {
        CellReason::OutOfOrder => t.get("import-cell-out-of-order"),
        CellReason::NoHeader => t.get("import-cell-no-header"),
        CellReason::ColumnCount => t.get("import-cell-column-count"),
        CellReason::ModelMarker => t.get("import-cell-model-marker"),
        CellReason::Repeated { field } => t.get_with("import-cell-repeated", "field", field),
        CellReason::Json { detail } => t.get_with("import-cell-json", "detail", detail),
        CellReason::ExpressionNeedsHub => t.get("import-cell-expression"),
        CellReason::HubNeedsIdentity => t.get("import-cell-hub"),
        CellReason::Description { detail } => {
            t.get_with("import-cell-description", "detail", detail)
        }
        CellReason::TitleShape => t.get("import-cell-title"),
        CellReason::Identifier => t.get("import-cell-identifier"),
        CellReason::MissingBarcode => t.get("import-cell-missing-barcode"),
        CellReason::NameRequired { field } => {
            t.get_with("import-cell-name-required", "field", &registry(t, *field))
        }
        CellReason::UnknownName { field, name } => t.get_texts(
            "import-cell-unknown-name",
            &[("field", &registry(t, *field)), ("name", name)],
        ),
        CellReason::AmbiguousName { field, name } => t.get_texts(
            "import-cell-ambiguous-name",
            &[("field", &registry(t, *field)), ("name", name)],
        ),
        CellReason::Item { problem, .. } => t.get(problem.message_key()),
        CellReason::IsbnGrouping => t.get("import-cell-isbn-grouping"),
        CellReason::GroupConflict { row } => {
            t.get_numbers("import-cell-group-conflict", &[("row", *row)])
        }
        CellReason::RecordLimit => t.get("import-cell-record-limit"),
    }
}

fn registry(t: &I18n, field: Registry) -> String {
    t.get(match field {
        Registry::HomeBranch => "import-registry-home-branch",
        Registry::HoldingBranch => "import-registry-holding-branch",
        Registry::Policy => "import-registry-policy",
        Registry::Location => "import-registry-location",
        Registry::Collection => "import-registry-collection",
    })
}

#[cfg(test)]
mod tests {
    use elzevir_app::items::{ItemColumn, RowProblem};

    use super::*;

    fn portuguese() -> I18n {
        I18n::portuguese().unwrap()
    }

    #[test]
    fn a_refused_cell_names_its_place_and_cause_in_portuguese() {
        let t = portuguese();
        let refusal = BatchRefusal::Cell {
            row: 2,
            column: 4,
            reason: CellReason::UnknownName {
                field: Registry::HomeBranch,
                name: "Sede".into(),
            },
        };
        let text = explain(&t, &refusal).unwrap();
        assert_eq!(
            text,
            "Linha 2, coluna 4: não há unidade de origem chamada \"Sede\"."
        );
    }

    #[test]
    fn every_reason_has_a_message_in_both_languages() {
        let reasons = [
            CellReason::OutOfOrder,
            CellReason::NoHeader,
            CellReason::ColumnCount,
            CellReason::ModelMarker,
            CellReason::Repeated {
                field: "Work.title".into(),
            },
            CellReason::Json {
                detail: "expected value".into(),
            },
            CellReason::ExpressionNeedsHub,
            CellReason::HubNeedsIdentity,
            CellReason::Description {
                detail: "title: catalog-error-required".into(),
            },
            CellReason::TitleShape,
            CellReason::Identifier,
            CellReason::MissingBarcode,
            CellReason::NameRequired {
                field: Registry::Policy,
            },
            CellReason::AmbiguousName {
                field: Registry::Collection,
                name: "Geral".into(),
            },
            CellReason::UnknownName {
                field: Registry::Location,
                name: "Estante".into(),
            },
            CellReason::Item {
                column: ItemColumn::Price,
                problem: RowProblem::Price,
            },
            CellReason::IsbnGrouping,
            CellReason::GroupConflict { row: 3 },
            CellReason::RecordLimit,
        ];
        for t in [I18n::english().unwrap(), portuguese()] {
            for registry_field in [
                Registry::HomeBranch,
                Registry::HoldingBranch,
                Registry::Policy,
                Registry::Location,
                Registry::Collection,
            ] {
                assert!(!registry(&t, registry_field).starts_with("import-"));
            }
            for reason in &reasons {
                let text = reason_text(&t, reason);
                assert!(
                    !text.is_empty() && !text.starts_with("import-"),
                    "{reason:?}"
                );
            }
            let barcode = BatchRefusal::RepeatedBarcode {
                row: 5,
                first: 2,
                barcode: "T-1".into(),
            };
            let text = explain(&t, &barcode).unwrap();
            assert!(text.contains('5') && text.contains('2') && text.contains("T-1"));
        }
        assert!(explain(&portuguese(), &BatchRefusal::ChooseMember).is_none());
    }
}
