// SPDX-License-Identifier: GPL-3.0-or-later
//! A store holding what the Koha oracle held for a recording, shared by the circulation
//! replays.
//!
//! The recordings name Koha's codes; here they name the test's branches and policies at
//! the MARC edge, as an import would. No Koha code reaches the domain.

use std::collections::{BTreeMap, BTreeSet};

use elzevir_app::catalog::{
    CatalogCommand, CatalogOutput, CatalogReader, ItemData, ItemStatus, NewInstance, NewItem,
    NewRecord, RecordItems,
};
use elzevir_app::circulation_rules::{CirculationCommand, RuleChange};
use elzevir_app::codes::{CodeTable, NamedCode};
use elzevir_app::commands::{WriteCommand, WriteOutput, WriteRefusal};
use elzevir_app::patrons::{
    CategoryChange, PatronCommand, PatronFields, PatronOutput, ReadingHistory,
};
use elzevir_domain::catalog::{BranchId, InstanceId, PolicyId};
use elzevir_domain::circulation_rules::RuleScopeKey;
use elzevir_domain::patron::{Enrolment, PatronCategoryId, PatronId};
use elzevir_store::{CommandFailed, ElzevirHandler, ElzevirStore, StoreConfig};
use jiff::civil::date;
use serde::Deserialize;

/// Sends one command and waits for it.
pub(crate) fn submit(
    store: &ElzevirStore,
    command: WriteCommand,
) -> Result<WriteOutput, CommandFailed<WriteRefusal>> {
    store.submit(command).unwrap().wait()
}

/// The rules of one context, as the oracle listed them.
#[derive(Deserialize)]
pub(crate) struct RecordedRules {
    pub(crate) context: Context,
    pub(crate) rules: BTreeMap<String, String>,
}

/// Koha's rule context, `*` for any.
#[derive(Deserialize)]
pub(crate) struct Context {
    #[serde(rename = "library_id")]
    pub(crate) library: String,
    #[serde(rename = "patron_category_id")]
    pub(crate) category: String,
    #[serde(rename = "item_type_id")]
    pub(crate) item_type: String,
}

/// A store with the oracle's branches, policies, one patron category and one patron.
pub(crate) struct Library {
    pub(crate) store: ElzevirStore,
    pub(crate) branches: BTreeMap<String, BranchId>,
    pub(crate) policies: BTreeMap<String, PolicyId>,
    pub(crate) category: PatronCategoryId,
    pub(crate) patron: PatronId,
    instance: InstanceId,
}

impl Library {
    /// `codes` are `(is_branch, code)`; the patron is enrolled at `patron_library`.
    pub(crate) fn new(
        dir: &tempfile::TempDir,
        codes: &BTreeSet<(bool, String)>,
        patron_library: &str,
        category: &str,
    ) -> Self {
        let store = ElzevirStore::open(
            &dir.path().join("elzevir.db"),
            &StoreConfig::default(),
            ElzevirHandler,
        )
        .unwrap();
        for (is_branch, code) in codes {
            let table = if *is_branch {
                CodeTable::Branch
            } else {
                CodeTable::ItemType
            };
            submit(
                &store,
                WriteCommand::Catalog(CatalogCommand::NameCodes(vec![NamedCode {
                    table,
                    name: code.clone(),
                    code: code.clone(),
                }])),
            )
            .unwrap();
        }
        let lists = store.codes().unwrap();
        let branches = lists
            .branches
            .iter()
            .map(|branch| (branch.name.clone(), branch.id))
            .collect::<BTreeMap<_, _>>();
        let policies = lists
            .policies
            .iter()
            .map(|policy| (policy.name.clone(), policy.id))
            .collect();
        let Ok(WriteOutput::Patrons(PatronOutput::CategoryChanged(Some(category)))) = submit(
            &store,
            WriteCommand::Patrons(PatronCommand::ChangeCategory(CategoryChange::Create {
                name: category.to_owned(),
                enrolment: Enrolment::Months(12),
            })),
        ) else {
            panic!("no category");
        };
        let patron = add_patron(&store, branches[patron_library], category);
        let instance = add_record(&store);
        Self {
            store,
            branches,
            policies,
            category,
            patron,
            instance,
        }
    }

    /// The scope of a Koha context.
    pub(crate) fn scope<'a>(&self, context: &'a Context) -> RuleScopeKey {
        let any = |code: &'a str| (code != "*").then_some(code);
        RuleScopeKey {
            branch: any(&context.library).map(|code| self.branches[code]),
            category: any(&context.category).map(|_| self.category),
            policy: any(&context.item_type).map(|code| self.policies[code]),
        }
    }

    /// Sets every recorded rule.
    pub(crate) fn set_rules(&self, recorded: &[RecordedRules]) -> Result<(), String> {
        for rules in recorded {
            let values = rules
                .rules
                .iter()
                .map(|(name, value)| (name.clone(), Some(value.clone())))
                .collect();
            submit(
                &self.store,
                WriteCommand::Circulation(CirculationCommand::SetRules(RuleChange {
                    scope: self.scope(&rules.context),
                    values,
                })),
            )
            .map_err(|error| format!("rules {:?}: {error:?}", rules.rules))?;
        }
        Ok(())
    }

    /// Adds an Item of the record.
    pub(crate) fn add_item(
        &self,
        barcode: &str,
        branches: (&str, &str),
        item_type: &str,
        status: ItemStatus,
    ) {
        self.add_item_to(self.instance, barcode, branches, item_type, status);
    }

    /// Adds an Item of `instance`.
    pub(crate) fn add_item_to(
        &self,
        instance: InstanceId,
        barcode: &str,
        (home, holding): (&str, &str),
        item_type: &str,
        status: ItemStatus,
    ) {
        submit(
            &self.store,
            WriteCommand::Catalog(CatalogCommand::AddItem(Box::new(NewItem {
                instance,
                item: ItemData {
                    issue_statement: None,
                    enumeration: None,
                    chronology: None,
                    barcode: Some(barcode.to_owned()),
                    inventory_number: None,
                    home_branch: home.to_owned(),
                    holding_branch: holding.to_owned(),
                    item_type: item_type.to_owned(),
                    call_number: None,
                    location: None,
                    collection: None,
                    price: None,
                    replacement_price: None,
                    copy_number: None,
                    status,
                    acquired_on: None,
                },
            }))),
        )
        .unwrap();
    }
}

fn add_patron(store: &ElzevirStore, branch: BranchId, category: PatronCategoryId) -> PatronId {
    let Ok(WriteOutput::Patrons(PatronOutput::Added(patron))) = submit(
        store,
        WriteCommand::Patrons(PatronCommand::Add(Box::new(PatronFields {
            cardnumber: "000001".to_owned(),
            surname: "Leitora".to_owned(),
            first_name: None,
            cpf: None,
            date_of_birth: None,
            email: None,
            phone: None,
            address: None,
            branch,
            category,
            enrolled: date(2026, 1, 1),
            expires: None,
            staff_note: None,
            reading_history: ReadingHistory::NotKept,
            extra: Vec::new(),
        }))),
    ) else {
        panic!("no patron");
    };
    patron
}

fn add_record(store: &ElzevirStore) -> InstanceId {
    let Ok(WriteOutput::Catalog(CatalogOutput::Record { instance, .. })) = submit(
        store,
        WriteCommand::Catalog(CatalogCommand::AddRecord(Box::new(NewRecord {
            work_title: "Obra".to_owned(),
            work_description: elzevir_domain::statement::Description::default(),
            hub: None,
            relations: Vec::new(),
            classification: None,
            classification_scheme: None,
            summary: None,
            audience: None,
            languages: Vec::new(),
            contributions: Vec::new(),
            subjects: Vec::new(),
            items: RecordItems::default(),
            source: None,
            instance: NewInstance {
                title: "Obra".to_owned(),
                ..NewInstance::default()
            },
        }))),
    ) else {
        panic!("no record");
    };
    instance
}
