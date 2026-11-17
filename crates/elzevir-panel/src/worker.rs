// SPDX-License-Identifier: GPL-3.0-or-later
//! One polling thread, a one-element refresh queue and a latest-only result slot.

use crate::{ObservationRequest, Snapshot};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

type Update = (Snapshot, Instant, u64);

pub(crate) struct Worker {
    updates: Arc<Mutex<Option<Update>>>,
    stop: Arc<AtomicBool>,
    wake: Option<mpsc::SyncSender<()>>,
    task: Option<std::thread::JoinHandle<()>>,
    pending: Arc<Mutex<(ObservationRequest, u64)>>,
    sequence: AtomicU64,
}

impl Worker {
    pub(crate) fn start(
        mut source: impl FnMut(ObservationRequest) -> Snapshot + Send + 'static,
    ) -> std::io::Result<Self> {
        let updates = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let slot = Arc::clone(&updates);
        let stopping = Arc::clone(&stop);
        let (wake, receiver) = mpsc::sync_channel(1);
        let pending = Arc::new(Mutex::new((ObservationRequest::Poll, 0)));
        let requests = Arc::clone(&pending);
        let task = std::thread::Builder::new()
            .name("elzevir-panel-status".to_owned())
            .spawn(move || {
                let mut completed = 0;
                while !stopping.load(Ordering::Acquire) {
                    let (request, sequence) = requests
                        .lock()
                        .map_or((ObservationRequest::Poll, 0), |mut pending| {
                            std::mem::take(&mut *pending)
                        });
                    let observed_at = Instant::now();
                    let snapshot = source(request);
                    completed = completed.max(sequence);
                    if stopping.load(Ordering::Acquire) {
                        break;
                    }
                    if let Ok(mut slot) = slot.lock() {
                        *slot = Some((snapshot, observed_at, completed));
                    }
                    match receiver.recv_timeout(Duration::from_secs(1)) {
                        Ok(()) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
            })?;
        Ok(Self {
            updates,
            stop,
            wake: Some(wake),
            task: Some(task),
            pending,
            sequence: AtomicU64::new(0),
        })
    }

    pub(crate) fn take(&self) -> Option<Update> {
        if self
            .task
            .as_ref()
            .is_some_and(std::thread::JoinHandle::is_finished)
        {
            return Some((
                Snapshot::default(),
                Instant::now(),
                self.sequence.load(Ordering::Relaxed),
            ));
        }
        // A poisoned slot is an unavailable observation, never old positive health.
        self.updates.lock().map_or_else(
            |_| {
                Some((
                    Snapshot::default(),
                    Instant::now(),
                    self.sequence.load(Ordering::Relaxed),
                ))
            },
            |mut slot| slot.take(),
        )
    }

    pub(crate) fn refresh(&self) {
        self.request(ObservationRequest::Poll);
    }

    pub(crate) fn request(&self, request: ObservationRequest) -> Option<u64> {
        let mut pending = self.pending.lock().ok()?;
        let next = pending.0.clone().merge(request)?;
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed) + 1;
        *pending = (next, sequence);
        drop(pending);
        if let Some(wake) = &self.wake {
            // Full means a request is already queued; disconnect means the source ended.
            match wake.try_send(()) {
                Ok(())
                | Err(mpsc::TrySendError::Full(()) | mpsc::TrySendError::Disconnected(())) => {}
            }
        }
        Some(sequence)
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        drop(self.wake.take());
        // The source may still be inside an OS read or bounded IPC timeout. Do not
        // hold terminal restoration or user exit hostage to it; it will not poll again.
        drop(self.task.take());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refreshes_do_not_replace_a_queued_export_and_completion_is_correlated() {
        let (entered, seen) = mpsc::sync_channel(2);
        let (release, wait) = mpsc::sync_channel(1);
        let worker = Worker::start(move |request| {
            entered.send(request).unwrap();
            wait.recv().unwrap();
            Snapshot::default()
        })
        .unwrap();
        assert_eq!(
            seen.recv_timeout(Duration::from_secs(2)).unwrap(),
            ObservationRequest::Poll
        );
        worker.request(ObservationRequest::Verify);
        let exported = worker.request(ObservationRequest::Export).unwrap();
        for _ in 0..100 {
            worker.refresh();
        }
        release.send(()).unwrap();
        assert_eq!(
            seen.recv_timeout(Duration::from_secs(2)).unwrap(),
            ObservationRequest::Export
        );
        assert!(
            worker.take().unwrap().2 < exported,
            "an old poll acknowledged a new action"
        );
        drop(worker);
        release.send(()).unwrap();
    }

    #[test]
    fn queued_refresh_and_exit_never_wait_for_a_blocked_source() {
        let (entered, seen) = mpsc::sync_channel(1);
        let (release, wait) = mpsc::sync_channel(1);
        let worker = Worker::start(move |_| {
            entered.send(()).unwrap();
            wait.recv().unwrap();
            Snapshot::default()
        })
        .unwrap();
        seen.recv_timeout(Duration::from_secs(2)).unwrap();
        for _ in 0..100 {
            worker.refresh();
        }
        let (exited, finished) = mpsc::sync_channel(1);
        let dropper = std::thread::spawn(move || {
            drop(worker);
            exited.send(()).unwrap();
        });
        finished.recv_timeout(Duration::from_secs(2)).unwrap();
        release.send(()).unwrap();
        dropper.join().unwrap();
    }
}
