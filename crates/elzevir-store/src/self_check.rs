// SPDX-License-Identifier: GPL-3.0-or-later
//! The self-check port: an Item by its barcode, the patron of a hold and branch names
//! (docs/specs/circulacao/sip2.md, items 25 and 27).

use elzevir_app::circulation_rules::CirculationDeskError;
use elzevir_app::self_check::{HoldPatron, SelfCheck, SelfCheckItem};
use elzevir_domain::catalog::{BranchId, ItemId};
use elzevir_domain::hold::HoldId;
use jiff::Zoned;
use jiff::civil::Date;
use rusqlite::OptionalExtension;
use rusqlite::types::Type;

use crate::ElzevirStore;

fn unavailable(error: &dyn std::fmt::Display) -> CirculationDeskError {
    CirculationDeskError::Unavailable(error.to_string())
}

fn conversion(
    index: usize,
    error: impl std::error::Error + Send + Sync + 'static,
) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(index, Type::Text, Box::new(error))
}

const ITEM: &str = "SELECT i.id, coalesce(i.barcode, ''), n.title, home.name, holding.name,
        i.call_number, l.due, i.lost, i.damaged, i.not_for_loan,
        EXISTS (SELECT 1 FROM transfers t WHERE t.item_id = i.id AND t.arrived_at IS NULL),
        (SELECT count(*) FROM holds h WHERE h.instance_id = i.instance_id),
        EXISTS (SELECT 1 FROM holds h WHERE h.item_id = i.id AND h.found IS NOT NULL),
        (SELECT h.expires_on FROM holds h WHERE h.item_id = i.id AND h.found = 'W')
     FROM items i
     JOIN instances n ON n.id = i.instance_id
     JOIN branches home ON home.id = i.home_branch
     JOIN branches holding ON holding.id = i.holding_branch
     LEFT JOIN loans l ON l.item_id = i.id
     WHERE i.barcode = ?1";

fn item_of(row: &rusqlite::Row<'_>) -> rusqlite::Result<SelfCheckItem> {
    let due = row
        .get::<_, Option<String>>(6)?
        .map(|text| text.parse::<Zoned>().map_err(|error| conversion(6, error)))
        .transpose()?;
    let waiting_until = row
        .get::<_, Option<String>>(13)?
        .map(|text| text.parse::<Date>().map_err(|error| conversion(13, error)))
        .transpose()?;
    Ok(SelfCheckItem {
        id: ItemId(row.get(0)?),
        barcode: row.get(1)?,
        title: row.get(2)?,
        home_branch: row.get(3)?,
        holding_branch: row.get(4)?,
        call_number: row.get(5)?,
        due,
        lost: row.get::<_, i64>(7)? != 0,
        damaged: row.get::<_, i64>(8)? != 0,
        not_for_loan: row.get(9)?,
        in_transit: row.get(10)?,
        holds: usize::try_from(row.get::<_, i64>(11)?).unwrap_or(0),
        set_aside: row.get(12)?,
        waiting_until,
    })
}

impl SelfCheck for ElzevirStore {
    fn item(&self, barcode: &str) -> Result<Option<SelfCheckItem>, CirculationDeskError> {
        self.readers()
            .read(|c| c.query_row(ITEM, [barcode], item_of).optional())
            .map_err(|error| unavailable(&error))
    }

    fn hold_patron(&self, hold: HoldId) -> Result<Option<HoldPatron>, CirculationDeskError> {
        self.readers()
            .read(|c| {
                c.query_row(
                    "SELECT p.cardnumber, p.first_name, p.surname, p.email, p.phone
                     FROM holds h JOIN patrons p ON p.id = h.patron_id WHERE h.id = ?1",
                    [hold.0],
                    |row| {
                        Ok(HoldPatron {
                            cardnumber: row.get(0)?,
                            first_name: row.get(1)?,
                            surname: row.get(2)?,
                            email: row.get(3)?,
                            phone: row.get(4)?,
                        })
                    },
                )
                .optional()
            })
            .map_err(|error| unavailable(&error))
    }

    fn branch_name(&self, branch: BranchId) -> Result<Option<String>, CirculationDeskError> {
        self.readers()
            .read(|c| {
                c.query_row(
                    "SELECT name FROM branches WHERE id = ?1",
                    [branch.0],
                    |row| row.get(0),
                )
                .optional()
            })
            .map_err(|error| unavailable(&error))
    }
}
