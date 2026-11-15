// SPDX-License-Identifier: GPL-3.0-or-later
//! Catalog commands and the read port (docs/specs/catalogo/modelo-bibliografico.md).
//!
//! Commands name vocabulary entries by their codes (`MAIN`, `BK`, `por`); the writer
//! resolves codes to numeric identities. Branches and item types must exist already;
//! other vocabulary codes (languages, relators, locations, collections, schemes,
//! thesauri) are created on first use with the code as their label.

use elzevir_domain::catalog::{
    AgentKind, AuthorityLinkKind, BranchId, CollectionId, HubId, IdentifierKind, InstanceId,
    ItemId, MaterialType, Money, NamePartKind, NoteKind, PolicyId, SubjectKind, SublocationId,
    WorkId, WorkRelationKind,
};
use serde::{Deserialize, Serialize};

use crate::codes::{CodeTable, NamedCode, NamingCount};
use crate::items::{ItemsUpdate, RowRefusal};
use crate::koha_types::TypeTranslations;
use crate::matching::Matching;
use crate::registries::{BranchContact, RegistryChange, RegistryEntry, RegistryRefusal};
use elzevir_domain::statement::{Description, EntityRef, Statement, StatementRefusal};

/// Catalog commands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CatalogCommand {
    /// Creates a branch.
    CreateBranch {
        /// Display name.
        name: String,
    },
    /// Creates a circulation policy.
    CreatePolicy {
        /// Display name.
        name: String,
    },
    /// Creates a Hub that Works can join.
    CreateHub {
        /// Title shared by the Works of the Hub, such as a uniform title.
        title: String,
    },
    /// Adds one bibliographic record: a new Work and its first Instance.
    AddRecord(Box<NewRecord>),
    /// Adds an Item to an Instance.
    AddItem(Box<NewItem>),
    /// Imports one record, matching it with a stored one first
    /// (docs/specs/catalogo/casamento.md).
    ImportRecord(Box<ImportRecord>),
    /// Replaces the description of a stored record (its Instance and Work) with `record`,
    /// keeping the ids, the Items, the Hub and the relations between Works. The Items of
    /// `record` are not used (docs/specs/catalogo/casamento.md, item 16).
    ReplaceRecord {
        /// The Instance to replace.
        instance: InstanceId,
        /// The new description.
        record: Box<NewRecord>,
    },
    /// Changes parts of a Work, if it is still at `row_version`
    /// (docs/specs/catalogo/edicao.md).
    UpdateWork(Box<WorkUpdate>),
    /// Changes parts of an Instance, if its record is still at `row_version`
    /// (docs/specs/catalogo/edicao.md).
    UpdateInstance(Box<InstanceUpdate>),
    /// Adds an Instance, without Items, to an existing Work
    /// (docs/specs/catalogo/criacao.md).
    AddInstance(Box<NewInstanceOfWork>),
    /// Saves an Instance's Items in one batch, all or nothing
    /// (docs/specs/catalogo/exemplares.md).
    SaveItems(Box<ItemsUpdate>),
    /// Gives codes of the code tables their names, creating the missing ones
    /// (docs/specs/catalogo/tabelas-de-codigos.md).
    NameCodes(Vec<NamedCode>),
    /// Changes the registries of branches, shelving places, collections and policies
    /// (docs/specs/catalogo/bibframe-completo.md, item 23d).
    ChangeRegistry(RegistryChange),
    /// Replaces the library's translation of its Koha item types, kept at the MARC edge
    /// (docs/specs/catalogo/bibframe-completo.md, item 23e).
    SetTypeTranslations(TypeTranslations),
    /// Replaces every BIBFRAME statement of a Hub, Work, Instance or Item, if it is still
    /// at `row_version` (docs/specs/catalogo/bibframe-completo.md, items 10a to 10g).
    SetStatements(Box<StatementsUpdate>),
}

/// The statements of one entity, as `catalog.set_statements` stores them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatementsUpdate {
    /// The entity described.
    pub subject: EntityRef,
    /// The version the editor read: the entity's own, or, for an Instance, its record's.
    pub row_version: i64,
    /// Every statement, in order; what is not here is removed.
    pub statements: Vec<Statement>,
}

/// Reads the BIBFRAME statements of an entity (items 10a to 10g).
pub trait StatementReader {
    /// The statements of `subject`, in the order stored; empty for an unknown entity.
    ///
    /// # Errors
    ///
    /// When the store cannot be read.
    fn statements(&self, subject: EntityRef) -> Result<Vec<Statement>, CatalogReadError>;

    /// The classes and statements of `subject` (item 10h); empty for an unknown entity.
    ///
    /// # Errors
    ///
    /// When the store cannot be read.
    fn description(&self, subject: EntityRef) -> Result<Description, CatalogReadError>;
}

/// A new Instance of a stored Work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewInstanceOfWork {
    /// The Work.
    pub work: WorkId,
    /// The Instance.
    pub instance: NewInstance,
}

/// An edit of a Work, read at `row_version`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkUpdate {
    /// The Work.
    pub work: WorkId,
    /// The Work's version when the editor read it ([`InstanceView::work_version`]).
    pub row_version: i64,
    /// What changes, in order; a later change of the same part wins.
    pub changes: Vec<WorkChange>,
}

/// One part of a Work that an edit replaces. Lists are replaced whole, in the new order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkChange {
    /// Preferred title.
    Title(String),
    /// Category of material.
    MaterialType(MaterialType),
    /// Classification number and its scheme code.
    Classification {
        /// The number, such as a DDC number.
        number: Option<String>,
        /// The scheme code, such as `ddc`.
        scheme: Option<String>,
    },
    /// Summary.
    Summary(Option<String>),
    /// Target audience.
    Audience(Option<String>),
    /// Language codes.
    Languages(Vec<String>),
    /// Contributors.
    Contributions(Vec<NewContribution>),
    /// Subjects.
    Subjects(Vec<NewSubject>),
    /// The statements of one property, replaced whole (docs/specs/catalogo/
    /// bibframe-completo.md, items 24 to 26).
    Statements {
        /// The property's local name.
        property: String,
        /// Its statements, in order.
        statements: Vec<Statement>,
    },
}

/// An edit of an Instance, read at `row_version`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceUpdate {
    /// The Instance.
    pub instance: InstanceId,
    /// The record's version when the editor read it ([`InstanceView::version`]).
    pub row_version: i64,
    /// What changes, in order; a later change of the same part wins.
    pub changes: Vec<InstanceChange>,
}

/// One part of an Instance that an edit replaces. Lists are replaced whole, in the new
/// order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstanceChange {
    /// Title as on the publication.
    Title(String),
    /// Category of material.
    MaterialType(MaterialType),
    /// Edition statement.
    Edition(Option<String>),
    /// Place of publication.
    Place(Option<String>),
    /// Publisher.
    Publisher(Option<String>),
    /// Date of publication, as written.
    Date(Option<String>),
    /// Physical description.
    Extent(Option<String>),
    /// Series statement.
    Series(Option<String>),
    /// Identifiers.
    Identifiers(Vec<NewIdentifier>),
    /// Notes.
    Notes(Vec<NewNote>),
}

/// A record to import with a matching rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportRecord {
    /// The record, as mapped from the file.
    pub record: NewRecord,
    /// How it is matched and what happens then.
    pub matching: Matching,
}

/// What importing one record did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportOutcome {
    /// A new record was stored.
    Created {
        /// The new Instance.
        instance: InstanceId,
        /// The stored Instance it matched, when it matched and was created anyway.
        matched: Option<InstanceId>,
        /// What happened to its Items.
        items: ImportedItems,
    },
    /// The record matched a stored one, whose description it replaced.
    Replaced {
        /// The stored Instance, now with the imported description.
        instance: InstanceId,
        /// What happened to its Items.
        items: ImportedItems,
    },
    /// The record matched a stored one, which was kept.
    Matched {
        /// The stored Instance.
        instance: InstanceId,
        /// What happened to its Items.
        items: ImportedItems,
    },
    /// The record matched nothing and was not stored.
    Skipped,
}

/// What happened to the Items of an imported record.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportedItems {
    /// Items stored, in order.
    pub added: Vec<ItemId>,
    /// Stored Items overwritten by an imported one with the same barcode, in order.
    pub replaced: Vec<ItemId>,
    /// Items refused; the others went in.
    pub refused: Vec<RefusedItem>,
}

/// One Item of an imported record that was not stored; the others were.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefusedItem {
    /// Position of the Item in the record (0-based).
    pub position: usize,
    /// Why it was refused.
    pub refusal: CatalogRefusal,
}

/// A new Work with its first Instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewRecord {
    /// Preferred title of the Work.
    pub work_title: String,
    /// Category of material of the Work.
    pub material_type: MaterialType,
    /// The Work's classes and statements (bibframe-completo.md, item 10h).
    #[serde(default)]
    pub work_description: Description,
    /// The Hub the Work belongs to, if any.
    pub hub: Option<HubId>,
    /// Relations from this Work to existing Works, in order.
    pub relations: Vec<NewWorkRelation>,
    /// Classification number, such as a DDC number.
    pub classification: Option<String>,
    /// Classification scheme code, such as `ddc`.
    pub classification_scheme: Option<String>,
    /// Summary.
    pub summary: Option<String>,
    /// Target audience, as written (`521`).
    #[serde(default)]
    pub audience: Option<String>,
    /// Language codes, in order.
    pub languages: Vec<String>,
    /// Contributors, in order.
    pub contributions: Vec<NewContribution>,
    /// Subjects, in order.
    pub subjects: Vec<NewSubject>,
    /// The Instance.
    pub instance: NewInstance,
    /// Items of the Instance, in order.
    pub items: Vec<ItemData>,
    /// The record as it arrived (MARC21 ISO 2709, UTF-8), kept byte for byte.
    pub source: Option<Vec<u8>>,
}

/// A relation from the new Work to an existing one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewWorkRelation {
    /// How the new Work relates to `work`.
    pub kind: WorkRelationKind,
    /// The existing Work.
    pub work: WorkId,
}

/// One contributor of a Work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewContribution {
    /// Kind of agent.
    pub kind: AgentKind,
    /// The agent's heading, as written.
    pub heading: String,
    /// Relator code or term, such as `aut`.
    pub role: Option<String>,
    /// The relator term as the record wrote it, such as `autor`, when it says more than
    /// `role`: beside a code, or with capitals.
    #[serde(default)]
    pub role_term: Option<String>,
    /// Whether this is the main entry (MARC21 1XX).
    pub primary: bool,
    /// Where each part of the heading after the first starts, in order. Empty when the
    /// heading is one part or was typed as text.
    #[serde(default)]
    pub name_parts: Vec<NamePart>,
    /// The agent's identifiers in other datasets (VIAF, Wikidata, LC names), in order.
    #[serde(default)]
    pub identifiers: Vec<AuthorityLink>,
}

/// A link of a name or subject to an authority or another dataset, as an IRI or a
/// control number.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorityLink {
    /// What it points at.
    pub kind: AuthorityLinkKind,
    /// The IRI or number.
    pub value: String,
}

/// A part of an agent's heading after the first: its kind and the character where it
/// starts in the heading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamePart {
    /// What the part is.
    pub kind: NamePartKind,
    /// The position of its first character in the heading, counted in characters.
    pub start: usize,
}

/// One subject of a Work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewSubject {
    /// Kind of subject.
    pub kind: SubjectKind,
    /// The heading, as written, its subdivisions after ` -- `.
    pub heading: String,
    /// Thesaurus code, such as `lcsh`.
    pub thesaurus: Option<String>,
    /// What each subdivision names, in order: the last `subdivisions.len()` parts of the
    /// heading, as the components of a `madsrdf:ComplexSubject` (a topic, a place, a period,
    /// a genre or form). Empty when the heading has no typed subdivisions; a subdivision
    /// without a kind is a topic.
    #[serde(default)]
    pub subdivisions: Vec<SubjectKind>,
    /// Where each part of the name starts, for a subject that names an agent: as the
    /// parts of a contribution's heading, counted in its name before the subdivisions.
    #[serde(default)]
    pub name_parts: Vec<NamePart>,
    /// The links the record gives the heading, in order.
    #[serde(default)]
    pub identifiers: Vec<AuthorityLink>,
}

/// An Instance's own data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct NewInstance {
    /// Title as on the publication.
    pub title: String,
    /// Category of material of the Instance; `None` takes the Work's (a digital
    /// edition of a printed book says `Digital` here).
    pub material_type: Option<MaterialType>,
    /// The Instance's classes and statements (bibframe-completo.md, item 10h).
    #[serde(default)]
    pub description: Description,
    /// Edition statement.
    pub edition: Option<String>,
    /// Place of publication.
    pub place: Option<String>,
    /// Publisher.
    pub publisher: Option<String>,
    /// Date of publication, as written.
    pub date: Option<String>,
    /// Physical description.
    pub extent: Option<String>,
    /// Series statement.
    pub series: Option<String>,
    /// Identifiers, in order.
    pub identifiers: Vec<NewIdentifier>,
    /// Notes, in order.
    #[serde(default)]
    pub notes: Vec<NewNote>,
}

/// One identifier of an Instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewIdentifier {
    /// Kind of identifier.
    pub kind: IdentifierKind,
    /// The value, as written.
    pub value: String,
    /// Qualifier, such as `(broch.)`.
    pub qualifier: Option<String>,
}

/// One note of an Instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewNote {
    /// Kind of note.
    pub kind: NoteKind,
    /// The note, as written.
    pub text: String,
}

/// A new Item of an existing Instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewItem {
    /// The Instance it belongs to.
    pub instance: InstanceId,
    /// The Item's data.
    pub item: ItemData,
}

/// The circulation status numbers of an Item; 0 means normal, other values are defined by
/// the library, as Koha's authorised values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ItemStatus {
    /// Not for loan.
    pub not_for_loan: i64,
    /// Damaged.
    pub damaged: i64,
    /// Lost.
    pub lost: i64,
    /// Withdrawn.
    pub withdrawn: i64,
    /// Restricted.
    pub restricted: i64,
}

impl ItemStatus {
    /// Whether the Item can be lent and is on the shelf as far as the catalog knows: every
    /// status is 0. Loans arrive with circulation (Phase 3) and will also count.
    ///
    /// Koha: the OPAC shows an Item as available unless it is not for loan, damaged, lost,
    /// withdrawn or restricted (`opac-detail.pl`, `Koha::Item`).
    #[must_use]
    pub const fn is_available(&self) -> bool {
        self.not_for_loan == 0
            && self.damaged == 0
            && self.lost == 0
            && self.withdrawn == 0
            && self.restricted == 0
    }
}

/// The counter signal of an Item in the staff listing (UI spec
/// §2). Loans, transit and holds arrive with circulation (Phase 3) and add variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ShelfSignal {
    /// Every status is 0: it can be lent.
    Available,
    /// Not for loan or restricted, but on the shelf.
    NotForLoan,
    /// Withdrawn, lost or damaged.
    OutOfCirculation,
}

impl ItemStatus {
    /// The signal shown for an Item with this status. Out of circulation wins over not
    /// for loan: a lost reference book is not on the shelf.
    #[must_use]
    pub const fn signal(&self) -> ShelfSignal {
        if self.withdrawn != 0 || self.lost != 0 || self.damaged != 0 {
            ShelfSignal::OutOfCirculation
        } else if self.not_for_loan != 0 || self.restricted != 0 {
            ShelfSignal::NotForLoan
        } else {
            ShelfSignal::Available
        }
    }
}

/// An Item's own data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemData {
    /// Barcode, unique when present.
    pub barcode: Option<String>,
    /// Inventory number (tombo), not necessarily unique.
    pub inventory_number: Option<String>,
    /// Home branch code.
    pub home_branch: String,
    /// Holding branch code.
    pub holding_branch: String,
    /// Item type code.
    pub item_type: String,
    /// Call number.
    pub call_number: Option<String>,
    /// Shelving location code.
    pub location: Option<String>,
    /// Collection code.
    pub collection: Option<String>,
    /// Purchase price.
    pub price: Option<Money>,
    /// Replacement price.
    pub replacement_price: Option<Money>,
    /// Copy number.
    pub copy_number: Option<String>,
    /// Circulation status numbers.
    pub status: ItemStatus,
    /// Date of acquisition, ISO 8601 as written.
    pub acquired_on: Option<String>,
}

/// What a catalog command created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogOutput {
    /// A branch was created.
    Branch(BranchId),
    /// A circulation policy was created.
    Policy(PolicyId),
    /// A Hub was created.
    Hub(HubId),
    /// A record was added.
    Record {
        /// The new Work.
        work: WorkId,
        /// Its Instance.
        instance: InstanceId,
        /// Its Items, in order.
        items: Vec<ItemId>,
    },
    /// An Item was added.
    Item(ItemId),
    /// A record was imported.
    Imported(ImportOutcome),
    /// A record's description was replaced.
    Replaced(InstanceId),
    /// A Work was edited; it is now at `row_version`.
    WorkUpdated {
        /// The Work.
        work: WorkId,
        /// Its new version.
        row_version: i64,
    },
    /// An Instance was edited; its record is now at `row_version`.
    InstanceUpdated {
        /// The Instance.
        instance: InstanceId,
        /// Its record's new version.
        row_version: i64,
    },
    /// A batch of Items was saved; the record is now at `row_version`.
    ItemsSaved {
        /// The Instance.
        instance: InstanceId,
        /// Its record's new version.
        row_version: i64,
        /// The Items of the batch's rows, in the batch's order.
        items: Vec<ItemId>,
    },
    /// Codes were named, counted per table in [`crate::codes::CodeTable::ALL`] order.
    CodesNamed([NamingCount; 4]),
    /// A registry changed; the row, when it was created, renamed or given contact data.
    RegistryChanged(Option<RegistryEntry>),
    /// The translation of Koha item types was replaced; how many types it has.
    TypeTranslationsSet(usize),
    /// The statements of an entity were replaced.
    StatementsSet {
        /// The entity.
        subject: EntityRef,
        /// Its new version.
        row_version: i64,
    },
    /// An Instance was added to a Work.
    InstanceAdded {
        /// The Work.
        work: WorkId,
        /// The new Instance.
        instance: InstanceId,
    },
}

/// Why a catalog command was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CatalogRefusal {
    /// No branch has this code.
    #[error("unknown branch `{0}`")]
    UnknownBranch(String),
    /// No circulation policy has this Koha item type code at the MARC edge.
    #[error("unknown item type `{0}`")]
    UnknownItemType(String),
    /// No Instance has this id.
    #[error("unknown instance {}", .0.0)]
    UnknownInstance(InstanceId),
    /// No Hub has this id.
    #[error("unknown hub {}", .0.0)]
    UnknownHub(HubId),
    /// No Work has this id.
    #[error("unknown work {}", .0.0)]
    UnknownWork(WorkId),
    /// Another Item has this barcode.
    #[error("barcode `{0}` is already in use")]
    BarcodeTaken(String),
    /// A code or name of `catalog.name_codes` is empty or too long; the position.
    #[error("code {0} of the list is empty or too long")]
    BadCodeName(usize),
    /// A registry change was refused.
    #[error("registry change refused: {0:?}")]
    Registry(RegistryRefusal),
    /// A statement breaks a rule of the ontology.
    #[error("{0}")]
    BadStatement(StatementRefusal),
    /// No Hub, Work, Instance or Item has this id.
    #[error("unknown {:?} {}", .0.entity, .0.id)]
    UnknownEntity(EntityRef),
    /// Cells of an Item batch that cannot be stored, all of them.
    #[error("{} cells of the item batch were refused", .0.len())]
    ItemRows(Vec<RowRefusal>),
    /// Someone changed the row after the editor read it.
    #[error("changed by someone else (version {current}, edit read {read}); reload")]
    Stale {
        /// The version the edit was read at.
        read: i64,
        /// The version stored now.
        current: i64,
    },
}

/// An Instance as the catalog shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceView {
    /// The Instance.
    pub id: InstanceId,
    /// The record's version: it grows whenever the Instance, its Work or its Items
    /// change. An Instance edit names it.
    pub version: i64,
    /// Its Work.
    pub work: WorkId,
    /// The Work's version: it grows whenever the Work changes. A Work edit names it.
    pub work_version: i64,
    /// The Work's preferred title.
    pub work_title: String,
    /// The Work's Hub, if any.
    pub hub: Option<HubId>,
    /// The Work's category of material; `None` only for rows written before it existed.
    pub work_material_type: Option<MaterialType>,
    /// The Instance's category of material; `None` as for the Work.
    pub material_type: Option<MaterialType>,
    /// Relations from the Work to other Works, in order.
    pub relations: Vec<(WorkRelationKind, WorkId)>,
    /// The Instance's title.
    pub title: String,
    /// The other titles of the Work and of the Instance (variant, parallel, abbreviated),
    /// each as its main title and subtitle.
    pub other_titles: Vec<String>,
    /// Publisher.
    pub publisher: Option<String>,
    /// Date of publication.
    pub date: Option<String>,
    /// The Work's classification number, such as a DDC number.
    pub classification: Option<String>,
    /// The Work's summary.
    pub summary: Option<String>,
    /// The Work's target audience.
    pub audience: Option<String>,
    /// Contributor headings with their kind, in order.
    pub contributors: Vec<(AgentKind, String)>,
    /// Subject headings with their kind, in order.
    pub subjects: Vec<(SubjectKind, String)>,
    /// Identifiers, in order.
    pub identifiers: Vec<(IdentifierKind, String)>,
    /// Notes with their kind, in order.
    pub notes: Vec<(NoteKind, String)>,
    /// The notes with a type from the `mnotetype` list, of the Work and of the Instance.
    pub typed_notes: Vec<String>,
    /// Language codes, in order.
    pub languages: Vec<String>,
}

/// An Item as the catalog shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemView {
    /// The Item.
    pub id: ItemId,
    /// Barcode.
    pub barcode: Option<String>,
    /// Inventory number (tombo).
    pub inventory_number: Option<String>,
    /// Home branch.
    pub home_branch: BranchId,
    /// Circulation policy.
    pub policy: PolicyId,
    /// Call number.
    pub call_number: Option<String>,
    /// Shelving place.
    pub location: Option<SublocationId>,
    /// Circulation status numbers.
    pub status: ItemStatus,
}

/// One row of a registry an Item refers to, with the name people read
/// (docs/specs/catalogo/exemplares.md, item 29).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Named<Id> {
    /// The row.
    pub id: Id,
    /// Its name.
    pub name: String,
}

/// A shelving place or a collection: a row of a branch's list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place<Id> {
    /// The row.
    pub id: Id,
    /// The branch it belongs to.
    pub branch: BranchId,
    /// Its name.
    pub name: String,
}

/// The registries an Item refers to, each in name order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CodeLists {
    /// Branches.
    pub branches: Vec<Named<BranchId>>,
    /// The contact data of each branch that has some.
    pub contacts: Vec<(BranchId, BranchContact)>,
    /// Circulation policies.
    pub policies: Vec<Named<PolicyId>>,
    /// Shelving places of every branch.
    pub locations: Vec<Place<SublocationId>>,
    /// Collections of every branch.
    pub collections: Vec<Place<CollectionId>>,
    /// The Koha codes of the MARC edge, each with the name of the branch, shelving place,
    /// collection or circulation policy it stands for (bibframe-completo.md, items 23b
    /// and 23c).
    pub edge: Vec<NamedCode>,
}

impl CodeLists {
    /// The name of a branch.
    #[must_use]
    pub fn branch(&self, id: BranchId) -> &str {
        self.branches
            .iter()
            .find(|entry| entry.id == id)
            .map_or("", |entry| entry.name.as_str())
    }

    /// The name of a circulation policy.
    #[must_use]
    pub fn policy(&self, id: PolicyId) -> &str {
        self.policies
            .iter()
            .find(|entry| entry.id == id)
            .map_or("", |entry| entry.name.as_str())
    }

    /// The name of a shelving place.
    #[must_use]
    pub fn location(&self, id: SublocationId) -> &str {
        self.locations
            .iter()
            .find(|entry| entry.id == id)
            .map_or("", |entry| entry.name.as_str())
    }

    /// The name of a collection.
    #[must_use]
    pub fn collection(&self, id: CollectionId) -> &str {
        self.collections
            .iter()
            .find(|entry| entry.id == id)
            .map_or("", |entry| entry.name.as_str())
    }

    /// The name of what a Koha code of the edge stands for; the text itself when no row
    /// has the code, since the edge writes the name of a row without one.
    #[must_use]
    pub fn edge_name<'a>(&'a self, table: CodeTable, code: &'a str) -> &'a str {
        self.edge
            .iter()
            .find(|entry| entry.table == table && entry.code == code)
            .map_or(code, |entry| entry.name.as_str())
    }
}

/// Port: reading the catalog.
pub trait CatalogReader: Send + Sync {
    /// One Instance, or `None`.
    ///
    /// # Errors
    ///
    /// When storage fails or holds a value this build does not understand.
    fn instance(&self, id: InstanceId) -> Result<Option<InstanceView>, CatalogReadError>;

    /// The Items of an Instance, in creation order.
    ///
    /// # Errors
    ///
    /// When storage fails.
    fn items(&self, instance: InstanceId) -> Result<Vec<ItemView>, CatalogReadError>;

    /// The version of an Instance's record: it grows whenever the Instance, its Work or its
    /// Items change, so a page rendered for one version is valid until the next. `None` if
    /// the Instance does not exist.
    ///
    /// # Errors
    ///
    /// When storage fails.
    fn record_version(&self, id: InstanceId) -> Result<Option<i64>, CatalogReadError>;

    /// The status of every Item of these Instances, in one read, for a page of results.
    ///
    /// # Errors
    ///
    /// When storage fails.
    fn item_statuses(
        &self,
        instances: &[InstanceId],
    ) -> Result<Vec<(InstanceId, ItemStatus)>, CatalogReadError>;

    /// The code tables, for showing names in place of codes.
    ///
    /// # Errors
    ///
    /// When storage fails.
    fn codes(&self) -> Result<CodeLists, CatalogReadError>;
}

/// The catalog could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("cannot read the catalog: {0}")]
pub struct CatalogReadError(pub String);
