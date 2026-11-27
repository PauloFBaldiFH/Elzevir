// SPDX-License-Identifier: GPL-3.0-or-later
//! Printed documents: spine labels (docs/specs/catalogo/etiquetas.md).

use elzevir_domain::labels::split_dewey_call_number;
use serde::Serialize;

use crate::listing::Family;

/// Most labels one sheet run prints (etiquetas.md, item 3).
pub const MAX_LABELS: usize = 5_000;

/// One spine label: an Item with a call number.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SpineLabel {
    /// The call number as stored.
    pub call_number: String,
    /// Its lines on the spine.
    pub lines: Vec<String>,
    /// The Item's barcode.
    pub barcode: Option<String>,
    /// The title of its Instance.
    pub title: String,
    /// The Work's main contributor.
    pub author: Option<String>,
}

/// The spine labels of the Items of `families`, in listing order, and how many Items were
/// left out for having no call number (etiquetas.md, item 2).
#[must_use]
pub fn spine_labels(families: &[Family]) -> (Vec<SpineLabel>, usize) {
    let mut labels = Vec::new();
    let mut without = 0;
    for work in families.iter().flat_map(|family| &family.works) {
        let author = work.main_contributor.as_ref().map(|(_, name)| name.clone());
        for instance in &work.instances {
            for item in &instance.items {
                let Some(call_number) = item.call_number.as_deref().map(str::trim) else {
                    without += 1;
                    continue;
                };
                if call_number.is_empty() {
                    without += 1;
                    continue;
                }
                labels.push(SpineLabel {
                    call_number: call_number.to_owned(),
                    lines: split_dewey_call_number(call_number),
                    barcode: item.barcode.clone(),
                    title: instance.title.clone(),
                    author: author.clone(),
                });
            }
        }
    }
    (labels, without)
}

/// Why a document was not printed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PrintError {
    /// More labels than one run prints.
    #[error("{count} labels, at most {max} at a time")]
    TooMany {
        /// Labels asked for.
        count: usize,
        /// The most there may be.
        max: usize,
    },
    /// The template did not typeset.
    #[error("the template failed: {0}")]
    Template(String),
}

/// Port: typesetting printed documents as PDF. The print crate implements it.
pub trait Printer: Send + Sync {
    /// A PDF with `labels`, in order, on the sheets of the spine label template.
    ///
    /// # Errors
    ///
    /// When there are too many labels or the template fails.
    fn spine_labels(&self, labels: &[SpineLabel]) -> Result<Vec<u8>, PrintError>;
}

#[cfg(test)]
mod tests {
    use elzevir_domain::catalog::{AgentKind, BranchId, InstanceId, ItemId, PolicyId, WorkId};

    use super::*;
    use crate::catalog::{ItemStatus, ItemView, Movement};
    use crate::listing::{FamilyCursor, FamilyHead, FamilyInstance, FamilyWork};
    use crate::material::Material;

    fn item(id: i64, call_number: Option<&str>) -> ItemView {
        ItemView {
            id: ItemId(id),
            barcode: Some(format!("000{id}")),
            inventory_number: None,
            home_branch: BranchId(1),
            policy: PolicyId(1),
            call_number: call_number.map(str::to_owned),
            location: None,
            status: ItemStatus::default(),
            due: None,
            movement: Movement::default(),
        }
    }

    #[test]
    fn each_item_with_a_call_number_gets_a_label() {
        let family = Family {
            cursor: FamilyCursor {
                sort_key: String::new(),
                head: FamilyHead::Work(WorkId(1)),
            },
            hub_title: None,
            works: vec![FamilyWork {
                id: WorkId(1),
                title: "Dom Casmurro".to_owned(),
                main_contributor: Some((AgentKind::Person, "Assis, Machado de".to_owned())),
                main_agent: None,
                classification: None,
                languages: Vec::new(),
                subjects: Vec::new(),
                instances: vec![FamilyInstance {
                    id: InstanceId(1),
                    version: 1,
                    title: "Dom Casmurro".to_owned(),
                    edition: None,
                    publisher: None,
                    date: None,
                    isbn: None,
                    material: Material::default(),
                    items: vec![
                        item(1, Some("869.3 A848d ex.1")),
                        item(2, None),
                        item(3, Some("  ")),
                        item(4, Some("869.3 A848d ex.2")),
                    ],
                }],
            }],
        };
        let (labels, without) = spine_labels(&[family]);
        assert_eq!(without, 2);
        assert_eq!(labels.len(), 2);
        assert_eq!(
            labels.first(),
            Some(&SpineLabel {
                call_number: "869.3 A848d ex.1".to_owned(),
                lines: vec!["869.3".to_owned(), "A848d".to_owned(), "ex.1".to_owned()],
                barcode: Some("0001".to_owned()),
                title: "Dom Casmurro".to_owned(),
                author: Some("Assis, Machado de".to_owned()),
            })
        );
    }
}
