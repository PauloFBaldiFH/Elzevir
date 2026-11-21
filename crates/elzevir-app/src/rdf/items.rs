// SPDX-License-Identifier: GPL-3.0-or-later
//! The Items of an Instance as BIBFRAME (bibframe-completo.md, item 28b): each registry row
//! by its name, never by a code of the MARC edge.

use elzevir_domain::catalog::{BranchId, ItemId};
use elzevir_domain::statement::{Description, EntityKind, EntityRef};

use super::record::statements;
use super::{IriBase, Node, Object, literal};
use crate::catalog::ItemStatus;

/// An Item as the RDF export reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldItem {
    /// The Item.
    pub id: ItemId,
    /// Barcode.
    pub barcode: Option<String>,
    /// Inventory number (tombo).
    pub inventory_number: Option<String>,
    /// The unit that owns it.
    pub branch: BranchId,
    /// The unit's name.
    pub branch_name: String,
    /// The name of its circulation policy.
    pub policy: String,
    /// Call number.
    pub call_number: Option<String>,
    /// The name of its shelving location.
    pub sublocation: Option<String>,
    /// The name of its collection.
    pub collection: Option<String>,
    /// Copy number.
    pub copy_number: Option<String>,
    /// Original free-text serial issue designation.
    pub issue_statement: Option<String>,
    /// Enumeration of the serial issue.
    pub enumeration: Option<String>,
    /// Chronology of the serial issue.
    pub chronology: Option<String>,
    /// Date of acquisition, ISO 8601 as written.
    pub acquired_on: Option<String>,
    /// Circulation status numbers.
    pub status: ItemStatus,
    /// Its classes and statements.
    pub description: Description,
}

/// The node of `item`, an Item of the Instance `instance`.
pub(super) fn item_node(item: &HeldItem, instance: &str, base: &IriBase) -> Node {
    let mut node = Node::blank("Item");
    node.types.extend(
        item.description
            .classes
            .iter()
            .map(|class| format!("{}{class}", super::BF)),
    );
    node.iri = Some(base.entity(EntityRef {
        entity: EntityKind::Item,
        id: item.id.0,
    }));
    node.add("itemOf", Object::Iri(instance.to_owned()));
    for (class, value) in [
        ("Barcode", &item.barcode),
        ("Local", &item.inventory_number),
    ] {
        if let Some(value) = value {
            let mut identifier = Node::blank(class);
            identifier.value(value);
            node.node("identifiedBy", identifier);
        }
    }
    if let Some(call_number) = &item.call_number {
        node.node("shelfMark", Node::labelled("ShelfMark", call_number));
    }
    let mut unit = Node::labelled("Organization", &item.branch_name);
    unit.iri = Some(base.organization(item.branch));
    node.add("heldBy", Object::Node(Box::new(unit)));
    // The collection is a second sublocation: the ontology has no property of its own
    // for it (spec, section 4, item 2).
    for name in [&item.sublocation, &item.collection].into_iter().flatten() {
        node.node("sublocation", Node::labelled("Sublocation", name));
    }
    if let Some(copy) = &item.copy_number {
        let mut number = Node::blank("Local");
        number.value(copy);
        number.text("qualifier", "copy number");
        node.node("identifiedBy", number);
    }
    for (class, value) in [
        ("EnumerationAndChronology", &item.issue_statement),
        ("Enumeration", &item.enumeration),
        ("Chronology", &item.chronology),
    ] {
        if let Some(value) = value {
            node.node("enumerationAndChronology", Node::labelled(class, value));
        }
    }
    if let Some(date) = &item.acquired_on {
        let mut acquisition = Node::blank("ImmediateAcquisition");
        acquisition.text("date", date);
        node.node("immediateAcquisition", acquisition);
    }
    node.node(
        "usageAndAccessPolicy",
        Node::labelled("UsePolicy", &item.policy),
    );
    if item.status.restricted != 0 {
        node.node(
            "usageAndAccessPolicy",
            coded("AccessPolicy", "restricted", item.status.restricted),
        );
    }
    for (label, number) in [
        ("not for loan", item.status.not_for_loan),
        ("damaged", item.status.damaged),
        ("lost", item.status.lost),
        ("withdrawn", item.status.withdrawn),
    ] {
        if number != 0 {
            node.node("status", coded("Status", label, number));
        }
    }
    statements(&mut node, &item.description.statements, base);
    node
}

/// A node of `class` with a label and, as `bf:code`, the library's own number for it.
fn coded(class: &str, label: &str, number: i64) -> Node {
    let mut node = Node::labelled(class, label);
    node.add("code", literal(&number.to_string()));
    node
}
