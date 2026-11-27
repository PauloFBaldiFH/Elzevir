// SPDX-License-Identifier: GPL-3.0-or-later
//! Exporting a selection of families over the real store, with the 200 records of the
//! staff export (docs/specs/catalogo/lote.md, items 6 to 8), and printing its spine labels
//! (docs/specs/catalogo/etiquetas.md).
#![expect(
    clippy::unwrap_used,
    reason = "integration test: helper functions fail the test on error"
)]

mod common;

use std::num::NonZeroUsize;

use elzevir_app::batch::instances_of;
use elzevir_app::export::{RecordFormat, export_record, export_selection};
use elzevir_app::listing::{CatalogListing, FamilyHead};
use elzevir_marc::Limits;

use common::imported;

#[test]
fn a_selection_exports_the_records_of_its_families_in_order() {
    let (_dir, store) = imported();
    let limits = Limits::default();
    // Twelve families of the listing, taken from the end first.
    let mut heads: Vec<FamilyHead> = store
        .families_after(None, NonZeroUsize::new(12).unwrap())
        .unwrap()
        .iter()
        .map(|family| family.cursor.head)
        .collect();
    heads.reverse();
    let instances = instances_of(&store, &heads).unwrap();
    assert!(instances.len() >= heads.len());
    let one_by_one: Vec<u8> = instances
        .iter()
        .flat_map(|&instance| {
            export_record(&store, instance, RecordFormat::Marc21, &limits)
                .unwrap()
                .unwrap()
        })
        .collect();
    let marc21 = export_selection(&store, &instances, RecordFormat::Marc21, &limits).unwrap();
    assert!(marc21.failures.is_empty());
    assert_eq!(marc21.bytes, one_by_one);
    let xml = export_selection(&store, &instances, RecordFormat::MarcXml, &limits).unwrap();
    let xml = String::from_utf8(xml.bytes).unwrap();
    assert_eq!(xml.matches("<record>").count(), instances.len());
    assert!(xml.starts_with("<?xml") && xml.ends_with("</collection>\n"));
}

#[test]
fn the_spine_labels_of_a_selection_are_typeset() {
    use elzevir_app::printing::{Printer, spine_labels};

    let (_dir, store) = imported();
    let families = store
        .families_after(None, NonZeroUsize::new(12).unwrap())
        .unwrap();
    let items: usize = families
        .iter()
        .flat_map(|family| &family.works)
        .flat_map(|work| &work.instances)
        .map(|instance| instance.items.len())
        .sum();
    let (labels, without) = spine_labels(&families);
    assert!(!labels.is_empty());
    assert_eq!(labels.len() + without, items);
    let pdf = elzevir_print::TypstPrinter::default()
        .spine_labels(&labels)
        .unwrap();
    assert!(pdf.starts_with(b"%PDF-"));
}
