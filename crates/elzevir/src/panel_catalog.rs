// SPDX-License-Identifier: GPL-3.0-or-later
//! Adapts the OS-private administrative channel to the catalog's existing ports.

use std::sync::Arc;

use elzevir_app::{
    catalog::{
        CatalogCommand, CatalogReader, InstanceChange, InstanceUpdate, ShelfSignal, WorkChange,
    },
    commands::WriteCommand,
    registries::{RegistryChange, RegistryEntry, RegistryList},
};
use elzevir_domain::catalog::{InstanceId, PolicyId};
use elzevir_panel::{
    CatalogAction, CatalogItem, CatalogItemStatus, CatalogList, CatalogPage, CatalogRow,
};
use elzevir_search::TantivyIndex;
use elzevir_store::ElzevirStore;

#[derive(Clone)]
pub(crate) struct Catalog {
    pub(crate) store: Arc<ElzevirStore>,
    pub(crate) index: Arc<TantivyIndex>,
}

impl Catalog {
    pub(crate) fn execute(&self, action: CatalogAction) -> Result<CatalogPage, String> {
        let (command, policies) = match action {
            CatalogAction::Query(query) => return self.records(&query),
            CatalogAction::Policies => return self.policies(),
            CatalogAction::Create(title) => {
                let record =
                    elzevir_app::creating::new_record(vec![WorkChange::Title(title)], vec![])
                        .map_err(|_| "record validation failed")?;
                (CatalogCommand::AddRecord(Box::new(record)), false)
            }
            CatalogAction::Edit { id, version, title } => (
                CatalogCommand::UpdateInstance(Box::new(InstanceUpdate {
                    instance: InstanceId(id),
                    row_version: version,
                    changes: vec![InstanceChange::Title(title)],
                })),
                false,
            ),
            CatalogAction::CreatePolicy(name) => (
                CatalogCommand::ChangeRegistry(RegistryChange::Create {
                    list: RegistryList::Policies,
                    name,
                }),
                true,
            ),
            CatalogAction::RenamePolicy { id, name } => (
                CatalogCommand::ChangeRegistry(RegistryChange::Rename {
                    entry: RegistryEntry::Policy(PolicyId(id)),
                    name,
                }),
                true,
            ),
            CatalogAction::DeletePolicy(id) => (
                CatalogCommand::ChangeRegistry(RegistryChange::Delete(RegistryEntry::Policy(
                    PolicyId(id),
                ))),
                true,
            ),
        };
        self.store
            .submit(WriteCommand::Catalog(command))
            .map_err(|error| error.to_string())?
            .wait()
            .map_err(|error| error.to_string())?;
        if policies {
            self.policies()
        } else {
            self.records("")
        }
    }

    fn policies(&self) -> Result<CatalogPage, String> {
        let codes = self.store.codes().map_err(|error| error.to_string())?;
        Ok(CatalogPage {
            list: CatalogList::Policies,
            query: String::new(),
            rows: codes
                .policies
                .into_iter()
                .take(20)
                .map(|policy| CatalogRow {
                    id: policy.id.0,
                    version: 0,
                    title: crate::panel_text(&policy.name, 256),
                    hub: None,
                    work: None,
                    total_items: 0,
                    items: vec![],
                })
                .collect(),
        })
    }

    fn records(&self, query: &str) -> Result<CatalogPage, String> {
        let rows = if query.trim().is_empty() {
            elzevir_app::listing::browse(self.store.as_ref(), None)
                .map_err(|error| error.to_string())?
                .rows
        } else {
            elzevir_app::listing::search(self.index.as_ref(), self.store.as_ref(), query, &[], 0)
                .map_err(|error| error.to_string())?
                .rows
        };
        Ok(CatalogPage {
            list: CatalogList::Records,
            query: crate::panel_text(query, 512),
            rows: rows
                .into_iter()
                .flat_map(|family| {
                    family.works.into_iter().flat_map(move |work| {
                        let hub = family.hub_title.clone();
                        work.instances.into_iter().map(move |instance| CatalogRow {
                            id: instance.id.0,
                            title: crate::panel_text(&instance.title, 512),
                            version: instance.version,
                            hub: hub.as_deref().map(|name| crate::panel_text(name, 160)),
                            work: Some(crate::panel_text(&work.title, 160)),
                            total_items: instance.items,
                            items: instance
                                .pills
                                .into_iter()
                                .take(5)
                                .map(|item| CatalogItem {
                                    label: item.inventory_number.or(item.call_number).map_or_else(
                                        || item.id.0.to_string(),
                                        |name| crate::panel_text(&name, 80),
                                    ),
                                    status: match item.signal {
                                        ShelfSignal::Available => CatalogItemStatus::Available,
                                        ShelfSignal::NotForLoan => CatalogItemStatus::InLibrary,
                                        ShelfSignal::OutOfCirculation => {
                                            CatalogItemStatus::Unavailable
                                        }
                                    },
                                })
                                .collect(),
                        })
                    })
                })
                .take(10)
                .collect(),
        })
    }
}
