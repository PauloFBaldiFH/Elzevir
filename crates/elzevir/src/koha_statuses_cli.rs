// SPDX-License-Identifier: GPL-3.0-or-later
//! The item statuses of `migrate-koha` and `switch-from-koha`: the names of Koha's status
//! numbers and the moment each Item took its status (docs/specs/circulacao/lost-items.md,
//! item 17).

use elzevir_app::commands::{WriteCommand, WriteOutput};
use elzevir_app::koha_migration::{KohaPlan, KohaStatusesMigrated};
use elzevir_app::patrons::{PatronCommand, PatronOutput};
use elzevir_store::ElzevirStore;

/// The names and the dates, in batches, after the catalog.
pub(crate) fn migrate(
    store: &ElzevirStore,
    plan: &KohaPlan,
) -> Result<KohaStatusesMigrated, String> {
    let mut done = KohaStatusesMigrated::default();
    for batch in plan.status_batches() {
        let answer = store
            .submit(WriteCommand::Patrons(PatronCommand::MigrateKohaStatuses(
                Box::new(batch),
            )))
            .map_err(|e| e.to_string())?
            .wait()
            .map_err(|e| e.to_string())?;
        let WriteOutput::Patrons(PatronOutput::KohaStatusesMigrated(write)) = answer else {
            return Err("unexpected answer from the store".to_owned());
        };
        done.add(&write);
    }
    Ok(done)
}

/// The report: counts only.
pub(crate) fn report(plan: &KohaPlan, done: &KohaStatusesMigrated) -> String {
    format!(
        "item status names: {} migrated ({} with their number, for a name another number \
         has), {} already named, {} left out for a taken name, {} outside 1 to 99 or \
         unnamed\n\
         item status dates: {} Items dated, {} Items not found\n",
        done.names,
        done.names_numbered,
        done.names_existing,
        done.names_taken,
        plan.status_names_left,
        done.dates,
        done.items_left
    )
}
