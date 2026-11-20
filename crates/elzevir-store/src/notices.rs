// SPDX-License-Identifier: GPL-3.0-or-later
//! Overdue notice rules, the queue of notices and what the sender reads from it
//! (docs/specs/circulacao/avisos.md).

use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap};

use elzevir_app::circulation_rules::{CirculationOutput, NoticeRules, OverdueNoticeRule};
use elzevir_app::commands::{WriteOutput, WriteRefusal};
use elzevir_app::notices::{
    NoticeAbout, NoticeId, NoticeItem, NoticeOutcome, NoticeQueue, NoticeQueueError, PendingEmail,
};
use elzevir_domain::calendar::Calendar;
use elzevir_domain::catalog::BranchId;
use elzevir_domain::circulation_rules::RuleScopeKey;
use elzevir_domain::notice::{
    NoticeLevel, NoticeSettings, OverdueLevel, Transport, Transports, checked_levels, days_overdue,
    sends_on, transports, triggered,
};
use elzevir_domain::patron::{PatronCategoryId, PatronId};
use jiff::Zoned;
use jiff::civil::Date;
use rusqlite::types::Type;
use rusqlite::{Connection, OptionalExtension, params};

use crate::calendar::read_calendar;
use crate::circulation_rules::scope_exists;
use crate::restrictions::restrict_for_overdues;
use crate::{CommandFailed, ElzevirStore, StoreError};

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
                     (branch_id, category_id, level, delay, email, print, restricts)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    branch,
                    rule.category.0,
                    level.level.number(),
                    level.delay,
                    level.transports.email,
                    level.transports.print,
                    level.restrict
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
        "SELECT branch_id, category_id, level, delay, email, print, restricts
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
            restrict: row.get(6)?,
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

/// Every overdue notice rule, "any branch" first, and the hold notice preferences, for the
/// notices screen.
pub(crate) fn notice_rules(connection: &Connection) -> rusqlite::Result<NoticeRules> {
    let mut overdue: Vec<OverdueNoticeRule> = read_rules(connection)?
        .into_iter()
        .map(|((branch, category), levels)| OverdueNoticeRule {
            branch: branch.map(BranchId),
            category: PatronCategoryId(category),
            levels,
        })
        .collect();
    overdue.sort_by_key(|rule| (rule.branch, rule.category));
    let mut statement = connection.prepare(
        "SELECT category_id, email, print FROM hold_notice_preferences ORDER BY category_id",
    )?;
    let holds = statement
        .query_map([], |row| {
            Ok((
                PatronCategoryId(row.get(0)?),
                Transports {
                    email: row.get(1)?,
                    print: row.get(2)?,
                },
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(NoticeRules { overdue, holds })
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

/// What one patron owes notices for: the category and the e-mail address.
type Patrons = HashMap<i64, (i64, Option<String>)>;

/// The loans of each patron at each branch whose days overdue are exactly a level's.
fn due_today(
    connection: &Connection,
    rules: &Rules,
    today: Date,
    settings: NoticeSettings,
) -> rusqlite::Result<(Due, Patrons)> {
    let mut calendars: HashMap<i64, Calendar> = HashMap::new();
    let mut due = Due::new();
    let mut patrons = Patrons::new();
    for loan in loans_out(connection)? {
        let Some(levels) = levels_for(rules, loan.branch, loan.category) else {
            continue;
        };
        let calendar = match calendars.entry(loan.branch) {
            Entry::Occupied(found) => found.into_mut(),
            Entry::Vacant(empty) => empty.insert(read_calendar(connection, loan.branch)?),
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
    Ok((due, patrons))
}

/// Records that the daily run of `name` went through `today`; `false` when it already had.
///
/// Koha runs `overdue_notices.pl` once a day from cron; the server runs every hour, so a
/// restriction the staff lift during the day is not put back by the next run.
fn first_run_today(connection: &Connection, name: &str, today: Date) -> rusqlite::Result<bool> {
    let changed = connection.execute(
        "INSERT INTO daily_runs (name, day) VALUES (?1, ?2)
         ON CONFLICT (name) DO UPDATE SET day = excluded.day WHERE day <> excluded.day",
        params![name, today.to_string()],
    )?;
    Ok(changed > 0)
}

/// Applies `circ.queue_overdue_notices`, once a day: every patron with a loan exactly as
/// many days overdue as a level of the rule for the patron's category at the loan's branch
/// gets that level's notices, listing the loans at that branch as late, and is restricted
/// when the level says so.
///
/// Koha: `misc/cronjobs/overdue_notices.pl -t`, with `OverdueNoticeFrom` `cron`
/// (`issues.branchcode`).
pub(crate) fn queue_overdue_notices(connection: &Connection, at: &Zoned) -> Applied {
    let today = at.date();
    let created_at = at.to_string();
    let mut queued = 0;
    if !first_run_today(connection, "overdue_notices", today).map_err(storage)? {
        return Ok(WriteOutput::Circulation(
            CirculationOutput::OverdueNoticesQueued { queued },
        ));
    }
    let rules = read_rules(connection).map_err(storage)?;
    let (due, patrons) =
        due_today(connection, &rules, today, NoticeSettings::default()).map_err(storage)?;
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
        if wanted.restrict {
            restrict_for_overdues(connection, PatronId(*patron), &created_at).map_err(storage)?;
        }
        for transport in transports(wanted.transports, email.is_some()) {
            let notice = Notice {
                patron: *patron,
                branch: *branch,
                level: *level,
                transport: transport.as_str(),
                address: (transport == Transport::Email).then_some(email).flatten(),
            };
            if enqueue(connection, &notice, today, &created_at, items).map_err(storage)? {
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

/// Applies `circ.mark_notice`: a pending notice becomes sent or failed.
///
/// Koha: `C4::Letters::_set_message_status`.
pub(crate) fn mark(
    connection: &Connection,
    notice: NoticeId,
    outcome: NoticeOutcome,
    at: &Zoned,
) -> Applied {
    connection
        .execute(
            "UPDATE notices SET status = ?2, sent_at = ?3
             WHERE id = ?1 AND status = 'pending'",
            params![notice.0, outcome.as_str(), at.to_string()],
        )
        .map_err(storage)?;
    Ok(WriteOutput::Circulation(CirculationOutput::NoticeMarked))
}

/// The oldest overdue notices still pending by e-mail, at most `limit`, with their Items.
pub(crate) fn pending_emails(
    connection: &Connection,
    limit: usize,
) -> rusqlite::Result<Vec<PendingEmail>> {
    let mut statement = connection.prepare_cached(
        "SELECT n.id, n.to_address, p.first_name, p.surname, b.name, b.email, n.level,
                n.hold_until
         FROM notices n
         JOIN patrons p ON p.id = n.patron_id
         JOIN branches b ON b.id = n.branch_id
         WHERE n.status = 'pending' AND n.transport = 'email' AND n.to_address IS NOT NULL
         ORDER BY n.id LIMIT ?1",
    )?;
    let limit = i64::try_from(limit).unwrap_or(i64::MAX);
    let notices = statement
        .query_map([limit], |row| {
            let first: Option<String> = row.get(2)?;
            let surname: String = row.get(3)?;
            let about = about(row.get(6)?, row.get(7)?).ok_or_else(|| {
                rusqlite::Error::FromSqlConversionFailure(6, Type::Integer, "no such notice".into())
            })?;
            Ok(PendingEmail {
                id: NoticeId(row.get(0)?),
                to: row.get(1)?,
                name: first.map_or_else(|| surname.clone(), |first| format!("{first} {surname}")),
                library: row.get(4)?,
                reply_to: row
                    .get::<_, Option<String>>(5)?
                    .filter(|email| !email.trim().is_empty()),
                about,
                items: Vec::new(),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    notices
        .into_iter()
        .map(|mut notice| {
            notice.items = notice_items(connection, notice.id)?;
            Ok(notice)
        })
        .collect()
}

/// What a stored notice is about: an overdue level, or the last day of a waiting hold.
fn about(level: Option<i64>, hold_until: Option<String>) -> Option<NoticeAbout> {
    match (level, hold_until) {
        (Some(level), None) => NoticeLevel::from_number(level).map(NoticeAbout::Overdue),
        (None, Some(until)) => until.parse().ok().map(NoticeAbout::HoldWaiting),
        _ => None,
    }
}

fn notice_items(connection: &Connection, notice: NoticeId) -> rusqlite::Result<Vec<NoticeItem>> {
    let mut statement = connection.prepare_cached(
        "SELECT coalesce(i.barcode, ''), n.title, x.due
         FROM notice_items x
         JOIN items i ON i.id = x.item_id
         JOIN instances n ON n.id = i.instance_id
         WHERE x.notice_id = ?1
         ORDER BY x.due, i.barcode",
    )?;
    statement
        .query_map([notice.0], |row| {
            let due: Option<String> = row.get(2)?;
            let due = due
                .map(|due| due.parse())
                .transpose()
                .map_err(|error: jiff::Error| {
                    rusqlite::Error::FromSqlConversionFailure(2, Type::Text, error.into())
                })?;
            Ok(NoticeItem {
                barcode: row.get(0)?,
                title: row.get(1)?,
                due,
            })
        })?
        .collect()
}

impl NoticeQueue for ElzevirStore {
    fn pending_emails(&self, limit: usize) -> Result<Vec<PendingEmail>, NoticeQueueError> {
        self.readers()
            .read(|c| pending_emails(c, limit))
            .map_err(|error| NoticeQueueError(error.to_string()))
    }
}

/// Applies `circ.set_hold_notice`: how patrons of a category hear of a hold waiting for
/// them; neither transport removes the preference.
///
/// Koha: the `Hold_Filled` messaging preference of the category.
pub(crate) fn set_hold_notice(
    connection: &Connection,
    category: PatronCategoryId,
    wanted: Transports,
) -> Applied {
    scope_exists(
        connection,
        &RuleScopeKey {
            branch: None,
            category: Some(category),
            policy: None,
        },
    )?;
    connection
        .execute(
            "DELETE FROM hold_notice_preferences WHERE category_id = ?1",
            [category.0],
        )
        .map_err(storage)?;
    if wanted.email || wanted.print {
        connection
            .execute(
                "INSERT INTO hold_notice_preferences (category_id, email, print)
                 VALUES (?1, ?2, ?3)",
                params![category.0, wanted.email, wanted.print],
            )
            .map_err(storage)?;
    }
    Ok(WriteOutput::Circulation(CirculationOutput::HoldNoticeSet))
}

/// A hold now waiting at its pickup branch, for its notice.
pub(crate) struct WaitingHold {
    /// Whose it is.
    pub(crate) patron: PatronId,
    /// Where it waits.
    pub(crate) pickup: BranchId,
    /// The Item set aside.
    pub(crate) item: i64,
    /// Its last day to be picked up.
    pub(crate) until: Date,
}

/// Queues the notice of a hold that has just started waiting, by the transports of the
/// patron's category: an e-mail to a patron without one goes on paper, once.
///
/// Koha: `C4::Reserves::_koha_notify_reserve`, called by `ModReserveAffect` unless the hold
/// was already waiting.
pub(crate) fn queue_hold_notice(
    connection: &Connection,
    hold: &WaitingHold,
    at: &Zoned,
) -> rusqlite::Result<()> {
    let wanted: Option<(Transports, Option<String>)> = connection
        .query_row(
            "SELECT h.email, h.print, p.email FROM patrons p
             JOIN hold_notice_preferences h ON h.category_id = p.category_id
             WHERE p.id = ?1",
            [hold.patron.0],
            |row| {
                Ok((
                    Transports {
                        email: row.get(0)?,
                        print: row.get(1)?,
                    },
                    row.get(2)?,
                ))
            },
        )
        .optional()?;
    let Some((wanted, email)) = wanted else {
        return Ok(());
    };
    let email = email.filter(|email| !email.trim().is_empty());
    for transport in transports(wanted, email.is_some()) {
        let address = (transport == Transport::Email)
            .then(|| email.as_deref().map(str::trim))
            .flatten();
        connection.execute(
            "INSERT INTO notices (patron_id, kind, branch_id, transport, to_address, day,
                                  created_at, hold_until)
             VALUES (?1, 'HOLD', ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                hold.patron.0,
                hold.pickup.0,
                transport.as_str(),
                address,
                at.date().to_string(),
                at.to_string(),
                hold.until.to_string()
            ],
        )?;
        connection.execute(
            "INSERT INTO notice_items (notice_id, item_id, due) VALUES (?1, ?2, NULL)",
            params![connection.last_insert_rowid(), hold.item],
        )?;
    }
    Ok(())
}
