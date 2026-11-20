// SPDX-License-Identifier: GPL-3.0-or-later
//! The run of overdue fines, overdue notices, long overdue Items and expired holds while the
//! server is up (docs/specs/circulacao/conta.md, docs/specs/circulacao/avisos.md,
//! docs/specs/circulacao/reservas.md).
//!
//! Koha runs `misc/cronjobs/fines.pl`, `overdue_notices.pl -t`, `longoverdue.pl` and
//! `holds/cancel_expired_holds.pl` from cron; here the server runs
//! them when it starts and then every hour, so no system scheduler is needed. A run only
//! changes a fine when its cents change, and an Item already lost is left alone, so
//! running more often than daily charges the same.

use std::sync::Arc;
use std::time::Duration;

use elzevir_app::circulation_rules::{CirculationCommand, CirculationOutput};
use elzevir_app::commands::{WriteCommand, WriteOutput};
use elzevir_domain::long_overdue::LongOverdueSettings;
use elzevir_store::ElzevirStore;
use jiff::tz::TimeZone;

use crate::logging::{self, Event};

/// How long between runs.
const EVERY: Duration = Duration::from_hours(1);

/// Submits one command and waits for what it did; `None` when it failed.
async fn submit(
    store: &Arc<ElzevirStore>,
    command: CirculationCommand,
) -> Option<CirculationOutput> {
    let store = Arc::clone(store);
    let done = tokio::task::spawn_blocking(move || {
        store
            .submit(WriteCommand::Circulation(command))
            .map(elzevir_store::Pending::wait)
    })
    .await;
    match done {
        Ok(Ok(Ok(WriteOutput::Circulation(output)))) => Some(output),
        _ => None,
    }
}

fn count(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

/// Runs the overdue fines and queues the overdue notices, and then the long overdue Items and the expired holds when they
/// are configured, now and then every hour, until `shutdown` turns true.
pub(crate) async fn run(
    store: Arc<ElzevirStore>,
    time_zone: TimeZone,
    (long_overdue, expire_holds): (Option<LongOverdueSettings>, bool),
    instance: String,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    loop {
        let at = jiff::Timestamp::now().to_zoned(time_zone.clone());
        let event = match submit(
            &store,
            CirculationCommand::ChargeOverdues { at: at.clone() },
        )
        .await
        {
            Some(CirculationOutput::OverduesCharged(charged)) => Event::OverdueFinesCharged {
                overdue: count(charged.overdue),
                charged: count(charged.charged),
            },
            _ => Event::OverdueFinesFailed,
        };
        logging::emit(&instance, event);
        if expire_holds {
            let event =
                match submit(&store, CirculationCommand::ExpireHolds { at: at.clone() }).await {
                    Some(CirculationOutput::HoldsExpired { cancelled }) => Event::HoldsExpired {
                        cancelled: count(cancelled),
                    },
                    _ => Event::HoldsExpiryFailed,
                };
            logging::emit(&instance, event);
        }
        let event = match submit(
            &store,
            CirculationCommand::QueueOverdueNotices { at: at.clone() },
        )
        .await
        {
            Some(CirculationOutput::OverdueNoticesQueued { queued }) => {
                Event::OverdueNoticesQueued {
                    queued: count(queued),
                }
            }
            _ => Event::OverdueNoticesFailed,
        };
        logging::emit(&instance, event);
        if let Some(settings) = long_overdue {
            let event = match submit(&store, CirculationCommand::MarkLongOverdue { at, settings })
                .await
            {
                Some(CirculationOutput::LongOverdueMarked(marked)) => Event::LongOverdueMarked {
                    lost: count(marked.lost),
                    charged: count(marked.charged),
                },
                _ => Event::LongOverdueFailed,
            };
            logging::emit(&instance, event);
        }
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
