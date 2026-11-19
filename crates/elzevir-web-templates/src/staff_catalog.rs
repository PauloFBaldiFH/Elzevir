// SPDX-License-Identifier: GPL-3.0-or-later
//! The staff catalog listing: family rows (docs/specs/catalogo/listagem.md).

use askama::Template;

use crate::{Badge, GridOption, I18n};

/// The listing page, or only its rows for the next page.
#[derive(Template)]
#[template(path = "staff_catalog.html", blocks = ["rows"])]
pub struct StaffCatalog<'a> {
    /// Interface texts.
    pub t: &'a I18n,
    /// The session's CSRF token, which the page's script sends with an edit.
    pub csrf: &'a str,
    /// Whether the viewer may catalogue, and so create records (criacao.md, item 13).
    pub can_catalogue: bool,
    /// Whether the viewer may lend from a pill (interface §18.2).
    pub can_circulate: bool,
    /// The query, empty when browsing.
    pub query: &'a str,
    /// Records found, for a search.
    pub total: Option<usize>,
    /// What a scanned code was, for the page's script to open
    /// (docs/specs/catalogo/bipe.md, item 8).
    pub scan: Option<ScanLine>,
    /// The rows of this page.
    pub rows: Vec<FamilyLine>,
    /// Where the next page is, when there is one.
    pub next_url: Option<String>,
    /// The facets and filters strip (docs/specs/catalogo/facetas.md, part b), absent on a
    /// scanned code's page.
    pub strip: Option<FacetStrip>,
    /// What the batch change of Items offers, for a viewer who may catalogue
    /// (docs/specs/catalogo/lote.md, item 9).
    pub batch: Option<BatchChoices>,
}

/// The rows the batch change of Items chooses from, each list in name order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BatchChoices {
    /// Branches.
    pub branches: Vec<GridOption>,
    /// Circulation policies.
    pub policies: Vec<GridOption>,
    /// Shelving places, each with its branch's name.
    pub locations: Vec<GridOption>,
    /// Collections, each with its branch's name.
    pub collections: Vec<GridOption>,
    /// The saved lists the selection can join (lote.md, item 16).
    pub lists: Vec<GridOption>,
    /// The saved list shown, which the selection can leave (lote.md, item 20).
    pub current_list: Option<i64>,
}

/// The strip under the search: active filters as removable chips and a menu per facet.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FacetStrip {
    /// The work lists, the one shown marked (facetas.md, item 13).
    pub lists: Vec<WorkListLink>,
    /// Whether the work list shown is empty.
    pub list_empty: bool,
    /// Families matching in all; `None` on a work list, which is not counted.
    pub families: Option<usize>,
    /// The filters in force, each with the address that drops it.
    pub active: Vec<ActiveFilter>,
    /// The filters as `facet:value`, carried by the search form so a new query keeps them.
    pub kept: Vec<String>,
    /// The address without any filter, when there is one.
    pub clear_url: Option<String>,
    /// One menu per facet that has values.
    pub menus: Vec<FacetMenu>,
    /// The lists the team saved, in name order (docs/specs/catalogo/lote.md, item 18).
    pub saved: Vec<SavedListLink>,
    /// The saved list shown, which a cataloguer may delete (lote.md, item 20).
    pub deletable: Option<i64>,
}

/// One saved list in the strip's menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedListLink {
    /// Its name.
    pub name: String,
    /// Its address.
    pub url: String,
    /// Families it holds.
    pub families: usize,
    /// Whether it is the one shown.
    pub current: bool,
}

impl FacetStrip {
    /// The name of the saved list shown, which titles the menu.
    #[must_use]
    pub fn saved_current(&self) -> Option<&str> {
        self.saved
            .iter()
            .find(|list| list.current)
            .map(|list| list.name.as_str())
    }

    /// How many families match, as a sentence.
    #[must_use]
    pub fn families_text(&self, t: &I18n) -> Option<String> {
        self.families.map(|families| {
            t.get_numbers(
                "facet-families",
                &[("count", u64::try_from(families).unwrap_or(u64::MAX))],
            )
        })
    }
}

/// One work list as a segmented button.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkListLink {
    /// Message key of its name.
    pub label_key: &'static str,
    /// Its address.
    pub url: String,
    /// Whether it is the one shown.
    pub current: bool,
}

/// One filter in force.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveFilter {
    /// Message key of its facet.
    pub facet_key: &'static str,
    /// The value, named.
    pub label: String,
    /// The listing without it.
    pub remove_url: String,
}

/// The values of one facet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FacetMenu {
    /// Its name in URLs.
    pub name: &'static str,
    /// Message key of its title.
    pub title_key: &'static str,
    /// Its values, most families first.
    pub options: Vec<FacetOption>,
}

/// One value of a facet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FacetOption {
    /// The value, named.
    pub label: String,
    /// Families that have it.
    pub families: usize,
    /// The listing with it as a filter; `None` when it is already one.
    pub add_url: Option<String>,
}

impl StaffCatalog<'_> {
    /// Whether the page is a work list with nothing in it.
    #[must_use]
    pub fn list_empty(&self) -> bool {
        self.strip.as_ref().is_some_and(|strip| strip.list_empty)
    }

    /// How many records matched, as a sentence, for a search.
    #[must_use]
    pub fn count_text(&self) -> Option<String> {
        self.total.map(|total| {
            self.t.get_numbers(
                "staff-catalog-count",
                &[("count", u64::try_from(total).unwrap_or(u64::MAX))],
            )
        })
    }
}

/// One family: a compact row that opens into its blocks
/// (docs/specs/catalogo/listagem.md, items 27 to 32).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FamilyLine {
    /// Its head, `h<id>` or `w<id>`: the element id and the URL fragment of its focus.
    pub head: String,
    /// The Hub's title, or the Work's.
    pub title: String,
    /// Icon of the content of its first Instance that has one.
    pub icon: &'static str,
    /// Classification of its first Work.
    pub classification: Option<String>,
    /// Instances in all its Works.
    pub instances: usize,
    /// Items in all its Instances.
    pub items: usize,
    /// Of those, the ones available.
    pub available: usize,
    /// Whether it opens focused: the first family a search found an Instance in.
    pub open: bool,
    /// The agents shown in the authorities column, each once.
    pub authorities: Vec<AuthorityLine>,
    /// The Hub, for a Hub family.
    pub hub: Option<HubLine>,
    /// Its Works.
    pub works: Vec<WorkLine>,
}

impl FamilyLine {
    /// "2 works, 3 instances".
    #[must_use]
    pub fn shape_text(&self, t: &I18n) -> String {
        t.get_numbers(
            "staff-row-shape",
            &[
                ("works", u64::try_from(self.works.len()).unwrap_or(u64::MAX)),
                (
                    "instances",
                    u64::try_from(self.instances).unwrap_or(u64::MAX),
                ),
            ],
        )
    }

    /// "3 of 5 available".
    #[must_use]
    pub fn availability_text(&self, t: &I18n) -> String {
        t.get_numbers(
            "staff-row-available",
            &[
                (
                    "available",
                    u64::try_from(self.available).unwrap_or(u64::MAX),
                ),
                ("items", u64::try_from(self.items).unwrap_or(u64::MAX)),
            ],
        )
    }
}

/// An agent or a subject in the authorities column, linked to the Works that name it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorityLine {
    /// Position in the row, to link the cords.
    pub key: usize,
    /// Heading.
    pub name: String,
    /// Where its focused block is read (docs/specs/catalogo/autoridades.md).
    pub fields: Option<String>,
    /// Message key of its pill: agent or subject.
    pub pill_key: &'static str,
    /// Whether it is a subject: the row's summary lists only the agents.
    pub subject: bool,
    /// Symbol of its kind in `/static/elzevir-icons.svg`.
    pub icon: &'static str,
}

/// A cord from a block of the row to a Work: the node it starts at and the message key
/// of its label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cord {
    /// The `data-node` of the block it starts at.
    pub node: String,
    /// Message key of its label.
    pub label_key: &'static str,
}

/// The Hub block of a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HubLine {
    /// Hub id, to link the cords.
    pub id: i64,
    /// Title.
    pub title: String,
}

/// One Work in a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkLine {
    /// Work id, to link the cords.
    pub id: i64,
    /// Preferred title.
    pub title: String,
    /// Main contributor.
    pub contributor: Option<String>,
    /// The cords to it: from the Hub, the main contributor and the subjects shown.
    pub cords: Vec<Cord>,
    /// Classification number.
    pub classification: Option<String>,
    /// Language codes.
    pub languages: Vec<String>,
    /// The subjects shown.
    pub subjects: Vec<String>,
    /// Subjects not shown.
    pub more_subjects: usize,
    /// Its Instances.
    pub instances: Vec<InstanceLine>,
}

/// One Instance in a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceLine {
    /// Instance id, for the link to the record.
    pub id: i64,
    /// Title.
    pub title: String,
    /// Edition statement.
    pub edition: Option<String>,
    /// Publisher and date.
    pub imprint: Option<String>,
    /// First ISBN.
    pub isbn: Option<String>,
    /// Its content and carrier, each with its icon.
    pub material: Vec<Badge>,
    /// Icon of its carrier, else of its content, in `/static/elzevir-icons.svg`, shown on
    /// each item.
    pub icon: &'static str,
    /// Whether the search found it.
    pub found: bool,
    /// Which of the cover colours it gets.
    pub cover: u8,
    /// Items in all.
    pub items: usize,
    /// The pills shown.
    pub pills: Vec<PillLine>,
    /// Items not shown.
    pub more_items: usize,
}

impl InstanceLine {
    /// "+N items", for the Items not shown.
    #[must_use]
    pub fn more_items_text(&self, t: &I18n) -> String {
        t.get_numbers(
            "staff-catalog-more-items",
            &[("count", u64::try_from(self.more_items).unwrap_or(u64::MAX))],
        )
    }
}

/// One Item pill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PillLine {
    /// `branch | location | call number | inventory number`, the parts it has.
    pub text: String,
    /// CSS class of its signal.
    pub signal_class: &'static str,
    /// Message key of its signal.
    pub signal_key: &'static str,
    /// Symbol of its signal's icon in `/static/elzevir-icons.svg` (bibframe-completo.md,
    /// item 15): an indicator, or the padlock of local use.
    pub signal_icon: &'static str,
    /// The Item, for lending it from the pill.
    pub item: i64,
    /// Whether it can be lent from the pill: available and with a barcode.
    pub lendable: bool,
    /// The due date as shown, while it is on loan.
    pub due: Option<String>,
    /// Why the last checkout from the pill was refused; the box stays open.
    pub refused: Vec<String>,
}

/// The inside of one Item's pill, alone, after a checkout from it (interface §18.2).
#[derive(Template)]
#[template(path = "staff_item_pill.html")]
pub struct StaffItemPill<'a> {
    /// Interface texts.
    pub t: &'a I18n,
    /// Anti-forgery token of the session.
    pub csrf: &'a str,
    /// Whether the person may lend (the circulation permission).
    pub can_circulate: bool,
    /// The Instance of the Item.
    pub instance: PillInstance,
    /// The Item.
    pub pill: PillLine,
}

/// What the pill reads of its Instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PillInstance {
    /// Instance id.
    pub id: i64,
    /// Symbol of the Instance's carrier icon.
    pub icon: &'static str,
}

/// What the server found for a scanned code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanLine {
    /// `known` (open the Instance's grid) or `new-isbn` (open a new family).
    pub kind: &'static str,
    /// The Instance whose grid opens.
    pub instance: Option<i64>,
    /// The Item whose row gets the cursor, for a barcode.
    pub item: Option<i64>,
    /// The ISBN a new family starts with.
    pub isbn: Option<String>,
}
