// SPDX-License-Identifier: GPL-3.0-or-later
//! `circ.suspend_hold` and `circ.resume_suspended_holds`: a suspended hold keeps its place
//! in the queue, but no Item fills it (docs/specs/circulacao/reservas.md, item 19).

use elzevir_app::circulation_rules::{CirculationOutput, CirculationRefusal};
use elzevir_app::commands::{WriteOutput, WriteRefusal};
use elzevir_app::holds::{HoldRefused, HoldSuspensionRequest, Suspension, SuspensionChange};
use rusqlite::{Connection, OptionalExtension, params};

use crate::{CommandFailed, StoreError};

type Applied = Result<WriteOutput, CommandFailed<WriteRefusal>>;

fn refused(refusal: HoldRefused) -> CommandFailed<WriteRefusal> {
    CommandFailed::Rejected(WriteRefusal::Circulation(CirculationRefusal::Hold(refusal)))
}

fn storage(error: rusqlite::Error) -> CommandFailed<WriteRefusal> {
    StoreError::from(error).into()
}

/// Applies `circ.suspend_hold`: suspends the patron's hold, changes until when, or resumes
/// it. A hold already set aside is not suspended; resuming a hold that is not suspended
/// changes nothing, as in Koha.
///
/// Koha: `Koha::Hold::suspend_hold` (the day kept without its time) and `Koha::Hold::resume`.
pub(crate) fn suspend_hold(connection: &Connection, request: &HoldSuspensionRequest) -> Applied {
    let set_aside: bool = connection
        .query_row(
            "SELECT found IS NOT NULL FROM holds WHERE id = ?1 AND patron_id = ?2",
            params![request.hold.0, request.patron.0],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage)?
        .ok_or_else(|| refused(HoldRefused::UnknownHold))?;
    let (suspended, until) = match request.change {
        SuspensionChange::Suspend(_) if set_aside => {
            return Err(refused(HoldRefused::SuspendSetAside));
        }
        SuspensionChange::Suspend(Suspension::Indefinite) => (true, None),
        SuspensionChange::Suspend(Suspension::Until(day)) => (true, Some(day.to_string())),
        SuspensionChange::Resume => (false, None),
    };
    connection
        .execute(
            "UPDATE holds SET suspended = ?2, suspended_until = ?3 WHERE id = ?1",
            params![request.hold.0, suspended, until],
        )
        .map_err(storage)?;
    Ok(WriteOutput::Circulation(
        CirculationOutput::HoldSuspensionChanged,
    ))
}

/// Applies `circ.resume_suspended_holds`: every hold suspended until a day that is today or
/// earlier, in the library's time zone, comes back to the queue in its place.
///
/// Koha: `C4::Reserves::AutoUnsuspendReserves`, run by
/// `misc/cronjobs/holds/auto_unsuspend_holds.pl`.
pub(crate) fn resume_suspended_holds(connection: &Connection, at: &jiff::Zoned) -> Applied {
    let resumed = connection
        .execute(
            "UPDATE holds SET suspended = 0, suspended_until = NULL
             WHERE suspended = 1 AND suspended_until <= ?1",
            [at.date().to_string()],
        )
        .map_err(storage)?;
    Ok(WriteOutput::Circulation(
        CirculationOutput::SuspendedHoldsResumed { resumed },
    ))
}
