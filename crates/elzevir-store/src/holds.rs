// SPDX-License-Identifier: GPL-3.0-or-later
//! `holds` and `ended_holds`: the queues of holds (docs/specs/circulacao/reservas.md).

use elzevir_app::circulation_rules::{CirculationOutput, CirculationRefusal};
use elzevir_app::commands::{WriteOutput, WriteRefusal};
use elzevir_app::holds::{
    HoldCancellation, HoldId, HoldLevel, HoldPlaced, HoldRefused, HoldRequest, PatronHold,
};
use elzevir_app::loans::checked_barcode;
use elzevir_domain::catalog::{BranchId, InstanceId, ItemId, Money, PolicyId};
use elzevir_domain::hold::{
    CurrentHold, HoldItem, HoldPatron, HoldRequestFacts, HoldSettings, HoldTarget, check_hold,
};
use elzevir_domain::patron::{PatronCategoryId, PatronId};
use jiff::civil::Date;
use rusqlite::types::Type;
use rusqlite::{Connection, OptionalExtension, params};

use crate::circulation_rules::read_rules;
use crate::restrictions::suspension;
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
    let suspended = suspension(connection, patron)?.is_some_and(|found| found.holds_on(today));
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

/// Numbers the Instance's queue 1, 2, 3... in its current order, closing any gap.
///
/// Koha: `C4::Reserves::FixPriority` (without lowest priority or found holds).
fn fix_priority(connection: &Connection, instance: i64) -> rusqlite::Result<()> {
    let ids = {
        let mut statement = connection
            .prepare_cached("SELECT id FROM holds WHERE instance_id = ?1 ORDER BY priority, id")?;
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
    check_hold(&facts, target, request.pickup, &HoldSettings::default())
        .map_err(|refusal| refused(HoldRefused::Rules(refusal)))?;
    let queued: i64 = connection
        .query_row(
            "SELECT count(*) FROM holds WHERE instance_id = ?1",
            [instance],
            |row| row.get(0),
        )
        .map_err(storage)?;
    connection
        .execute(
            "INSERT INTO holds (patron_id, instance_id, item_id, pickup_id, priority, placed_on,
                                created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
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
    let instance: i64 = connection
        .query_row(
            "SELECT instance_id FROM holds WHERE id = ?1 AND patron_id = ?2",
            params![request.hold.0, request.patron.0],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage)?
        .ok_or_else(|| refused(HoldRefused::UnknownHold))?;
    connection
        .execute(
            "INSERT INTO ended_holds (patron_id, instance_id, item_id, pickup_id, placed_on,
                                      ended_at, outcome)
             SELECT CASE WHEN p.reading_history = 1 THEN h.patron_id END,
                    h.instance_id, h.item_id, h.pickup_id, h.placed_on, ?2, 'CANCELLED'
             FROM holds h JOIN patrons p ON p.id = h.patron_id
             WHERE h.id = ?1",
            params![request.hold.0, request.at.to_string()],
        )
        .map_err(storage)?;
    connection
        .execute("DELETE FROM holds WHERE id = ?1", [request.hold.0])
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
        "SELECT h.id, n.title, i.barcode, b.name, h.placed_on, h.priority
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
            Ok(PatronHold {
                id: HoldId(row.get(0)?),
                title: row.get(1)?,
                barcode: row.get(2)?,
                pickup: row.get(3)?,
                placed_on: date(&placed_on, 4)?,
                priority: row.get(5)?,
            })
        })?
        .collect()
}
