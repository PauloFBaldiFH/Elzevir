// SPDX-License-Identifier: GPL-3.0-or-later
//! Readiness only, on a separate loopback listener. No administrative commands.

use axum::{Json, extract::State, http::StatusCode};
use elzevir_store::ElzevirStore;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A component's last observed state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Check {
    Starting,
    Ok,
    Failed,
}

/// Versioned, bounded health response; never includes paths or user data.
#[derive(Debug, Serialize)]
pub(crate) struct Report {
    pub(crate) schema: u8,
    pub(crate) instance: String,
    pub(crate) pid: u32,
    pub(crate) version: String,
    pub(crate) uptime_seconds: u64,
    pub(crate) database: Check,
    pub(crate) indexer: Check,
}

impl Report {
    pub(crate) fn healthy(&self) -> bool {
        self.database == Check::Ok && self.indexer == Check::Ok
    }
}

pub(crate) struct Probe {
    instance: String,
    store: Arc<ElzevirStore>,
    started: Instant,
    indexer: Mutex<Option<(Check, Instant)>>,
    inspections: Arc<tokio::sync::Semaphore>,
}

impl Probe {
    pub(crate) fn new(store: Arc<ElzevirStore>) -> Result<Self, String> {
        let mut bytes = [0_u8; 16];
        getrandom::fill(&mut bytes).map_err(|_| "cannot generate instance identity")?;
        let instance = format!("{:032x}", u128::from_be_bytes(bytes));
        Ok(Self {
            instance,
            store,
            started: Instant::now(),
            indexer: Mutex::new(None),
            inspections: Arc::new(tokio::sync::Semaphore::new(4)),
        })
    }

    pub(crate) fn indexed(&self, success: bool) {
        // A poisoned observation is reported as failed, never recovered as healthy.
        if let Ok(mut observation) = self.indexer.lock() {
            *observation = Some((
                if success { Check::Ok } else { Check::Failed },
                Instant::now(),
            ));
        }
    }

    fn report(&self, database: Check) -> Report {
        let indexer = self.indexer.lock().map_or(Check::Failed, |observation| {
            index_state(*observation, Instant::now())
        });
        Report {
            schema: 1,
            instance: self.instance.clone(),
            pid: std::process::id(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            uptime_seconds: self.started.elapsed().as_secs(),
            database,
            indexer,
        }
    }

    fn inspect(&self) -> Report {
        let database = self
            .store
            .readers()
            .read(|connection| {
                connection.query_row(
                    "SELECT COUNT(*) = 3 FROM sqlite_schema
                 WHERE type = 'table'
                   AND name IN ('staff_accounts', 'audit_log', 'effect_outbox')",
                    [],
                    |row| row.get::<_, bool>(0),
                )
            })
            .is_ok_and(|ready| ready);
        self.report(if database { Check::Ok } else { Check::Failed })
    }

    pub(crate) async fn snapshot(self: &Arc<Self>) -> Report {
        let Ok(permit) = Arc::clone(&self.inspections).try_acquire_owned() else {
            return self.report(Check::Failed);
        };
        let worker = Arc::clone(self);
        match tokio::task::spawn_blocking(move || {
            // Keep the permit inside the worker even if its caller cancels.
            let _permit = permit;
            worker.inspect()
        })
        .await
        {
            Ok(report) => report,
            Err(_) => self.report(Check::Failed),
        }
    }
}

fn index_state(observation: Option<(Check, Instant)>, now: Instant) -> Check {
    match observation {
        None => Check::Starting,
        Some((_, at)) if now.saturating_duration_since(at) > Duration::from_secs(5) => {
            Check::Failed
        }
        Some((state, _)) => state,
    }
}

pub(crate) async fn endpoint(State(probe): State<Arc<Probe>>) -> impl axum::response::IntoResponse {
    let report = probe.snapshot().await;
    (
        if report.healthy() {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        [(axum::http::header::CACHE_CONTROL, "no-store")],
        Json(report),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn inspection_pressure_is_bounded_and_recovers_when_capacity_returns() {
        let root = tempfile::tempdir().unwrap();
        let data = crate::open_store(root.path()).unwrap();
        let probe = Arc::new(Probe::new(Arc::new(data.store)).unwrap());
        probe.indexed(true);
        let busy = Arc::clone(&probe.inspections)
            .try_acquire_many_owned(4)
            .unwrap();
        assert!(!probe.snapshot().await.healthy());
        drop(busy);
        assert!(probe.snapshot().await.healthy());
    }

    #[test]
    fn stale_or_failed_indexer_is_not_healthy() {
        let at = Instant::now();
        assert_eq!(index_state(None, at), Check::Starting);
        assert_eq!(index_state(Some((Check::Ok, at)), at), Check::Ok);
        assert_eq!(index_state(Some((Check::Failed, at)), at), Check::Failed);
        assert_eq!(
            index_state(Some((Check::Ok, at)), at + Duration::from_secs(6)),
            Check::Failed
        );
    }

    #[test]
    fn a_failed_database_is_not_ready_even_with_a_fresh_indexer() {
        let root = tempfile::tempdir().unwrap();
        let data = crate::open_store(root.path()).unwrap();
        let probe = Probe::new(Arc::new(data.store)).unwrap();
        probe.indexed(true);
        assert!(probe.inspect().healthy());
        assert!(!probe.report(Check::Failed).healthy());
    }

    #[tokio::test]
    async fn http_readiness_requires_a_successful_cycle_and_recovers_from_failure() {
        use axum::response::IntoResponse;
        use http_body_util::BodyExt as _;

        let root = tempfile::tempdir().unwrap();
        let data = crate::open_store(root.path()).unwrap();
        let probe = Arc::new(Probe::new(Arc::new(data.store)).unwrap());
        for (cycle, expected, state) in [
            (None, StatusCode::SERVICE_UNAVAILABLE, "starting"),
            (Some(false), StatusCode::SERVICE_UNAVAILABLE, "failed"),
            (Some(true), StatusCode::OK, "ok"),
        ] {
            if let Some(success) = cycle {
                probe.indexed(success);
            }
            let response = endpoint(State(Arc::clone(&probe))).await.into_response();
            assert_eq!(response.status(), expected);
            assert_eq!(response.headers()["cache-control"], "no-store");
            let body = response.into_body().collect().await.unwrap().to_bytes();
            let report: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(report["database"], "ok");
            assert_eq!(report["indexer"], state);
        }
    }
}
