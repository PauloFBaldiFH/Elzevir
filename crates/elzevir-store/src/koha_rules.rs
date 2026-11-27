// SPDX-License-Identifier: GPL-3.0-or-later
//! `patrons.migrate_koha_rules`: Koha's circulation rules, each set as `circ.set_rules`
//! sets it, with its scope found by Koha's codes (docs/specs/migracao/troca-do-koha.md,
//! item 7a).

use elzevir_app::circulation_rules::RuleChange;
use elzevir_app::commands::WriteRefusal;
use elzevir_app::koha_migration::{KohaRule, KohaRulesBatch, KohaRulesMigrated, RuleLeftover};
use elzevir_app::patrons::PatronOutput;
use elzevir_domain::catalog::{BranchId, PolicyId};
use elzevir_domain::circulation_rules::RuleScopeKey;
use elzevir_domain::patron::PatronCategoryId;
use rusqlite::Connection;

use crate::koha_migration::{Keyed, key};
use crate::places::{Coded, by_code};
use crate::{CommandFailed, StoreError};

/// Sets every rule whose scope the Elzevir has; a rule it cannot take is left out alone.
pub(crate) fn apply(
    connection: &Connection,
    batch: &KohaRulesBatch,
) -> Result<PatronOutput, CommandFailed<WriteRefusal>> {
    let mut done = KohaRulesMigrated::default();
    for rule in &batch.rules {
        let scope = match scope(connection, rule)? {
            Ok(scope) => scope,
            Err(why) => {
                done.left.push((rule.id, why));
                continue;
            }
        };
        let change = RuleChange {
            scope,
            values: vec![(rule.name.clone(), Some(rule.value.clone()))],
        };
        match crate::circulation_rules::set_rules(connection, &change) {
            Ok(_) => done.rules += 1,
            // The plan already checked the rule; a refusal here is the writer's own.
            Err(CommandFailed::Rejected(WriteRefusal::Circulation(_))) => {
                done.left.push((rule.id, RuleLeftover::BadValue));
            }
            Err(other) => return Err(other),
        }
    }
    Ok(PatronOutput::KohaRulesMigrated(done))
}

fn scope(
    connection: &Connection,
    rule: &KohaRule,
) -> Result<Result<RuleScopeKey, RuleLeftover>, StoreError> {
    let branch = match &rule.branch {
        None => None,
        Some(code) => match by_code(connection, Coded::Branch, code)? {
            Some(id) => Some(BranchId(id)),
            None => return Ok(Err(RuleLeftover::UnknownBranch)),
        },
    };
    let category = match &rule.category {
        None => None,
        Some(code) => match key(connection, Keyed::Category, code)? {
            Some(id) => Some(PatronCategoryId(id)),
            None => return Ok(Err(RuleLeftover::UnknownCategory)),
        },
    };
    let policy = match &rule.item_type {
        None => None,
        Some(code) => match by_code(connection, Coded::Policy, code)? {
            Some(id) => Some(PolicyId(id)),
            None => return Ok(Err(RuleLeftover::UnknownItemType)),
        },
    };
    Ok(Ok(RuleScopeKey {
        branch,
        category,
        policy,
    }))
}
