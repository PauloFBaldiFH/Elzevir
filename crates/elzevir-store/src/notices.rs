// SPDX-License-Identifier: GPL-3.0-or-later
//! Overdue notice rules and the queue of notices (docs/specs/circulacao/avisos.md).

use std::collections::{BTreeMap, HashMap};

use elzevir_app::circulation_rules::{CirculationOutput, OverdueNoticeRule};
use elzevir_app::commands::{WriteOutput, WriteRefusal};
use elzevir_domain::calendar::Calendar;
use elzevir_domain::circulation_rules::RuleScopeKey;
use elzevir_domain::notice::{
    NoticeLevel, NoticeSettings, OverdueLevel, Transport, Transports, checked_levels, days_overdue,
    sends_on, transports, triggered,
};
use jiff::Zoned;
use jiff::civil::Date;
use rusqlite::types::Type;
use rusqlite::{Connection, params};

use crate::calendar::read_calendar;
use crate::circulation_rules::scope_exists;
use crate::{CommandFailed, StoreError};

type Applied = Result<WriteOutput, CommandFailed<WriteRefusal>>;

fn storage(error: rusqlite::Error) -> CommandFailed<WriteRefusal> {
    StoreError::from(error).into()
}

/// Applies `circ.set_overdue_notice_rules`: the levels of the scope are replaced, and no
/// level removes the rule.
///
/// Koha: `tools/overduerules.pl`.
pub(crate) fn set_rule(connection: &Connection, rule: &OverdueNoticeRule) -> Applied {
    let levels = checked_levels(rule.levels.clone())
        .map_err(|error| CommandFailed::Rejected(WriteRefusal::Circulation(error.into())))?;
    scope_exists(
        connection,
        &RuleScopeKey {
            branch: rule.branch,
            category: Some(rule.category),
            policy: None,
        },
    )?;
    let branch = rule.branch.map(|id| id.0);
    connection
        .execute(
            "DELETE FROM overdue_notice_rules WHERE branch_id IS ?1 AND category_id = ?2",
            params![branch, rule.category.0],
        )
        .map_err(storage)?;
    for level in levels {
        connection
            .execute(
                "INSERT INTO overdue_notice_rules
                     (branch_id, category_id, level, delay, email, print)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    branch,
                    rule.category.0,
                    level.level.number(),
                    level.delay,
                    level.transports.email,
                    level.transports.print
                ],
            )
            .map_err(storage)?;
    }
    Ok(WriteOutput::Circulation(
        CirculationOutput::OverdueNoticeRuleSet,
    ))
}

/// The levels of every rule, by branch (`None` for any branch) and patron category.
type Rules = HashMap<(Option<i64>, i64), Vec<OverdueLevel>>;

fn read_rules(connection: &Connection) -> rusqlite::Result<Rules> {
    let mut statement = connection.prepare(
        "SELECT branch_id, category_id, level, delay, email, print
         FROM overdue_notice_rules ORDER BY level",
    )?;
    let rows = statement.query_map([], |row| {
        let level = NoticeLevel::from_number(row.get(2)?).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(2, Type::Integer, "no such level".into())
        })?;
        let scope = (row.get(0)?, row.get(1)?);
        let level = OverdueLevel {
            level,
            delay: row.get(3)?,
            transports: Transports {
                email: row.get(4)?,
                print: row.get(5)?,
            },
        };
        Ok((scope, level))
    })?;
    let mut rules = Rules::new();
    for row in rows {
        let (scope, level) = row?;
        rules.entry(scope).or_default().push(level);
    }
    Ok(rules)
}

/// The levels for a patron category at a branch: the branch's rules, or the rules of any
/// branch when the branch has none for any category.
///
/// Koha: `overdue_notices.pl` falls back on the default rules only when the branch has no
/// rule at all.
fn levels_for(rules: &Rules, branch: i64, category: i64) -> Option<&Vec<OverdueLevel>> {
    let branch_has_rules = rules.keys().any(|(id, _)| *id == Some(branch));
    let scope = if branch_has_rules { Some(branch) } else { None };
    rules.get(&(scope, category))
}

/// A loan out whose Item is not lost.
struct LoanOut {
    patron: i64,
    branch: i64,
    category: i64,
    email: Option<String>,
    item: i64,
    due: Zoned,
}

fn loans_out(connection: &Connection) -> rusqlite::Result<Vec<LoanOut>> {
    let mut statement = connection.prepare(
        "SELECT l.patron_id, l.branch_id, p.category_id, p.email, l.item_id, l.due
         FROM loans l JOIN items i ON i.id = l.item_id JOIN patrons p ON p.id = l.patron_id
         WHERE i.lost = 0
         ORDER BY l.patron_id, l.branch_id, l.id",
    )?;
    statement
        .query_map([], |row| {
            let due: String = row.get(5)?;
            let due = due.parse().map_err(|error: jiff::Error| {
                rusqlite::Error::FromSqlConversionFailure(5, Type::Text, error.into())
            })?;
            Ok(LoanOut {
                patron: row.get(0)?,
                branch: row.get(1)?,
                category: row.get(2)?,
                email: row.get(3)?,
                item: row.get(4)?,
                due,
            })
        })?
        .collect()
}

/// The Items of one notice, with their due dates, by patron, branch and level.
type Due = BTreeMap<(i64, i64, NoticeLevel), Vec<(i64, String)>>;

/// Applies `circ.queue_overdue_notices`: every patron with a loan exactly as many days
/// overdue as a level of the rule for the patron's category at the loan's branch gets that
/// level's notices, listing the loans at that branch as late; a notice already queued today
/// is not queued again.
///
/// Koha: `misc/cronjobs/overdue_notices.pl -t`, with `OverdueNoticeFrom` `cron`
/// (`issues.branchcode`).
pub(crate) fn queue_overdue_notices(connection: &Connection, at: &Zoned) -> Applied {
    let settings = NoticeSettings::default();
    let today = at.date();
    let rules = read_rules(connection).map_err(storage)?;
    let loans = loans_out(connection).map_err(storage)?;
    let mut calendars: HashMap<i64, Calendar> = HashMap::new();
    let mut due = Due::new();
    let mut patrons: HashMap<i64, (i64, Option<String>)> = HashMap::new();
    for loan in loans {
        let Some(levels) = levels_for(&rules, loan.branch, loan.category) else {
            continue;
        };
        let calendar = match calendars.entry(loan.branch) {
            std::collections::hash_map::Entry::Occupied(found) => found.into_mut(),
            std::collections::hash_map::Entry::Vacant(empty) => {
                empty.insert(read_calendar(connection, loan.branch).map_err(storage)?)
            }
        };
        if !sends_on(today, calendar, settings) {
            continue;
        }
        let Some(level) = days_overdue(loan.due.date(), today, calendar, settings)
            .and_then(|days| triggered(levels, days))
        else {
            continue;
        };
        patrons.insert(loan.patron, (loan.category, loan.email));
        due.entry((loan.patron, loan.branch, level.level))
            .or_default()
            .push((loan.item, loan.due.to_string()));
    }
    let mut queued = 0;
    for ((patron, branch, level), items) in &due {
        let Some((category, email)) = patrons.get(patron) else {
            continue;
        };
        let email = email
            .as_deref()
            .map(str::trim)
            .filter(|email| !email.is_empty());
        let Some(wanted) = levels_for(&rules, *branch, *category)
            .and_then(|levels| levels.iter().find(|found| found.level == *level))
        else {
            continue;
        };
        for transport in transports(wanted.transports, email.is_some()) {
            let address = (transport == Transport::Email).then_some(email).flatten();
            let notice = Notice {
                patron: *patron,
                branch: *branch,
                level: *level,
                transport: transport.as_str(),
                address,
            };
            if enqueue(connection, &notice, today, &at.to_string(), items).map_err(storage)? {
                queued += 1;
            }
        }
    }
    Ok(WriteOutput::Circulation(
        CirculationOutput::OverdueNoticesQueued { queued },
    ))
}

/// One notice to queue.
struct Notice<'a> {
    patron: i64,
    branch: i64,
    level: NoticeLevel,
    transport: &'static str,
    address: Option<&'a str>,
}

/// Queues the notice with its Items, unless it is already queued today; whether it was.
fn enqueue(
    connection: &Connection,
    notice: &Notice<'_>,
    today: Date,
    created_at: &str,
    items: &[(i64, String)],
) -> rusqlite::Result<bool> {
    let inserted = connection.execute(
        "INSERT OR IGNORE INTO notices
             (patron_id, kind, level, branch_id, transport, to_address, day, created_at)
         VALUES (?1, 'OVERDUE', ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            notice.patron,
            notice.level.number(),
            notice.branch,
            notice.transport,
            notice.address,
            today.to_string(),
            created_at
        ],
    )?;
    if inserted == 0 {
        return Ok(false);
    }
    let id = connection.last_insert_rowid();
    for (item, due) in items {
        connection.execute(
            "INSERT INTO notice_items (notice_id, item_id, due) VALUES (?1, ?2, ?3)",
            params![id, item, due],
        )?;
    }
    Ok(true)
}
