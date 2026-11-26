// SPDX-License-Identifier: GPL-3.0-or-later
//! `patrons.migrate_koha_circulation`: one write of Koha's current loans and holds
//! (docs/specs/migracao/circulacao-do-koha.md). Patrons, records and Items are found by
//! Koha's numbers in the edge tables the earlier migrations and the import filled.

use elzevir_app::commands::WriteRefusal;
use elzevir_app::koha_migration::{
    CirculationLeftover, KohaCirculationBatch, KohaCirculationMigrated, KohaHold, KohaHoldState,
    KohaItem, KohaLoan,
};
use elzevir_app::patrons::PatronOutput;
use elzevir_domain::catalog::MatchRule;
use rusqlite::{Connection, OptionalExtension, params};

use crate::places::{Coded, by_code};
use crate::{CommandFailed, StoreError};

pub(crate) type Found<T> = Result<T, CirculationLeftover>;

/// Applies one write: the loans, then the holds.
pub(crate) fn apply(
    connection: &Connection,
    batch: &KohaCirculationBatch,
) -> Result<PatronOutput, CommandFailed<WriteRefusal>> {
    let mut done = KohaCirculationMigrated::default();
    for loan in &batch.loans {
        match add_loan(connection, loan)? {
            Ok(true) => done.loans += 1,
            Ok(false) => done.already_migrated += 1,
            Err(why) => done.loans_left.push((loan.issue_id, why)),
        }
    }
    for hold in &batch.holds {
        match add_hold(connection, hold)? {
            Ok(true) => done.holds += 1,
            Ok(false) => done.already_migrated += 1,
            Err(why) => done.holds_left.push((hold.reserve_id, why)),
        }
    }
    Ok(PatronOutput::KohaCirculationMigrated(done))
}

/// The one id `sql` finds for `key`; none or more than one is `why`.
fn only(
    connection: &Connection,
    sql: &str,
    key: &dyn rusqlite::ToSql,
    why: CirculationLeftover,
) -> Result<Found<i64>, StoreError> {
    let mut statement = connection.prepare_cached(sql)?;
    let ids = statement
        .query_map([key], |row| row.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(match ids.as_slice() {
        [id] => Ok(*id),
        _ => Err(why),
    })
}

pub(crate) fn patron(
    connection: &Connection,
    borrowernumber: u64,
) -> Result<Found<i64>, StoreError> {
    only(
        connection,
        "SELECT local_id FROM koha_keys WHERE kind = 2 AND koha_key = ?1",
        &borrowernumber.to_string(),
        CirculationLeftover::Patron,
    )
}

/// The Item imported with Koha's `itemnumber`, or else the one with its barcode.
pub(crate) fn item(connection: &Connection, item: &KohaItem) -> Result<Found<i64>, StoreError> {
    let by_number = only(
        connection,
        "SELECT DISTINCT item_id FROM item_origins
         WHERE system = 'koha.itemnumber' AND external_id = ?1",
        &item.itemnumber.to_string(),
        CirculationLeftover::Item,
    )?;
    match (&by_number, &item.barcode) {
        (Err(_), Some(barcode)) => only(
            connection,
            "SELECT id FROM items WHERE barcode = ?1",
            barcode,
            CirculationLeftover::Item,
        ),
        _ => Ok(by_number),
    }
}

/// The Instance imported with Koha's `biblionumber`.
fn instance(connection: &Connection, biblionumber: u64) -> Result<Found<i64>, StoreError> {
    let key = biblionumber.to_string();
    let mut statement = connection.prepare_cached(
        "SELECT instance_id FROM match_keys WHERE rule = ?1 AND key = ?2
         UNION SELECT instance_id FROM import_identities
         WHERE entity = 'instance' AND system = 'koha.biblionumber' AND external_id = ?2",
    )?;
    let ids = statement
        .query_map(params![MatchRule::KohaBiblionumber.code(), key], |row| {
            row.get::<_, i64>(0)
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(match ids.as_slice() {
        [id] => Ok(*id),
        _ => Err(CirculationLeftover::Record),
    })
}

fn branch(connection: &Connection, code: &str) -> Result<Found<i64>, StoreError> {
    Ok(by_code(connection, Coded::Branch, code)?.ok_or(CirculationLeftover::Branch))
}

/// Item 3: the loan, unless the Item is already lent (`Ok(false)` when to the same patron).
fn add_loan(connection: &Connection, loan: &KohaLoan) -> Result<Found<bool>, StoreError> {
    let patron = match patron(connection, loan.borrowernumber)? {
        Ok(id) => id,
        Err(why) => return Ok(Err(why)),
    };
    let item = match item(connection, &loan.item)? {
        Ok(id) => id,
        Err(why) => return Ok(Err(why)),
    };
    let branch = match branch(connection, &loan.branch)? {
        Ok(id) => id,
        Err(why) => return Ok(Err(why)),
    };
    let lent_to: Option<i64> = connection
        .query_row(
            "SELECT patron_id FROM loans WHERE item_id = ?1",
            [item],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(lent_to) = lent_to {
        return Ok(if lent_to == patron {
            Ok(false)
        } else {
            Err(CirculationLeftover::ItemOnLoan)
        });
    }
    connection.execute(
        "INSERT INTO loans (item_id, patron_id, branch_id, issued_at, due, on_site, renewals,
                            renewed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            item,
            patron,
            branch,
            loan.issued_at,
            loan.due,
            loan.on_site,
            loan.renewals,
            loan.renewed_at
        ],
    )?;
    // The OPAC keeps pages by version: the Item now shows as lent.
    connection.execute(
        "UPDATE items SET row_version = row_version + 1 WHERE id = ?1",
        [item],
    )?;
    Ok(Ok(true))
}

/// Item 5: the hold at the end of its Instance's queue, or set aside, unless the patron
/// already holds the Instance (`Ok(false)`).
fn add_hold(connection: &Connection, hold: &KohaHold) -> Result<Found<bool>, StoreError> {
    let patron = match patron(connection, hold.borrowernumber)? {
        Ok(id) => id,
        Err(why) => return Ok(Err(why)),
    };
    let instance = match instance(connection, hold.biblionumber)? {
        Ok(id) => id,
        Err(why) => return Ok(Err(why)),
    };
    let pickup = match branch(connection, &hold.pickup)? {
        Ok(id) => id,
        Err(why) => return Ok(Err(why)),
    };
    let item = match (
        &hold.item,
        hold.item_level || !matches!(hold.state, KohaHoldState::Queued(_)),
    ) {
        (Some(koha), true) => match item(connection, koha)? {
            Ok(id) => Some(id),
            Err(why) => return Ok(Err(why)),
        },
        _ => None,
    };
    let already_held = connection
        .query_row(
            "SELECT 1 FROM holds WHERE patron_id = ?1 AND instance_id = ?2",
            params![patron, instance],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if already_held {
        return Ok(Ok(false));
    }
    let (found, priority, since, until) = match hold.state {
        KohaHoldState::Queued(_) => {
            let queued: i64 = connection.query_row(
                "SELECT count(*) FROM holds WHERE instance_id = ?1 AND found IS NULL",
                [instance],
                |row| row.get(0),
            )?;
            (None, queued + 1, None, None)
        }
        KohaHoldState::Waiting { since, until } => (
            Some("W"),
            0,
            Some(since.to_string()),
            until.map(|day| day.to_string()),
        ),
        KohaHoldState::InTransit => (Some("T"), 0, None, None),
    };
    connection.execute(
        "INSERT INTO holds (patron_id, instance_id, item_id, pickup_id, priority, placed_on,
                            created_at, item_level, found, waiting_since, expires_on)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            patron,
            instance,
            item,
            pickup,
            priority,
            hold.placed_on.to_string(),
            hold.created_at,
            hold.item_level,
            found,
            since,
            until
        ],
    )?;
    if let (KohaHoldState::InTransit, Some(item)) = (hold.state, item) {
        // As `send_in_transit`: the Item travels from where it is to the pickup branch.
        connection.execute(
            "DELETE FROM transfers WHERE item_id = ?1 AND arrived_at IS NULL",
            [item],
        )?;
        connection.execute(
            "INSERT INTO transfers (item_id, from_branch, to_branch, reason, sent_at)
             SELECT id, holding_branch, ?2, 'Reserve', ?3 FROM items WHERE id = ?1",
            params![item, pickup, hold.created_at],
        )?;
    }
    crate::holds::fix_priority(connection, instance)?;
    Ok(Ok(true))
}
