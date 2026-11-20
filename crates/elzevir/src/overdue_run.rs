// SPDX-License-Identifier: GPL-3.0-or-later
//! The run of overdue fines while the server is up (docs/specs/circulacao/conta.md).
//!
//! Koha runs `misc/cronjobs/fines.pl` from cron; here the server runs it when it starts
//! and then every hour, so no system scheduler is needed. A run only changes a fine when
//! its cents change, so running more often than daily charges the same.

use std::sync::Arc;
use std::time::Duration;

use elzevir_app::circulation_rules::{CirculationCommand, CirculationOutput};
use elzevir_app::commands::{WriteCommand, WriteOutput};
use elzevir_store::ElzevirStore;
use jiff::tz::TimeZone;

use crate::logging::{self, Event};

/// How long between runs.
const EVERY: Duration = Duration::from_hours(1);

/// Runs the overdue fines now and then every hour, until `shutdown` turns true.
pub(crate) async fn run(
    store: Arc<ElzevirStore>,
    time_zone: TimeZone,
    instance: String,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    loop {
        let store = Arc::clone(&store);
        let at = jiff::Timestamp::now().to_zoned(time_zone.clone());
        let command = WriteCommand::Circulation(CirculationCommand::ChargeOverdues { at });
        let done = tokio::task::spawn_blocking(move || {
            store.submit(command).map(elzevir_store::Pending::wait)
        })
        .await;
        let event = match done {
            Ok(Ok(Ok(WriteOutput::Circulation(CirculationOutput::OverduesCharged(charged))))) => {
                Event::OverdueFinesCharged {
                    overdue: u64::try_from(charged.overdue).unwrap_or(u64::MAX),
                    charged: u64::try_from(charged.charged).unwrap_or(u64::MAX),
                }
            }
            _ => Event::OverdueFinesFailed,
        };
        logging::emit(&instance, event);
        tokio::select! {
            () = tokio::time::sleep(EVERY) => {}
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    return;
                }
            }
        }
    }
}
