// SPDX-License-Identifier: GPL-3.0-or-later
//! `circ.set_item_status` and `circ.change_status_name`: the lost, damaged and withdrawn
//! statuses of an Item, changed at the desk, and the names of their numbers
//! (docs/specs/circulacao/lost-items.md, part a).

use elzevir_app::circulation_rules::{CirculationDeskError, CirculationOutput, CirculationRefusal};
use elzevir_app::commands::{WriteOutput, WriteRefusal};
use elzevir_app::item_status::{
    ItemAtTheDesk, ItemStatusChange, ItemStatusChanged, ItemStatusDesk, ItemStatusLine,
    ItemStatusRefusal, StatusKind, StatusName, StatusNameChange, checked_change, checked_name,
};
use elzevir_domain::account::OverdueStatus;
use elzevir_domain::catalog::{BranchId, ItemId, Money, PolicyId};
use elzevir_domain::patron::PatronId;
use jiff::Zoned;
use rusqlite::types::Type;
use rusqlite::{Connection, OptionalExtension, params};

use crate::accounts::{OverdueLoan, close_overdue};
use crate::circulation_rules::read_rules;
use crate::long_overdue::charge_lost;
use crate::lost_found::{FoundAt, found_by_rules};
use crate::returns::close_loan;
use crate::{CommandFailed, ElzevirStore, StoreError};

type Applied = Result<WriteOutput, CommandFailed<WriteRefusal>>;

fn refused(refusal: ItemStatusRefusal) -> CommandFailed<WriteRefusal> {
    CommandFailed::Rejected(WriteRefusal::Circulation(CirculationRefusal::ItemStatus(
        refusal,
    )))
}

fn storage(error: rusqlite::Error) -> CommandFailed<WriteRefusal> {
    StoreError::from(error).into()
}

/// The columns of a status and its date. They come from this fixed list, never from data.
const fn columns(kind: StatusKind) -> (&'static str, &'static str) {
    match kind {
        StatusKind::Lost => ("lost", "lost_on"),
        StatusKind::Damaged => ("damaged", "damaged_on"),
        StatusKind::Withdrawn => ("withdrawn", "withdrawn_on"),
        StatusKind::NotForLoan => ("not_for_loan", "not_for_loan"),
    }
}

/// Applies `circ.change_status_name` (item 2).
pub(crate) fn change_name(connection: &Connection, change: &StatusNameChange) -> Applied {
    match checked_name(change).map_err(refused)? {
        StatusNameChange::Set(name) => {
            let taken: bool = connection
                .query_row(
                    "SELECT EXISTS (SELECT 1 FROM item_status_names
                                    WHERE kind = ?1 AND label = ?2 AND number <> ?3)",
                    params![name.kind.code(), name.label, name.number],
                    |row| row.get(0),
                )
                .map_err(storage)?;
            if taken {
                return Err(refused(ItemStatusRefusal::LabelTaken));
            }
            connection
                .execute(
                    "INSERT INTO item_status_names (kind, number, label) VALUES (?1, ?2, ?3)
                     ON CONFLICT (kind, number) DO UPDATE SET label = excluded.label",
                    params![name.kind.code(), name.number, name.label],
                )
                .map_err(storage)?;
        }
        StatusNameChange::Remove { kind, number } => {
            connection
                .execute(
                    "DELETE FROM item_status_names WHERE kind = ?1 AND number = ?2",
                    params![kind.code(), number],
                )
                .map_err(storage)?;
        }
    }
    Ok(WriteOutput::Circulation(
        CirculationOutput::StatusNameChanged,
    ))
}

/// What the status change needs of the Item.
struct Item {
    current: i64,
    holding: BranchId,
    policy: PolicyId,
    replacement_price: Option<Money>,
}

fn find_item(
    connection: &Connection,
    item: ItemId,
    kind: StatusKind,
) -> rusqlite::Result<Option<Item>> {
    let (column, _) = columns(kind);
    connection
        .query_row(
            &format!(
                "SELECT {column}, holding_branch, policy_id, replacement_price_cents
                 FROM items WHERE id = ?1"
            ),
            [item.0],
            |row| {
                Ok(Item {
                    current: row.get(0)?,
                    holding: BranchId(row.get(1)?),
                    policy: PolicyId(row.get(2)?),
                    replacement_price: row.get::<_, Option<i64>>(3)?.map(Money),
                })
            },
        )
        .optional()
}

/// Applies `circ.set_item_status` (items 3 to 8).
///
/// Koha: `catalogue/updateitem.pl`, `Koha::Item::store` and, for lost,
/// `C4::Circulation::LostItem` from `moredetail`.
pub(crate) fn set_status(connection: &Connection, change: &ItemStatusChange) -> Applied {
    checked_change(change).map_err(refused)?;
    let item = find_item(connection, change.item, change.kind)
        .map_err(storage)?
        .ok_or_else(|| refused(ItemStatusRefusal::UnknownItem))?;
    if item.current != change.from {
        return Err(refused(ItemStatusRefusal::Stale));
    }
    let mut done = ItemStatusChanged::default();
    if change.to == change.from {
        return Ok(WriteOutput::Circulation(
            CirculationOutput::ItemStatusChanged(done),
        ));
    }
    let at = change.at.to_string();
    let (column, since) = columns(change.kind);
    // The moment is kept when the status goes from 0 to a number and cleared at 0
    // (Koha: the `*_on` columns of `Koha::Item::store`).
    connection
        .execute(
            &format!(
                "UPDATE items SET {column} = ?2,
                        {since} = CASE WHEN ?2 = 0 THEN NULL WHEN ?3 = 0 THEN ?4 ELSE {since} END,
                        row_version = row_version + 1
                 WHERE id = ?1"
            ),
            params![change.item.0, change.to, change.from, at],
        )
        .map_err(storage)?;
    if change.kind == StatusKind::Lost {
        lose(connection, change, &item, &at, &mut done).map_err(storage)?;
        if change.to == 0 {
            // Found at the item page: the rules of the Item's home branch (item 10).
            done.found = found_by_rules(connection, change.item.0, FoundAt::ELSEWHERE, &change.at)
                .map_err(storage)?;
        }
    }
    Ok(WriteOutput::Circulation(
        CirculationOutput::ItemStatusChanged(done),
    ))
}

/// What Koha's `LostItem` does when the desk changes the lost status (item 4): with a loan
/// and a status above 0, the fine stops, the patron is charged and the loan ends; the open
/// transfer is cancelled on any change, as `moredetail` calls `LostItem` on any change.
fn lose(
    connection: &Connection,
    change: &ItemStatusChange,
    item: &Item,
    at: &str,
    done: &mut ItemStatusChanged,
) -> rusqlite::Result<()> {
    let loan: Option<(i64, PatronId)> = connection
        .query_row(
            "SELECT id, patron_id FROM loans WHERE item_id = ?1",
            [change.item.0],
            |row| Ok((row.get(0)?, PatronId(row.get(1)?))),
        )
        .optional()?;
    if let Some((loan, patron)) = loan.filter(|_| change.to > 0) {
        let about = OverdueLoan {
            loan,
            patron,
            item: change.item.0,
            branch: item.holding,
        };
        close_overdue(connection, patron, change.item.0, OverdueStatus::Lost, at)?;
        let rules = read_rules(connection)?;
        done.charged = charge_lost(
            connection,
            &rules,
            &about,
            item.policy,
            item.replacement_price,
            at,
        )?;
        close_loan(connection, loan, at, item.holding)?;
        done.loan_ended = true;
    }
    // Koha keeps the cancelled transfer with its reason (`ItemLost`); the Elzevir keeps no
    // cancelled transfers.
    done.transfer_cancelled = connection.execute(
        "DELETE FROM transfers WHERE item_id = ?1 AND arrived_at IS NULL",
        [change.item.0],
    )? > 0;
    Ok(())
}

fn unavailable(error: &dyn std::fmt::Display) -> CirculationDeskError {
    CirculationDeskError::Unavailable(error.to_string())
}

fn moment(index: usize, text: Option<String>) -> rusqlite::Result<Option<Zoned>> {
    text.map(|text| {
        text.parse::<Zoned>().map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(index, Type::Text, error.into())
        })
    })
    .transpose()
}

impl ItemStatusDesk for ElzevirStore {
    fn status_names(&self) -> Result<Vec<StatusName>, CirculationDeskError> {
        self.readers()
            .read(|c| {
                let mut statement = c.prepare_cached(
                    "SELECT kind, number, label FROM item_status_names ORDER BY kind, number",
                )?;
                let rows = statement.query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })?;
                let mut names = Vec::new();
                for row in rows {
                    let (kind, number, label) = row?;
                    if let Some(kind) = StatusKind::from_code(&kind) {
                        names.push(StatusName {
                            kind,
                            number,
                            label,
                        });
                    }
                }
                names.sort_by_key(|name| (name.kind, name.number));
                Ok::<_, rusqlite::Error>(names)
            })
            .map_err(|error| unavailable(&error))
    }

    fn item_at_the_desk(
        &self,
        barcode: &str,
    ) -> Result<Option<ItemAtTheDesk>, CirculationDeskError> {
        self.readers()
            .read(|c| {
                c.query_row(
                    "SELECT i.id, i.barcode, n.title, i.call_number, i.holding_branch, b.name,
                            l.due, i.lost, i.lost_on, i.damaged, i.damaged_on, i.withdrawn,
                            i.withdrawn_on
                     FROM items i
                     JOIN instances n ON n.id = i.instance_id
                     JOIN branches b ON b.id = i.holding_branch
                     LEFT JOIN loans l ON l.item_id = i.id
                     WHERE i.barcode = ?1",
                    [barcode],
                    |row| {
                        let line = |kind, number: usize| -> rusqlite::Result<ItemStatusLine> {
                            Ok(ItemStatusLine {
                                kind,
                                number: row.get(number)?,
                                since: moment(number + 1, row.get(number + 1)?)?,
                            })
                        };
                        Ok(ItemAtTheDesk {
                            item: ItemId(row.get(0)?),
                            barcode: row.get(1)?,
                            title: row.get(2)?,
                            call_number: row.get(3)?,
                            holding: BranchId(row.get(4)?),
                            holding_name: row.get(5)?,
                            due: moment(6, row.get(6)?)?,
                            statuses: vec![
                                line(StatusKind::Lost, 7)?,
                                line(StatusKind::Damaged, 9)?,
                                line(StatusKind::Withdrawn, 11)?,
                            ],
                        })
                    },
                )
                .optional()
            })
            .map_err(|error| unavailable(&error))
    }
}
