// SPDX-License-Identifier: GPL-3.0-or-later
//! Who changed a Work or an Instance, what and when (docs/specs/catalogo/historico.md),
//! read from the `audit_log`: each command names the entities it touched, and its JSON
//! says which parts it changed.

use elzevir_domain::statement::{EntityKind, EntityRef};
use serde_json::Value as Json;

use crate::catalog::{CatalogCommand, CatalogOutput, CatalogReadError, ImportOutcome};
use crate::commands::WriteCommand;

/// Most entries a history shows; older ones are left out.
pub const HISTORY_LIMIT: usize = 100;

/// The entities a catalog command touched, given what it returned: the ones whose
/// history it belongs to.
#[must_use]
pub fn touched(command: &CatalogCommand, output: &CatalogOutput) -> Vec<EntityRef> {
    let work = |id: i64| EntityRef {
        entity: EntityKind::Work,
        id,
    };
    let instance = |id: i64| EntityRef {
        entity: EntityKind::Instance,
        id,
    };
    match output {
        CatalogOutput::Record {
            work: w,
            instance: i,
            ..
        }
        | CatalogOutput::InstanceAdded {
            work: w,
            instance: i,
        } => vec![work(w.0), instance(i.0)],
        CatalogOutput::Imported(
            ImportOutcome::Created { instance: i, .. }
            | ImportOutcome::Replaced { instance: i, .. },
        )
        | CatalogOutput::Replaced(i)
        | CatalogOutput::InstanceUpdated { instance: i, .. }
        | CatalogOutput::ItemsSaved { instance: i, .. } => vec![instance(i.0)],
        CatalogOutput::WorkUpdated { work: w, .. } => vec![work(w.0)],
        CatalogOutput::StatementsSet { subject, .. } => vec![*subject],
        CatalogOutput::Item(_) => match command {
            CatalogCommand::AddItem(new) => vec![instance(new.instance.0)],
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

/// One entry of the `audit_log` that touched an entity, as the store reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoggedCommand {
    /// When it was applied, `YYYY-MM-DDTHH:MM:SSZ` in UTC; `None` before the log kept it.
    pub at: Option<String>,
    /// The login of the staff account it came from; `None` for imports and the command
    /// line, or when the account is gone.
    pub actor: Option<String>,
    /// The command, as the log stores it.
    pub command: String,
}

/// Port: the commands that touched an entity, newest first. The store implements it.
pub trait HistoryReader: Send + Sync {
    /// Up to `limit` commands that touched `entity`, newest first.
    ///
    /// # Errors
    ///
    /// When storage fails.
    fn history(
        &self,
        entity: EntityRef,
        limit: usize,
    ) -> Result<Vec<LoggedCommand>, CatalogReadError>;
}

/// A part of a Work or an Instance that a change named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Part {
    /// A part of the form, by its change name, such as `Title` or `Subjects`.
    Field(String),
    /// The statements of a BIBFRAME property, by its local name.
    Property(String),
}

/// What a logged command did to the entity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// It created the record or the Instance, from the screen.
    Created,
    /// It imported the record from a file, or replaced its description from one.
    Imported,
    /// It edited these parts, in the order of the edit.
    Edited(Vec<Part>),
    /// It saved this many rows of the Items grid, or added one Item.
    Items(usize),
    /// Something this build does not describe.
    Other,
}

/// One line of a history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    /// When, as the log keeps it.
    pub at: Option<String>,
    /// Who, as the log keeps it.
    pub actor: Option<String>,
    /// What.
    pub change: Change,
}

/// The history line of a logged command.
#[must_use]
pub fn entry(logged: LoggedCommand) -> HistoryEntry {
    let change = serde_json::from_str::<WriteCommand>(&logged.command)
        .map_or(Change::Other, |command| change_of(&command));
    HistoryEntry {
        at: logged.at,
        actor: logged.actor,
        change,
    }
}

fn change_of(command: &WriteCommand) -> Change {
    let WriteCommand::Catalog(command) = command else {
        return Change::Other;
    };
    match command {
        // A record with its source record came from a file.
        CatalogCommand::AddRecord(record) if record.source.is_some() => Change::Imported,
        CatalogCommand::AddRecord(_) | CatalogCommand::AddInstance(_) => Change::Created,
        CatalogCommand::ImportRecord(_) | CatalogCommand::ReplaceRecord { .. } => Change::Imported,
        CatalogCommand::UpdateWork(update) => {
            Change::Edited(update.changes.iter().map(part).collect())
        }
        CatalogCommand::UpdateInstance(update) => {
            Change::Edited(update.changes.iter().map(part).collect())
        }
        CatalogCommand::EditStatements(edit) => Change::Edited(
            edit.changes
                .iter()
                .map(|change| Part::Property(change.property.clone()))
                .collect(),
        ),
        CatalogCommand::SetStatements(_) => Change::Edited(Vec::new()),
        CatalogCommand::SaveItems(update) => Change::Items(update.rows.len()),
        CatalogCommand::AddItem(_) => Change::Items(1),
        _ => Change::Other,
    }
}

/// The part a Work or Instance change names: its variant, or its property for the
/// statements of one property. Read from the change's own JSON, so a new variant needs
/// no line here.
fn part(change: &impl serde::Serialize) -> Part {
    match serde_json::to_value(change) {
        Ok(Json::String(name)) => Part::Field(name),
        Ok(Json::Object(map)) => match map.iter().next() {
            Some((name, value)) if name == "Statements" => {
                value.get("property").and_then(Json::as_str).map_or_else(
                    || Part::Field(name.clone()),
                    |p| Part::Property(p.to_owned()),
                )
            }
            Some((name, _)) => Part::Field(name.clone()),
            None => Part::Field(String::new()),
        },
        _ => Part::Field(String::new()),
    }
}

#[cfg(test)]
mod tests {
    use elzevir_domain::catalog::{InstanceId, ItemId, WorkId};
    use elzevir_domain::statement::Statement;

    use super::*;
    use crate::catalog::{InstanceChange, InstanceUpdate, WorkChange, WorkUpdate};
    use crate::items::ItemsUpdate;

    fn logged(command: &WriteCommand) -> LoggedCommand {
        LoggedCommand {
            at: Some("2026-11-16T15:00:00Z".to_owned()),
            actor: Some("ana".to_owned()),
            command: serde_json::to_string(command).unwrap(),
        }
    }

    #[test]
    fn an_edit_names_its_parts_and_properties() {
        let edit = WriteCommand::Catalog(CatalogCommand::UpdateWork(Box::new(WorkUpdate {
            work: WorkId(3),
            row_version: 1,
            changes: vec![
                WorkChange::Title("Dom Casmurro".to_owned()),
                WorkChange::Summary(None),
                WorkChange::Statements {
                    property: "genreForm".to_owned(),
                    statements: Vec::<Statement>::new(),
                },
            ],
        })));
        assert_eq!(
            entry(logged(&edit)),
            HistoryEntry {
                at: Some("2026-11-16T15:00:00Z".to_owned()),
                actor: Some("ana".to_owned()),
                change: Change::Edited(vec![
                    Part::Field("Title".to_owned()),
                    Part::Field("Summary".to_owned()),
                    Part::Property("genreForm".to_owned()),
                ]),
            }
        );
        let instance =
            WriteCommand::Catalog(CatalogCommand::UpdateInstance(Box::new(InstanceUpdate {
                instance: InstanceId(7),
                row_version: 1,
                changes: vec![InstanceChange::Edition(Some("2. ed.".to_owned()))],
            })));
        assert_eq!(
            entry(logged(&instance)).change,
            Change::Edited(vec![Part::Field("Edition".to_owned())])
        );
        let items = WriteCommand::Catalog(CatalogCommand::SaveItems(Box::new(ItemsUpdate {
            instance: InstanceId(7),
            row_version: 1,
            rows: Vec::new(),
        })));
        assert_eq!(entry(logged(&items)).change, Change::Items(0));
        let unknown = LoggedCommand {
            command: r#"{"Catalog":{"Future":{}}}"#.to_owned(),
            ..logged(&items)
        };
        assert_eq!(entry(unknown).change, Change::Other);
    }

    #[test]
    fn a_command_touches_the_entities_it_returns() {
        let items = CatalogCommand::SaveItems(Box::new(ItemsUpdate {
            instance: InstanceId(7),
            row_version: 1,
            rows: Vec::new(),
        }));
        let saved = CatalogOutput::ItemsSaved {
            instance: InstanceId(7),
            row_version: 2,
            items: vec![ItemId(1)],
        };
        assert_eq!(
            touched(&items, &saved),
            [EntityRef {
                entity: EntityKind::Instance,
                id: 7
            }]
        );
        let added = CatalogOutput::Record {
            work: WorkId(3),
            instance: InstanceId(7),
            items: Vec::new(),
        };
        assert_eq!(touched(&items, &added).len(), 2);
    }
}
