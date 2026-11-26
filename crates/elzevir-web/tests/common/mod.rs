// SPDX-License-Identifier: GPL-3.0-or-later
//! Fakes shared by the HTTP tests.

use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use elzevir_app::catalog::{
    CatalogReadError, CatalogReader, CodeLists, InstanceView, ItemState, ItemStatus, ItemView,
    Named, Place, RecentInstance,
};
use elzevir_app::codes::{CodeTable, NamedCode};
use elzevir_app::listing::{CatalogListing, Family, FamilyCursor, FamilyHead};
use elzevir_app::search::{
    IndexBatch, SearchError, SearchHit, SearchIndex, SearchResults, SearchScope,
};
use elzevir_domain::catalog::{BranchId, InstanceId, PolicyId, SublocationId, WorkId};

/// One record: its version, the Instance and its Items.
pub(crate) type Record = (i64, InstanceView, Vec<ItemView>);

/// A catalog held in memory, counting how often records are read.
#[derive(Default)]
pub(crate) struct Catalog {
    /// Records by Instance, with their version and Items.
    pub(crate) records: Mutex<BTreeMap<i64, Record>>,
    /// Calls to [`CatalogReader::instance`].
    pub(crate) reads: AtomicUsize,
}

impl CatalogReader for Catalog {
    fn instance(&self, id: InstanceId) -> Result<Option<InstanceView>, CatalogReadError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        Ok(self
            .records
            .lock()
            .unwrap()
            .get(&id.0)
            .map(|(_, view, _)| view.clone()))
    }

    fn items(&self, instance: InstanceId) -> Result<Vec<ItemView>, CatalogReadError> {
        Ok(self
            .records
            .lock()
            .unwrap()
            .get(&instance.0)
            .map(|(_, _, items)| items.clone())
            .unwrap_or_default())
    }

    fn record_version(&self, id: InstanceId) -> Result<Option<i64>, CatalogReadError> {
        Ok(self
            .records
            .lock()
            .unwrap()
            .get(&id.0)
            .map(|(version, _, _)| *version))
    }

    fn item_statuses(
        &self,
        instances: &[InstanceId],
    ) -> Result<Vec<(InstanceId, ItemState)>, CatalogReadError> {
        Ok(instances
            .iter()
            .flat_map(|id| {
                self.items(*id)
                    .unwrap_or_default()
                    .into_iter()
                    .map(move |item| (*id, item.state()))
            })
            .collect())
    }

    fn codes(&self) -> Result<CodeLists, CatalogReadError> {
        let edge = |table, code: &str, name: &str| NamedCode {
            table,
            code: code.to_owned(),
            name: name.to_owned(),
        };
        Ok(CodeLists {
            contacts: Vec::new(),
            branches: vec![
                Named {
                    id: BranchId(1),
                    name: "Main Library".to_owned(),
                },
                Named {
                    id: BranchId(2),
                    name: "East Branch".to_owned(),
                },
            ],
            policies: vec![Named {
                id: PolicyId(1),
                name: "Normal loan".to_owned(),
            }],
            locations: vec![Place {
                id: SublocationId(1),
                branch: BranchId(1),
                name: "General stacks".to_owned(),
            }],
            collections: Vec::new(),
            edge: vec![
                edge(CodeTable::Branch, "CENTRAL", "Main Library"),
                edge(CodeTable::Location, "STACKS", "General stacks"),
                edge(CodeTable::ItemType, "T1", "Normal loan"),
            ],
        })
    }

    fn recent_instances(&self, limit: usize) -> Result<Vec<RecentInstance>, CatalogReadError> {
        Ok(self
            .records
            .lock()
            .unwrap()
            .values()
            .rev()
            .filter(|(_, view, _)| !view.opac_hidden)
            .take(limit)
            .map(|(_, view, items)| RecentInstance {
                id: view.id,
                title: view.title.clone(),
                contributor: view.contributors.first().map(|(_, name)| name.clone()),
                date: view.date.clone(),
                call_number: items.iter().find_map(|item| item.call_number.clone()),
            })
            .collect())
    }
}

/// An index that answers every query with the same hits.
#[derive(Default)]
pub(crate) struct Index(pub(crate) Vec<SearchHit>);

impl SearchIndex for Index {
    fn cursor(&self) -> Result<i64, SearchError> {
        Ok(0)
    }

    fn apply(&self, _: IndexBatch) -> Result<(), SearchError> {
        Ok(())
    }

    fn search(
        &self,
        _: &elzevir_app::search::SearchQuery<'_>,
        _: SearchScope,
        offset: usize,
        limit: usize,
    ) -> Result<SearchResults, SearchError> {
        Ok(SearchResults {
            total: self.0.len(),
            hits: self.0.iter().skip(offset).take(limit).cloned().collect(),
        })
    }

    fn facets(
        &self,
        _: &elzevir_app::search::SearchQuery<'_>,
        _: SearchScope,
    ) -> Result<elzevir_app::search::FacetCounts, SearchError> {
        use elzevir_app::search::{Facet, FacetCounts, FacetValue};
        // Every hit is in Portuguese; a third of them also in English.
        let value = |value: &str, families| FacetValue {
            value: value.to_owned(),
            families,
        };
        let n = self.0.len();
        Ok(FacetCounts {
            families: n,
            facets: if n == 0 {
                Vec::new()
            } else {
                vec![(
                    Facet::Language,
                    vec![value("por", n), value("eng", n / 3 + 1)],
                )]
            },
        })
    }
}

/// Families held in memory, in listing order.
#[derive(Default)]
pub(crate) struct Families(pub(crate) Vec<Family>);

impl CatalogListing for Families {
    fn families_after(
        &self,
        after: Option<&FamilyCursor>,
        limit: NonZeroUsize,
    ) -> Result<Vec<Family>, CatalogReadError> {
        Ok(self
            .0
            .iter()
            .filter(|family| after.is_none_or(|after| &family.cursor > after))
            .take(limit.get())
            .cloned()
            .collect())
    }

    fn families(&self, heads: &[FamilyHead]) -> Result<Vec<Family>, CatalogReadError> {
        Ok(heads
            .iter()
            .filter_map(|head| self.0.iter().find(|family| family.cursor.head == *head))
            .cloned()
            .collect())
    }

    /// Barcode `000123` is Item 3 of Instance 10 (Work 1), whose ISBN is 9788535910667.
    fn scanned(
        &self,
        code: elzevir_app::listing::ScanCode<'_>,
    ) -> Result<Option<elzevir_app::listing::Scanned>, CatalogReadError> {
        use elzevir_app::listing::{ScanCode, Scanned};
        let item = match code {
            ScanCode::Barcode("000123") => Some(elzevir_domain::catalog::ItemId(3)),
            ScanCode::Isbn("9788535910667") => None,
            _ => return Ok(None),
        };
        Ok(Some(Scanned {
            head: FamilyHead::Work(WorkId(1)),
            instance: InstanceId(10),
            item,
        }))
    }
}

/// Stored records held in memory, in Instance order.
pub(crate) struct Records(pub(crate) Vec<elzevir_app::export::StoredRecord>);

impl elzevir_app::export::RecordSource for Records {
    fn records_after(
        &self,
        after: Option<InstanceId>,
        limit: NonZeroUsize,
    ) -> Result<Vec<elzevir_app::export::StoredRecord>, CatalogReadError> {
        Ok(self
            .0
            .iter()
            .filter(|stored| after.is_none_or(|after| stored.instance > after))
            .take(limit.get())
            .cloned()
            .collect())
    }

    fn held_items(
        &self,
        _instance: InstanceId,
    ) -> Result<Vec<elzevir_app::rdf::HeldItem>, CatalogReadError> {
        Ok(Vec::new())
    }
}

/// Records edits from the screen. An edit read at version 1 is accepted and moves to 2;
/// any other version is stale, as if someone else had edited first. Creations are
/// accepted, except a new Instance of Work 404, which does not exist; a link of an
/// authority named `Linked` is refused as already there; a batch change that sets
/// shelving place 404 is refused for one Item, and otherwise reaches two Items an Instance.
#[derive(Default)]
pub(crate) struct Editor(
    pub(crate) Mutex<Vec<(elzevir_app::staff::StaffId, elzevir_app::editing::Edit)>>,
);

impl elzevir_app::editing::CatalogEditor for Editor {
    fn apply(
        &self,
        actor: elzevir_app::staff::StaffId,
        edit: elzevir_app::editing::Edit,
    ) -> Result<elzevir_app::catalog::CatalogOutput, elzevir_app::editing::EditFailed> {
        use elzevir_app::catalog::CatalogRefusal;
        use elzevir_app::editing::{Edit, EditFailed};
        let read = match &edit {
            Edit::Work(update) => update.row_version,
            Edit::Instance(update) => update.row_version,
            Edit::Items(update) => update.row_version,
            Edit::Statements(edit) => edit.row_version,
            Edit::Agent(update) => {
                if renamed_to_taken(&update.changes) {
                    return Err(EditFailed::Refused(CatalogRefusal::AgentHeadingTaken(
                        "Taken".to_owned(),
                    )));
                }
                update.row_version
            }
            Edit::NewInstance(new) if new.work.0 == 404 => {
                return Err(EditFailed::Refused(CatalogRefusal::UnknownWork(new.work)));
            }
            Edit::Registry(change) => {
                if let Err(refusal) = elzevir_app::registries::checked(change.clone()) {
                    return Err(EditFailed::Refused(CatalogRefusal::Registry(refusal)));
                }
                1
            }
            Edit::Subject(update) => {
                if renamed_to_taken(&update.changes) {
                    return Err(EditFailed::Refused(CatalogRefusal::SubjectHeadingTaken(
                        "Taken".to_owned(),
                    )));
                }
                update.row_version
            }
            Edit::Link(link) => {
                use elzevir_app::catalog::AuthorityRole as Role;
                let (Role::Contribution { heading, .. }
                | Role::Subject { heading, .. }
                | Role::PublicationPlace { heading, .. }) = &link.role;
                if heading == "Linked" {
                    return Err(EditFailed::Refused(CatalogRefusal::AlreadyLinked(
                        heading.clone(),
                    )));
                }
                1
            }
            Edit::BatchItems(change) => {
                if let Some(refusal) = batch_refusal(change) {
                    return Err(EditFailed::Refused(refusal));
                }
                1
            }
            Edit::SavedList(change) => {
                if let Some(refusal) = saved_list_refusal(change) {
                    return Err(EditFailed::Refused(CatalogRefusal::SavedList(refusal)));
                }
                1
            }
            Edit::NewRecord(_) | Edit::NewInstance(_) | Edit::Inventory(_) => 1,
        };
        if read != 1 {
            return Err(EditFailed::Refused(CatalogRefusal::Stale {
                read,
                current: 2,
            }));
        }
        let output = output_of(&edit);
        self.0.lock().unwrap().push((actor, edit));
        Ok(output)
    }
}

/// Whether an authority edit renames it to `Taken`, the heading of another one.
fn renamed_to_taken(changes: &[elzevir_app::catalog::AgentChange]) -> bool {
    changes.iter().any(
        |change| matches!(change, elzevir_app::catalog::AgentChange::Heading(h) if h == "Taken"),
    )
}

/// One agent, 1, named by three Works with four Instances; one subject, 1, of two Works
/// with two Instances.
pub(crate) struct Authorities;

impl elzevir_app::authorities::AuthorityReader for Authorities {
    fn agent(
        &self,
        id: elzevir_domain::catalog::AgentId,
    ) -> Result<Option<elzevir_app::authorities::AgentView>, CatalogReadError> {
        Ok((id.0 == 1).then(|| elzevir_app::authorities::AgentView {
            id,
            kind: elzevir_domain::catalog::AgentKind::Person,
            heading: "Assis, Machado de, 1839-1908".to_owned(),
            version: 1,
            works: 3,
            instances: 4,
            identifiers: vec![
                elzevir_app::catalog::AuthorityLink {
                    kind: elzevir_domain::catalog::AuthorityLinkKind::RealWorldObject,
                    value: "http://viaf.org/viaf/27068555".to_owned(),
                },
                elzevir_app::catalog::AuthorityLink {
                    kind: elzevir_domain::catalog::AuthorityLinkKind::AuthorityRecord,
                    value: "(DLC)n  79021164".to_owned(),
                },
            ],
        }))
    }

    fn subject(
        &self,
        id: elzevir_domain::catalog::SubjectId,
    ) -> Result<Option<elzevir_app::authorities::SubjectView>, CatalogReadError> {
        Ok((id.0 == 1).then(|| elzevir_app::authorities::SubjectView {
            id,
            kind: elzevir_domain::catalog::SubjectKind::Topic,
            heading: "Romance brasileiro".to_owned(),
            thesaurus: Some("lcsh".to_owned()),
            version: 1,
            works: 2,
            instances: 2,
            identifiers: Vec::new(),
        }))
    }
}

/// Suggests from a fixed list of agents and subjects.
pub(crate) struct Suggestions;

impl elzevir_app::suggestions::AuthoritySuggestions for Suggestions {
    fn agents(
        &self,
        prefix: &str,
        limit: usize,
    ) -> Result<Vec<elzevir_app::suggestions::AgentSuggestion>, CatalogReadError> {
        Ok(["Assis, Machado de, 1839-1908", "Assunção, Maria"]
            .into_iter()
            .filter(|heading| heading.to_lowercase().starts_with(&prefix.to_lowercase()))
            .take(limit)
            .map(|heading| elzevir_app::suggestions::AgentSuggestion {
                heading: heading.to_owned(),
                kind: elzevir_domain::catalog::AgentKind::Person,
            })
            .collect())
    }

    fn subjects(
        &self,
        prefix: &str,
        limit: usize,
    ) -> Result<Vec<elzevir_app::suggestions::SubjectSuggestion>, CatalogReadError> {
        Ok(["Romance brasileiro"]
            .into_iter()
            .filter(|heading| heading.to_lowercase().starts_with(&prefix.to_lowercase()))
            .take(limit)
            .map(|heading| elzevir_app::suggestions::SubjectSuggestion {
                heading: heading.to_owned(),
                kind: elzevir_domain::catalog::SubjectKind::Topic,
                thesaurus: Some("larpcal".to_owned()),
            })
            .collect())
    }
}

/// The grid of Instance 10: one Item, and the codes the cells choose from.
pub(crate) struct ItemGrid;

impl elzevir_app::items::ItemGridSource for ItemGrid {
    fn grid_items(
        &self,
        instance: InstanceId,
    ) -> Result<Vec<elzevir_app::items::GridItem>, CatalogReadError> {
        if instance != InstanceId(10) {
            return Ok(Vec::new());
        }
        Ok(vec![elzevir_app::items::GridItem {
            id: elzevir_domain::catalog::ItemId(3),
            fields: elzevir_app::items::ItemFields {
                barcode: Some("000123".to_owned()),
                inventory_number: Some("T-77".to_owned()),
                home_branch: BranchId(1),
                holding_branch: BranchId(1),
                policy: PolicyId(1),
                call_number: Some("869.3 A848d".to_owned()),
                location: None,
                collection: None,
                copy_number: None,
                price: Some(elzevir_domain::catalog::Money(4590)),
            },
            status: ItemStatus::default(),
        }])
    }
}

/// Item 3 with one note of custodial history; every other entity unknown.
pub(crate) struct Statements;

impl elzevir_app::catalog::StatementReader for Statements {
    fn statements(
        &self,
        subject: elzevir_domain::statement::EntityRef,
    ) -> Result<Vec<elzevir_domain::statement::Statement>, CatalogReadError> {
        Ok(self.description(subject)?.statements)
    }

    fn description(
        &self,
        subject: elzevir_domain::statement::EntityRef,
    ) -> Result<elzevir_domain::statement::Description, CatalogReadError> {
        use elzevir_domain::statement::{Description, EntityKind, Statement, Value};
        if subject.entity != EntityKind::Item || subject.id != 3 {
            return Ok(Description::default());
        }
        Ok(Description {
            classes: Vec::new(),
            statements: vec![Statement {
                property: "custodialHistory".to_owned(),
                value: Value::Literal {
                    text: "Doação da família Baldi".to_owned(),
                    language: None,
                },
            }],
        })
    }

    fn version(
        &self,
        subject: elzevir_domain::statement::EntityRef,
    ) -> Result<Option<i64>, CatalogReadError> {
        use elzevir_domain::statement::EntityKind;
        Ok((subject.entity == EntityKind::Item && subject.id == 3).then_some(4))
    }
}

/// Instance 10 was imported before the log kept times, then edited by ana; nothing else
/// has a history.
pub(crate) struct History;

impl elzevir_app::history::HistoryReader for History {
    fn history(
        &self,
        entity: elzevir_domain::statement::EntityRef,
        _limit: usize,
    ) -> Result<Vec<elzevir_app::history::LoggedCommand>, CatalogReadError> {
        use elzevir_app::catalog::{CatalogCommand, InstanceChange, InstanceUpdate};
        use elzevir_app::commands::WriteCommand;
        use elzevir_app::history::LoggedCommand;
        use elzevir_domain::statement::EntityKind;

        if entity.entity != EntityKind::Instance || entity.id != 10 {
            return Ok(Vec::new());
        }
        let edit =
            WriteCommand::Catalog(CatalogCommand::UpdateInstance(Box::new(InstanceUpdate {
                instance: InstanceId(10),
                row_version: 1,
                changes: vec![
                    InstanceChange::Edition(Some("2. ed.".to_owned())),
                    InstanceChange::Statements {
                        property: "dimensions".to_owned(),
                        statements: Vec::new(),
                    },
                ],
            })));
        Ok(vec![
            LoggedCommand {
                at: Some("2026-11-16T15:32:22Z".to_owned()),
                actor: Some("ana".to_owned()),
                command: serde_json::to_string(&edit).unwrap_or_default(),
            },
            LoggedCommand {
                at: None,
                actor: None,
                command: r#"{"Catalog":{"ImportRecord":{}}}"#.to_owned(),
            },
        ])
    }
}

/// The staff account `n` last touched Work `n`, and before it Works 35 down to 1; nothing
/// was imported today. Saved list 3, "Doações", holds Works 2 and 1.
pub(crate) struct WorkLists;

impl elzevir_app::worklist::WorkListReader for WorkLists {
    fn heads(
        &self,
        query: elzevir_app::worklist::WorkListQuery,
        offset: usize,
        limit: NonZeroUsize,
    ) -> Result<Vec<FamilyHead>, CatalogReadError> {
        use elzevir_app::worklist::WorkListQuery;
        let staff = match query {
            WorkListQuery::TouchedBy(staff) => staff,
            WorkListQuery::Saved(list) if list.0 == 3 => {
                return Ok(vec![
                    FamilyHead::Work(WorkId(2)),
                    FamilyHead::Work(WorkId(1)),
                ]);
            }
            _ => return Ok(Vec::new()),
        };
        Ok(std::iter::once(staff.0)
            .chain((1..=35).rev().filter(|n| *n != staff.0))
            .skip(offset)
            .take(limit.get())
            .map(|n| FamilyHead::Work(WorkId(n)))
            .collect())
    }

    fn saved_lists(&self) -> Result<Vec<elzevir_app::worklist::SavedList>, CatalogReadError> {
        use elzevir_app::worklist::{SavedList, SavedListId};
        Ok(vec![SavedList {
            id: SavedListId(3),
            name: "Doações".to_owned(),
            families: 2,
        }])
    }
}

/// A new list named `Taken` repeats a name; list 404 does not exist.
fn saved_list_refusal(
    change: &elzevir_app::worklist::SavedListChange,
) -> Option<elzevir_app::worklist::SavedListRefusal> {
    use elzevir_app::worklist::{SavedListChange, SavedListRefusal};
    match change {
        SavedListChange::Create { name, .. } if name == "Taken" => {
            Some(SavedListRefusal::NameTaken)
        }
        SavedListChange::Add { list, .. }
        | SavedListChange::Remove { list, .. }
        | SavedListChange::Delete { list }
            if list.0 == 404 =>
        {
            Some(SavedListRefusal::Unknown)
        }
        _ => None,
    }
}

/// A batch change that sets shelving place 404 is refused for one Item.
fn batch_refusal(
    change: &elzevir_app::batch::ItemsChange,
) -> Option<elzevir_app::catalog::CatalogRefusal> {
    use elzevir_app::batch::{ItemRefusal, ItemSet};
    use elzevir_app::items::{ItemColumn, RowProblem};
    use elzevir_domain::catalog::{ItemId, SublocationId};
    change
        .set
        .contains(&ItemSet::Location(Some(SublocationId(404))))
        .then(|| {
            elzevir_app::catalog::CatalogRefusal::BatchItems(vec![ItemRefusal {
                item: ItemId(7),
                label: "0001234".to_owned(),
                column: ItemColumn::Location,
                problem: RowProblem::NotInBranch,
            }])
        })
}

/// What the fake editor answers to an accepted edit.
fn output_of(edit: &elzevir_app::editing::Edit) -> elzevir_app::catalog::CatalogOutput {
    use elzevir_app::catalog::CatalogOutput;
    use elzevir_app::editing::Edit;
    match edit {
        Edit::SavedList(change) => {
            use elzevir_app::worklist::{SavedListChange, SavedListId};
            CatalogOutput::SavedListChanged(match change {
                SavedListChange::Create { .. } => SavedListId(9),
                SavedListChange::Add { list, .. }
                | SavedListChange::Remove { list, .. }
                | SavedListChange::Delete { list } => *list,
            })
        }
        Edit::Work(update) => CatalogOutput::WorkUpdated {
            work: update.work,
            row_version: 2,
        },
        Edit::Instance(update) => CatalogOutput::InstanceUpdated {
            instance: update.instance,
            row_version: 2,
        },
        Edit::NewRecord(_) => CatalogOutput::Record {
            work: WorkId(900),
            instance: InstanceId(901),
            items: Vec::new(),
        },
        Edit::Items(update) => CatalogOutput::ItemsSaved {
            instance: update.instance,
            row_version: 2,
            items: (500..)
                .zip(&update.rows)
                .map(|(new, row)| row.id.unwrap_or(elzevir_domain::catalog::ItemId(new)))
                .collect(),
        },
        Edit::BatchItems(change) => CatalogOutput::ItemsChanged {
            instances: change.instances.clone(),
            items: change.instances.len() * 2,
        },
        Edit::Registry(_) => CatalogOutput::RegistryChanged(None),
        Edit::Statements(edit) => CatalogOutput::StatementsSet {
            subject: edit.subject,
            row_version: 2,
        },
        Edit::Subject(update) => CatalogOutput::SubjectUpdated {
            subject: update.subject,
            row_version: 2,
        },
        Edit::Link(link) => CatalogOutput::WorkUpdated {
            work: link.work,
            row_version: 2,
        },
        Edit::Agent(update) => CatalogOutput::AgentUpdated {
            agent: update.agent,
            row_version: 2,
        },
        Edit::NewInstance(new) => CatalogOutput::InstanceAdded {
            work: new.work,
            instance: InstanceId(902),
        },
        Edit::Inventory(scan) => {
            CatalogOutput::Inventory(Box::new(elzevir_app::inventory::InventoryReport {
                read: scan.barcodes.len(),
                ..elzevir_app::inventory::InventoryReport::default()
            }))
        }
    }
}
