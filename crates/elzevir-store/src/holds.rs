// SPDX-License-Identifier: GPL-3.0-or-later
//! `holds` and `ended_holds`: the queues of holds (docs/specs/circulacao/reservas.md).

use elzevir_app::circulation_rules::{CirculationOutput, CirculationRefusal};
use elzevir_app::commands::{WriteOutput, WriteRefusal};
use elzevir_app::holds::{
    CheckinHold, HoldAside, HoldCancellation, HoldId, HoldLevel, HoldPlaced, HoldRefused,
    HoldRequest, PatronHold, SetAside, Suspension,
};
use elzevir_app::loans::checked_barcode;
use elzevir_domain::catalog::{BranchId, InstanceId, ItemId, Money, PolicyId};
use elzevir_domain::circulation_rules::{CirculationRule, RuleScopeKey};
use elzevir_domain::hold::{
    CurrentHold, Found, HoldItem, HoldMatch, HoldPatron, HoldRequestFacts, HoldSettings,
    HoldTarget, QueueState, QueuedHold, check_hold, hold_for_item, pickup_days,
};
use elzevir_domain::patron::{PatronCategoryId, PatronId};
use jiff::civil::Date;
use rusqlite::types::Type;
use rusqlite::{Connection, OptionalExtension, params};

use crate::circulation_rules::read_rules;
use crate::notices::{WaitingHold, queue_hold_notice};
use crate::restrictions::restricted;
use crate::{CommandFailed, StoreError};

type Applied = Result<WriteOutput, CommandFailed<WriteRefusal>>;

fn refused(refusal: HoldRefused) -> CommandFailed<WriteRefusal> {
    CommandFailed::Rejected(WriteRefusal::Circulation(CirculationRefusal::Hold(refusal)))
}

fn storage(error: rusqlite::Error) -> CommandFailed<WriteRefusal> {
    CommandFailed::from(StoreError::from(error))
}

fn date(text: &str, column: usize) -> rusqlite::Result<Date> {
    text.parse().map_err(|error: jiff::Error| {
        rusqlite::Error::FromSqlConversionFailure(column, Type::Text, error.into())
    })
}

/// The patron as the hold rules see them, at `today`.
fn hold_patron(
    connection: &Connection,
    patron: PatronId,
    today: Date,
) -> rusqlite::Result<Option<HoldPatron>> {
    let Some((branch, category)) = connection
        .query_row(
            "SELECT branch_id, category_id FROM patrons WHERE id = ?1",
            [patron.0],
            |row| Ok((BranchId(row.get(0)?), PatronCategoryId(row.get(1)?))),
        )
        .optional()?
    else {
        return Ok(None);
    };
    let suspended = restricted(connection, patron, today)?.is_some();
    // Koha: `Koha::Account::balance`, debits and credits together.
    let balance: i64 = connection.query_row(
        "SELECT coalesce(sum(outstanding), 0) FROM account_lines WHERE patron_id = ?1",
        [patron.0],
        |row| row.get(0),
    )?;
    Ok(Some(HoldPatron {
        branch,
        category,
        suspended,
        balance: Money(balance),
    }))
}

fn current_holds(connection: &Connection, patron: PatronId) -> rusqlite::Result<Vec<CurrentHold>> {
    let mut statement = connection.prepare_cached(
        "SELECT h.instance_id, h.item_id, i.policy_id, h.placed_on
         FROM holds h LEFT JOIN items i ON i.id = h.item_id
         WHERE h.patron_id = ?1",
    )?;
    statement
        .query_map([patron.0], |row| {
            let placed_on: String = row.get(3)?;
            Ok(CurrentHold {
                instance: InstanceId(row.get(0)?),
                item: row.get::<_, Option<i64>>(1)?.map(ItemId),
                policy: row.get::<_, Option<i64>>(2)?.map(PolicyId),
                placed_on: date(&placed_on, 3)?,
            })
        })?
        .collect()
}

fn instance_items(connection: &Connection, instance: i64) -> rusqlite::Result<Vec<HoldItem>> {
    let mut statement = connection.prepare_cached(
        "SELECT id, home_branch, holding_branch, policy_id FROM items
         WHERE instance_id = ?1 ORDER BY id",
    )?;
    statement
        .query_map([instance], |row| {
            Ok(HoldItem {
                id: ItemId(row.get(0)?),
                home: BranchId(row.get(1)?),
                holding: BranchId(row.get(2)?),
                policy: PolicyId(row.get(3)?),
            })
        })?
        .collect()
}

fn branches(connection: &Connection) -> rusqlite::Result<Vec<BranchId>> {
    let mut statement = connection.prepare_cached("SELECT id FROM branches ORDER BY id")?;
    statement
        .query_map([], |row| row.get(0).map(BranchId))?
        .collect()
}

/// Numbers the Instance's queue 1, 2, 3... in its current order, closing any gap; holds set
/// aside stay out of it.
///
/// Koha: `C4::Reserves::FixPriority` (without lowest priority).
pub(crate) fn fix_priority(connection: &Connection, instance: i64) -> rusqlite::Result<()> {
    let ids = {
        let mut statement = connection.prepare_cached(
            "SELECT id FROM holds WHERE instance_id = ?1 AND found IS NULL
             ORDER BY priority, id",
        )?;
        statement
            .query_map([instance], |row| row.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    for (id, priority) in ids.into_iter().zip(1_i64..) {
        connection.execute(
            "UPDATE holds SET priority = ?2 WHERE id = ?1 AND priority <> ?2",
            params![id, priority],
        )?;
    }
    Ok(())
}

/// Applies `circ.place_hold`: checks the hold as Koha's REST API does and puts it at the end
/// of the Instance's queue.
///
/// Koha: `Koha::REST::V1::Holds::add`, `C4::Reserves::CalculatePriority`, `AddReserve`.
pub(crate) fn place_hold(connection: &Connection, request: &HoldRequest) -> Applied {
    let today = request.at.date();
    let barcode =
        checked_barcode(&request.barcode).ok_or_else(|| refused(HoldRefused::UnknownBarcode))?;
    let (item, instance): (i64, i64) = connection
        .query_row(
            "SELECT id, instance_id FROM items WHERE barcode = ?1",
            [barcode],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(storage)?
        .ok_or_else(|| refused(HoldRefused::UnknownBarcode))?;
    let patron = hold_patron(connection, request.patron, today)
        .map_err(storage)?
        .ok_or_else(|| refused(HoldRefused::UnknownPatron))?;
    let branches = branches(connection).map_err(storage)?;
    if !branches.contains(&request.pickup) {
        return Err(refused(HoldRefused::UnknownBranch));
    }
    let rules = read_rules(connection).map_err(storage)?;
    let holds = current_holds(connection, request.patron).map_err(storage)?;
    let items = instance_items(connection, instance).map_err(storage)?;
    let target = match request.level {
        HoldLevel::Item => HoldTarget::Item(ItemId(item)),
        HoldLevel::Record => HoldTarget::Record,
    };
    let facts = HoldRequestFacts {
        rules: &rules,
        patron: &patron,
        holds: &holds,
        instance: InstanceId(instance),
        items: &items,
        branches: &branches,
        today,
    };
    // Koha's staff desk (`reserve/placerequest.pl`) does not look at the debt; the OPAC and
    // the REST API do (`Koha::Patron::can_place_holds`).
    let settings = if request.by_patron {
        HoldSettings::default()
    } else {
        HoldSettings {
            max_outstanding: None,
            ..HoldSettings::default()
        }
    };
    check_hold(&facts, target, request.pickup, &settings)
        .map_err(|refusal| refused(HoldRefused::Rules(refusal)))?;
    let queued: i64 = connection
        .query_row(
            "SELECT count(*) FROM holds WHERE instance_id = ?1 AND found IS NULL",
            [instance],
            |row| row.get(0),
        )
        .map_err(storage)?;
    connection
        .execute(
            "INSERT INTO holds (patron_id, instance_id, item_id, pickup_id, priority, placed_on,
                                created_at, item_level)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?3 IS NOT NULL)",
            params![
                request.patron.0,
                instance,
                (request.level == HoldLevel::Item).then_some(item),
                request.pickup.0,
                queued + 1,
                today.to_string(),
                request.at.to_string()
            ],
        )
        .map_err(storage)?;
    let hold = connection.last_insert_rowid();
    fix_priority(connection, instance).map_err(storage)?;
    let priority: u32 = connection
        .query_row("SELECT priority FROM holds WHERE id = ?1", [hold], |row| {
            row.get(0)
        })
        .map_err(storage)?;
    Ok(WriteOutput::Circulation(CirculationOutput::HoldPlaced(
        HoldPlaced {
            hold: HoldId(hold),
            priority,
        },
    )))
}

/// Applies `circ.cancel_hold`: the hold moves to the ended holds and the queue closes up.
///
/// Koha: `Koha::Hold::cancel`.
pub(crate) fn cancel_hold(connection: &Connection, request: &HoldCancellation) -> Applied {
    let (instance, set_aside): (i64, bool) = connection
        .query_row(
            "SELECT instance_id, found IS NOT NULL FROM holds WHERE id = ?1 AND patron_id = ?2",
            params![request.hold.0, request.patron.0],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(storage)?
        .ok_or_else(|| refused(HoldRefused::UnknownHold))?;
    if request.by_patron && set_aside {
        return Err(refused(HoldRefused::SetAside));
    }
    end_hold(
        connection,
        request.hold.0,
        &request.at.to_string(),
        "CANCELLED",
    )
    .map_err(storage)?;
    fix_priority(connection, instance).map_err(storage)?;
    Ok(WriteOutput::Circulation(CirculationOutput::HoldCancelled))
}

/// The patron's holds, oldest first.
pub(crate) fn patron_holds(
    connection: &Connection,
    patron: PatronId,
) -> rusqlite::Result<Vec<PatronHold>> {
    let mut statement = connection.prepare_cached(
        "SELECT h.id, n.title, i.barcode, b.name, h.placed_on, h.priority, h.found,
                h.expires_on, h.suspended, h.suspended_until
         FROM holds h
         JOIN instances n ON n.id = h.instance_id
         JOIN branches b ON b.id = h.pickup_id
         LEFT JOIN items i ON i.id = h.item_id
         WHERE h.patron_id = ?1
         ORDER BY h.created_at, h.id",
    )?;
    statement
        .query_map([patron.0], |row| {
            let placed_on: String = row.get(4)?;
            let found: Option<String> = row.get(6)?;
            let expires_on: Option<String> = row.get(7)?;
            let until: Option<String> = row.get(9)?;
            let suspended = match (row.get::<_, bool>(8)?, until) {
                (false, _) => None,
                (true, None) => Some(Suspension::Indefinite),
                (true, Some(day)) => Some(Suspension::Until(date(&day, 9)?)),
            };
            Ok(PatronHold {
                id: HoldId(row.get(0)?),
                title: row.get(1)?,
                barcode: row.get(2)?,
                pickup: row.get(3)?,
                placed_on: date(&placed_on, 4)?,
                priority: row.get(5)?,
                found: found.as_deref().and_then(Found::from_code),
                expires_on: expires_on.map(|day| date(&day, 7)).transpose()?,
                suspended,
            })
        })?
        .collect()
}

/// The Instance's holds, as `hold_for_item` reads them.
fn instance_queue(connection: &Connection, instance: i64) -> rusqlite::Result<Vec<QueuedHold>> {
    let mut statement = connection.prepare_cached(
        "SELECT h.id, p.branch_id, h.pickup_id, h.priority, h.found, h.item_id, h.suspended
         FROM holds h JOIN patrons p ON p.id = h.patron_id
         WHERE h.instance_id = ?1",
    )?;
    statement
        .query_map([instance], |row| {
            let found: Option<String> = row.get(4)?;
            let state = match found.as_deref().and_then(Found::from_code) {
                Some(found) => QueueState::Found(found),
                None if row.get::<_, bool>(6)? => QueueState::Suspended,
                None => QueueState::Queued,
            };
            Ok(QueuedHold {
                id: HoldId(row.get(0)?),
                patron_branch: BranchId(row.get(1)?),
                pickup: BranchId(row.get(2)?),
                priority: row.get(3)?,
                state,
                item: row.get::<_, Option<i64>>(5)?.map(ItemId),
            })
        })?
        .collect()
}

/// An Item at a checkin, as the holds see it.
pub(crate) struct CheckinItem {
    /// The Item, already at the desk's branch.
    pub(crate) item: HoldItem,
    /// Its Instance.
    pub(crate) instance: i64,
    /// Its `notforloan` status.
    pub(crate) not_for_loan: i64,
}

/// The hold a checked-in Item answers, read before anything changes.
///
/// Koha: `C4::Circulation::AddReturn` calls `C4::Reserves::CheckReserves` unless the Item is
/// withdrawn; `ConfirmFutureHolds` is off.
pub(crate) fn hold_at_checkin(
    connection: &Connection,
    rules: &[CirculationRule],
    item: &CheckinItem,
) -> rusqlite::Result<Option<CheckinHold>> {
    let queue = instance_queue(connection, item.instance)?;
    let Some(found) = hold_for_item(rules, &queue, &item.item, item.not_for_loan) else {
        return Ok(None);
    };
    let pickup = queue
        .iter()
        .find(|hold| hold.id == found.hold())
        .map_or(item.item.holding, |hold| hold.pickup);
    Ok(Some(CheckinHold { found, pickup }))
}

/// A hold waiting at another branch than the desk's goes back to the head of the queue, so
/// the Item can be sent where it is picked up.
///
/// Koha: `C4::Circulation::AddReturn` and `Koha::Hold::revert_found`.
pub(crate) fn revert_waiting(
    connection: &Connection,
    found: CheckinHold,
    desk: BranchId,
    instance: i64,
) -> rusqlite::Result<CheckinHold> {
    let HoldMatch::Waiting(hold) = found.found else {
        return Ok(found);
    };
    if found.pickup == desk {
        return Ok(found);
    }
    connection.execute(
        "UPDATE holds SET priority = priority + 1 WHERE instance_id = ?1 AND priority > 0",
        [instance],
    )?;
    connection.execute(
        "UPDATE holds SET priority = 1, found = NULL, waiting_since = NULL, expires_on = NULL,
                          item_id = CASE WHEN item_level = 1 THEN item_id END
         WHERE id = ?1",
        [hold.0],
    )?;
    fix_priority(connection, instance)?;
    Ok(CheckinHold {
        found: HoldMatch::Reserved(hold),
        ..found
    })
}

/// The hold to set aside, with what deciding needs.
struct Aside {
    instance: i64,
    item: Option<i64>,
    pickup: BranchId,
    category: i64,
    waiting_since: Option<String>,
    patron: PatronId,
    waiting: bool,
}

fn find_aside(connection: &Connection, hold: HoldId) -> rusqlite::Result<Option<Aside>> {
    connection
        .query_row(
            "SELECT h.instance_id, h.item_id, h.pickup_id, p.category_id, h.waiting_since,
                    h.patron_id, h.found = 'W'
             FROM holds h JOIN patrons p ON p.id = h.patron_id WHERE h.id = ?1",
            [hold.0],
            |row| {
                Ok(Aside {
                    instance: row.get(0)?,
                    item: row.get(1)?,
                    pickup: BranchId(row.get(2)?),
                    category: row.get(3)?,
                    waiting_since: row.get(4)?,
                    patron: PatronId(row.get(5)?),
                    waiting: row.get::<_, Option<bool>>(6)?.unwrap_or(false),
                })
            },
        )
        .optional()
}

/// Applies `circ.set_hold_aside`: the Item waits for the hold at the desk's branch, or goes
/// in transit to its pickup branch.
///
/// Koha: `C4::Reserves::ModReserveAffect`, `Koha::Hold::set_waiting`, `set_transfer`, and
/// `ModItemTransfer` with reason `Reserve` (`circ/returns.pl`).
pub(crate) fn set_hold_aside(connection: &Connection, request: &HoldAside) -> Applied {
    let barcode =
        checked_barcode(&request.barcode).ok_or_else(|| refused(HoldRefused::UnknownBarcode))?;
    let (item, instance, policy): (i64, i64, i64) = connection
        .query_row(
            "SELECT id, instance_id, policy_id FROM items WHERE barcode = ?1",
            [barcode],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(storage)?
        .ok_or_else(|| refused(HoldRefused::UnknownBarcode))?;
    let hold = find_aside(connection, request.hold)
        .map_err(storage)?
        .ok_or_else(|| refused(HoldRefused::UnknownHold))?;
    if hold.instance != instance || hold.item.is_some_and(|asked| asked != item) {
        return Err(refused(HoldRefused::WrongItem));
    }
    let at = request.at.to_string();
    let open: Option<i64> = connection
        .query_row(
            "SELECT id FROM transfers WHERE item_id = ?1 AND arrived_at IS NULL",
            [item],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage)?;
    let set_aside = if hold.pickup == request.desk {
        let today = request.at.date();
        let since = match &hold.waiting_since {
            Some(day) => date(day, 0).map_err(storage)?,
            None => today,
        };
        let scope = RuleScopeKey {
            branch: Some(hold.pickup),
            category: Some(PatronCategoryId(hold.category)),
            policy: Some(PolicyId(policy)),
        };
        let rules = read_rules(connection).map_err(storage)?;
        // A period past the calendar's end waits until its last day.
        let expires = since
            .checked_add(jiff::Span::new().days(i64::from(pickup_days(&rules, &scope))))
            .unwrap_or(Date::MAX);
        connection
            .execute(
                "UPDATE holds SET found = 'W', priority = 0, item_id = ?2, waiting_since = ?3,
                                  expires_on = ?4, suspended = 0, suspended_until = NULL
                 WHERE id = ?1",
                params![request.hold.0, item, since.to_string(), expires.to_string()],
            )
            .map_err(storage)?;
        if let Some(open) = open {
            connection
                .execute(
                    "UPDATE transfers SET arrived_at = ?2 WHERE id = ?1",
                    params![open, at],
                )
                .map_err(storage)?;
        }
        if !hold.waiting {
            let waiting = WaitingHold {
                patron: hold.patron,
                pickup: hold.pickup,
                item,
                until: expires,
            };
            queue_hold_notice(connection, &waiting, &request.at).map_err(storage)?;
        }
        SetAside {
            found: Found::Waiting,
            pickup: hold.pickup,
            expires_on: Some(expires),
        }
    } else {
        send_in_transit(connection, request, &hold, item).map_err(storage)?
    };
    fix_priority(connection, instance).map_err(storage)?;
    Ok(WriteOutput::Circulation(CirculationOutput::HoldSetAside(
        set_aside,
    )))
}

/// Sends the Item in transit to the hold's pickup branch: the hold is set aside in transit,
/// and the Item's open transfer is replaced by one for the hold.
fn send_in_transit(
    connection: &Connection,
    request: &HoldAside,
    hold: &Aside,
    item: i64,
) -> rusqlite::Result<SetAside> {
    let at = request.at.to_string();
    connection.execute(
        "UPDATE holds SET found = 'T', priority = 0, item_id = ?2, suspended = 0,
                          suspended_until = NULL
         WHERE id = ?1",
        params![request.hold.0, item],
    )?;
    // Koha: `ModItemTransfer` replaces the open transfer with this one; Elzevir keeps no
    // cancelled transfers.
    connection.execute(
        "DELETE FROM transfers WHERE item_id = ?1 AND arrived_at IS NULL",
        [item],
    )?;
    connection.execute(
        "INSERT INTO transfers (item_id, from_branch, to_branch, reason, sent_at)
             VALUES (?1, ?2, ?3, 'Reserve', ?4)",
        params![item, request.desk.0, hold.pickup.0, at],
    )?;
    Ok(SetAside {
        found: Found::InTransit,
        pickup: hold.pickup,
        expires_on: None,
    })
}

/// Another patron's hold, or the patron's own, that an Item answers at checkout.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ItemHold {
    /// The hold, and whether it is set aside.
    pub(crate) found: HoldMatch,
    /// Whose hold it is.
    pub(crate) patron: PatronId,
}

/// The hold the Item answers where it is now, before a checkout.
///
/// Koha: `C4::Circulation::CanBookBeIssued` calls `C4::Reserves::CheckReserves`.
pub(crate) fn hold_for_checkout(
    connection: &Connection,
    rules: &[CirculationRule],
    item: &HoldItem,
    instance: i64,
    not_for_loan: i64,
) -> rusqlite::Result<Option<ItemHold>> {
    let queue = instance_queue(connection, instance)?;
    let Some(found) = hold_for_item(rules, &queue, item, not_for_loan) else {
        return Ok(None);
    };
    let patron = connection.query_row(
        "SELECT patron_id FROM holds WHERE id = ?1",
        [found.hold().0],
        |row| row.get(0).map(PatronId),
    )?;
    Ok(Some(ItemHold { found, patron }))
}

/// After a checkout, fills the patron's own hold the Item answers, or else the patron's
/// first hold on the Instance (of the record, or of this Item) when the rule
/// `fill_other_biblio_holds_policy` (default on) lets it. The hold ends as `FILLED`.
///
/// Koha: `C4::Reserves::MoveReserve` (without `cancelreserve`) and `Koha::Hold::fill`.
pub(crate) fn fill_at_checkout(
    connection: &Connection,
    rules: &[CirculationRule],
    (item, instance): (&HoldItem, i64),
    patron: PatronId,
    answered: Option<ItemHold>,
    at: &str,
) -> rusqlite::Result<()> {
    let own = match answered {
        Some(found) if found.patron == patron => Some(found.found.hold()),
        _ => connection
            .query_row(
                "SELECT id FROM holds
                 WHERE patron_id = ?1 AND instance_id = ?2 AND (item_level = 0 OR item_id = ?3)
                 ORDER BY priority, id LIMIT 1",
                params![patron.0, instance, item.id.0],
                |row| row.get(0).map(HoldId),
            )
            .optional()?,
    };
    let Some(hold) = own else {
        return Ok(());
    };
    let asked: Option<i64> =
        connection.query_row("SELECT item_id FROM holds WHERE id = ?1", [hold.0], |row| {
            row.get(0)
        })?;
    if asked != Some(item.id.0) {
        let patron_branch = connection.query_row(
            "SELECT branch_id FROM patrons WHERE id = ?1",
            [patron.0],
            |row| row.get(0).map(BranchId),
        )?;
        let scope = RuleScopeKey {
            branch: Some(patron_branch),
            category: None,
            policy: Some(item.policy),
        };
        let fill = elzevir_domain::circulation_rules::effective_rule(
            rules,
            elzevir_app::circulation_rules::RuleKind::FillOtherBiblioHoldsPolicy,
            &scope,
        )
        .is_none_or(|rule| rule.value != "0" && !rule.value.is_empty());
        if !fill {
            return Ok(());
        }
    }
    connection.execute(
        "INSERT INTO ended_holds (patron_id, instance_id, item_id, pickup_id, placed_on,
                                  ended_at, outcome)
         SELECT CASE WHEN p.reading_history = 1 THEN h.patron_id END,
                h.instance_id, ?2, h.pickup_id, h.placed_on, ?3, 'FILLED'
         FROM holds h JOIN patrons p ON p.id = h.patron_id
         WHERE h.id = ?1",
        params![hold.0, item.id.0, at],
    )?;
    connection.execute("DELETE FROM holds WHERE id = ?1", [hold.0])?;
    fix_priority(connection, instance)
}

/// Applies `circ.expire_holds`: every hold waiting past its last day to pick up is
/// cancelled, and its queue closes up. The Item stays where it is, for the checkin to find
/// the next hold.
///
/// Koha: `C4::Reserves::CancelExpiredReserves` with `ExpireReservesMaxPickUpDelay` on,
/// `ExpireReservesOnHolidays` on and no charge (`ExpireReservesMaxPickUpDelayCharge` 0);
/// `misc/cronjobs/holds/cancel_expired_holds.pl`.
pub(crate) fn expire_holds(connection: &Connection, at: &jiff::Zoned) -> Applied {
    let today = at.date().to_string();
    let expired = {
        let mut statement = connection
            .prepare_cached(
                "SELECT id, instance_id FROM holds
                 WHERE found = 'W' AND expires_on < ?1 ORDER BY id",
            )
            .map_err(storage)?;
        statement
            .query_map([&today], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(storage)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage)?
    };
    let ended_at = at.to_string();
    for (hold, instance) in &expired {
        end_hold(connection, *hold, &ended_at, "CANCELLED").map_err(storage)?;
        fix_priority(connection, *instance).map_err(storage)?;
    }
    Ok(WriteOutput::Circulation(CirculationOutput::HoldsExpired {
        cancelled: expired.len(),
    }))
}

/// Moves a hold to the ended holds with `outcome`, without its patron unless the patron
/// keeps a reading history.
fn end_hold(connection: &Connection, hold: i64, at: &str, outcome: &str) -> rusqlite::Result<()> {
    connection.execute(
        "INSERT INTO ended_holds (patron_id, instance_id, item_id, pickup_id, placed_on,
                                  ended_at, outcome)
         SELECT CASE WHEN p.reading_history = 1 THEN h.patron_id END,
                h.instance_id, h.item_id, h.pickup_id, h.placed_on, ?2, ?3
         FROM holds h JOIN patrons p ON p.id = h.patron_id
         WHERE h.id = ?1",
        params![hold, at, outcome],
    )?;
    connection.execute("DELETE FROM holds WHERE id = ?1", [hold])?;
    Ok(())
}
