// SPDX-License-Identifier: GPL-3.0-or-later
//! `circ.set_rules` and the rule book (docs/specs/circulacao/regras.md).

use elzevir_app::circulation_rules::{
    CirculationCommand, CirculationOutput, CirculationRefusal, RuleBook, RuleBookError, RuleChange,
    RuleKind, checked_rules,
};
use elzevir_app::commands::{WriteOutput, WriteRefusal};
use elzevir_domain::catalog::{BranchId, PolicyId};
use elzevir_domain::circulation_rules::{CirculationRule, RuleScopeKey};
use elzevir_domain::patron::PatronCategoryId;
use rusqlite::{Connection, params};

use crate::catalog::exists;
use crate::{CommandFailed, ElzevirStore, StoreError, WriteTx};

type Applied = Result<WriteOutput, CommandFailed<WriteRefusal>>;

const fn refused(refusal: CirculationRefusal) -> CommandFailed<WriteRefusal> {
    CommandFailed::Rejected(WriteRefusal::Circulation(refusal))
}

/// Applies one circulation command inside the writer's transaction.
pub(crate) fn apply(tx: &WriteTx<'_>, command: &CirculationCommand) -> Applied {
    match command {
        CirculationCommand::SetRules(change) => set_rules(tx.connection(), change),
    }
}

/// Confirms that every id of the scope exists.
fn scope_exists(
    connection: &Connection,
    scope: &RuleScopeKey,
) -> Result<(), CommandFailed<WriteRefusal>> {
    let checks = [
        (
            scope.branch.map(|id| id.0),
            "SELECT 1 FROM branches WHERE id = ?1",
            CirculationRefusal::UnknownBranch,
        ),
        (
            scope.category.map(|id| id.0),
            "SELECT 1 FROM patron_categories WHERE id = ?1",
            CirculationRefusal::UnknownCategory,
        ),
        (
            scope.policy.map(|id| id.0),
            "SELECT 1 FROM circulation_policies WHERE id = ?1",
            CirculationRefusal::UnknownPolicy,
        ),
    ];
    for (id, sql, refusal) in checks {
        if let Some(id) = id
            && !exists(connection, sql, id)?
        {
            return Err(refused(refusal));
        }
    }
    Ok(())
}

fn set_rules(connection: &Connection, change: &RuleChange) -> Applied {
    let rules = checked_rules(change).map_err(refused)?;
    scope_exists(connection, &change.scope)?;
    let scope = &change.scope;
    let (branch, category, policy) = (
        scope.branch.map(|id| id.0),
        scope.category.map(|id| id.0),
        scope.policy.map(|id| id.0),
    );
    let (mut stored, mut removed) = (0, 0);
    for (kind, value) in rules {
        // `IS` matches "any" (NULL) as well as an id.
        let deleted = connection
            .execute(
                "DELETE FROM circulation_rules
                 WHERE kind = ?1 AND branch_id IS ?2 AND category_id IS ?3 AND policy_id IS ?4",
                params![kind.koha_name(), branch, category, policy],
            )
            .map_err(StoreError::from)?;
        match value {
            Some(value) => {
                connection
                    .execute(
                        "INSERT INTO circulation_rules (kind, branch_id, category_id, policy_id, value)
                         VALUES (?1, ?2, ?3, ?4, ?5)",
                        params![kind.koha_name(), branch, category, policy, value],
                    )
                    .map_err(StoreError::from)?;
                stored += 1;
            }
            None => removed += deleted,
        }
    }
    Ok(WriteOutput::Circulation(CirculationOutput::RulesSet {
        stored,
        removed,
    }))
}

impl RuleBook for ElzevirStore {
    fn rules(&self) -> Result<Vec<CirculationRule>, RuleBookError> {
        self.readers()
            .read(|c| {
                let mut statement = c.prepare_cached(
                    "SELECT kind, branch_id, category_id, policy_id, value FROM circulation_rules
                     ORDER BY kind, branch_id, category_id, policy_id",
                )?;
                let rows = statement.query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        RuleScopeKey {
                            branch: row.get::<_, Option<i64>>(1)?.map(BranchId),
                            category: row.get::<_, Option<i64>>(2)?.map(PatronCategoryId),
                            policy: row.get::<_, Option<i64>>(3)?.map(PolicyId),
                        },
                        row.get::<_, String>(4)?,
                    ))
                })?;
                rows.collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(|error| RuleBookError(error.to_string()))?
            .into_iter()
            .map(|(name, scope, value)| {
                let kind = RuleKind::from_koha_name(&name)
                    .ok_or_else(|| RuleBookError(format!("unknown stored rule `{name}`")))?;
                Ok(CirculationRule { kind, scope, value })
            })
            .collect()
    }
}
