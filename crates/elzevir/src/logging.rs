// SPDX-License-Identifier: GPL-3.0-or-later
//! Operational events contain no HTTP or dependency payloads.

use std::path::Path;
use tracing_subscriber::prelude::*;

const TARGET: &str = "elzevir::ops";

/// Known operational events accept only numeric metrics, never free-form payloads.
#[derive(Clone, Copy)]
pub(crate) enum Event {
    ServerStarted,
    ServerStopped,
    ServerFailed,
    IndexerCommitted { effects: u64 },
    IndexerFailed,
    IndexerPanicked,
    RuntimeCleanupFailed,
    AdminHandlerFailed,
    OverdueFinesCharged { overdue: u64, charged: u64 },
    OverdueFinesFailed,
    LongOverdueMarked { lost: u64, charged: u64 },
    LongOverdueFailed,
    HoldsExpired { cancelled: u64 },
    HoldsExpiryFailed,
    OverdueNoticesQueued { queued: u64 },
    OverdueNoticesFailed,
}

/// Records a known event with the identity generated for this execution.
pub(crate) fn emit(instance: &str, event: Event) {
    match event {
        Event::ServerStarted => {
            tracing::info!(target: TARGET, institution = "local", instance, event = "server.started", pid = std::process::id(), "server started");
        }
        Event::ServerStopped => {
            tracing::info!(target: TARGET, institution = "local", instance, event = "server.stopped", "server stopped");
        }
        Event::ServerFailed => {
            tracing::error!(target: TARGET, institution = "local", instance, event = "server.failed", "server failed");
        }
        Event::IndexerCommitted { effects } => {
            tracing::info!(target: TARGET, institution = "local", instance, event = "indexer.committed", effects, "index batch committed");
        }
        Event::IndexerFailed => {
            tracing::warn!(target: TARGET, institution = "local", instance, event = "indexer.failed", "indexing failed; retry scheduled");
        }
        Event::IndexerPanicked => {
            tracing::warn!(target: TARGET, institution = "local", instance, event = "indexer.panicked", "indexer worker panicked");
        }
        Event::AdminHandlerFailed => {
            tracing::warn!(target: TARGET, institution = "local", instance, event = "admin.handler_failed", "administrative handler failed");
        }
        Event::OverdueFinesCharged { overdue, charged } => {
            tracing::info!(target: TARGET, institution = "local", instance, event = "fines.charged", overdue, charged, "overdue fines brought up to date");
        }
        Event::OverdueFinesFailed => {
            tracing::warn!(target: TARGET, institution = "local", instance, event = "fines.failed", "overdue fines run failed; retried next hour");
        }
        Event::LongOverdueMarked { lost, charged } => {
            tracing::info!(target: TARGET, institution = "local", instance, event = "long_overdue.marked", lost, charged, "long overdue items declared lost");
        }
        Event::LongOverdueFailed => {
            tracing::warn!(target: TARGET, institution = "local", instance, event = "long_overdue.failed", "long overdue run failed; retried next hour");
        }
        Event::HoldsExpired { cancelled } => {
            tracing::info!(target: TARGET, institution = "local", instance, event = "holds.expired", cancelled, "waiting holds past their pickup day cancelled");
        }
        Event::HoldsExpiryFailed => {
            tracing::warn!(target: TARGET, institution = "local", instance, event = "holds.expiry_failed", "expired holds run failed; retried next hour");
        }
        Event::OverdueNoticesQueued { queued } => {
            tracing::info!(target: TARGET, institution = "local", instance, event = "notices.overdue_queued", queued, "overdue notices queued");
        }
        Event::OverdueNoticesFailed => {
            tracing::warn!(target: TARGET, institution = "local", instance, event = "notices.overdue_failed", "overdue notices run failed; retried next hour");
        }
        Event::RuntimeCleanupFailed => {
            tracing::warn!(target: TARGET, institution = "local", instance, event = "runtime.cleanup_failed", "runtime metadata cleanup failed");
        }
    }
}

/// Starts a bounded, lossless queue; its guard flushes after the producers stop.
pub(crate) fn start(
    data_dir: &Path,
) -> Result<tracing_appender::non_blocking::WorkerGuard, String> {
    let directory = data_dir.join("logs");
    match std::fs::symlink_metadata(&directory) {
        Ok(metadata) if !metadata.is_dir() || metadata.is_symlink() => {
            return Err("logs must be a regular directory".to_owned());
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir(&directory).map_err(|_| "cannot create log directory")?;
        }
        Err(_) => return Err("cannot inspect log directory".to_owned()),
    }
    for entry in std::fs::read_dir(&directory).map_err(|_| "cannot inspect log directory")? {
        let entry = entry.map_err(|_| "cannot inspect log directory")?;
        let owned = entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with("elzevir") && name.ends_with("jsonl"));
        if owned
            && !entry
                .file_type()
                .map_err(|_| "cannot inspect operational log")?
                .is_file()
        {
            return Err("operational log must be a regular file".to_owned());
        }
    }
    let rolling = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("elzevir")
        .filename_suffix("jsonl")
        .max_log_files(7)
        .build(&directory)
        .map_err(|_| "cannot open operational log")?;
    let (writer, guard) = tracing_appender::non_blocking::NonBlockingBuilder::default()
        .buffered_lines_limit(1024)
        .lossy(false)
        .finish(rolling);
    tracing::subscriber::set_global_default(subscriber(writer))
        .map_err(|_| "cannot initialize operational logging")?;
    Ok(guard)
}

fn subscriber(
    writer: tracing_appender::non_blocking::NonBlocking,
) -> impl tracing::Subscriber + Send + Sync {
    let events = tracing_subscriber::fmt::layer()
        .json()
        .with_ansi(false)
        .with_current_span(false)
        .with_span_list(false)
        .with_writer(writer)
        .with_filter(tracing_subscriber::filter::filter_fn(|metadata| {
            metadata.target() == TARGET && *metadata.level() <= tracing::Level::INFO
        }));
    tracing_subscriber::registry().with(events)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_known_events_are_written_without_span_or_dependency_payloads() {
        let output = tempfile::NamedTempFile::new().unwrap();
        let (writer, guard) = tracing_appender::non_blocking(output.reopen().unwrap());
        let instance = "0".repeat(32);
        let marker = "fictional-secret-never-logged";
        tracing::subscriber::with_default(subscriber(writer), || {
            let span = tracing::info_span!(target: TARGET, "request", secret = marker);
            let _entered = span.enter();
            emit(&instance, Event::ServerStarted);
            emit(&instance, Event::IndexerCommitted { effects: 3 });
            emit(&instance, Event::ServerStopped);
            tracing::error!(target: "dependency", secret = marker);
            tracing::warn!(target: "elzevir::ops::http", secret = marker);
            tracing::debug!(target: TARGET, secret = marker);
        });
        drop(guard);
        let text = std::fs::read_to_string(output.path()).unwrap();
        assert!(!text.contains(marker));
        let rows: Vec<serde_json::Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(rows.len(), 3);
        for row in &rows {
            assert_eq!(row["target"], TARGET);
            assert!(row["timestamp"].as_str().unwrap().ends_with('Z'));
            assert_eq!(row["fields"]["institution"], "local");
            assert_eq!(row["fields"]["instance"], instance);
            assert!(row.get("span").is_none());
            assert!(row.get("spans").is_none());
        }
        assert_eq!(rows[1]["fields"]["effects"], 3);
        assert_eq!(rows[2]["fields"]["event"], "server.stopped");
    }
}
