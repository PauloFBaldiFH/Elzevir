// SPDX-License-Identifier: GPL-3.0-or-later
//! One durable operational worker per institution; UI clients only submit and observe.

use crate::{backup_archive as archive, operations_config::Policy};
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex},
    thread::JoinHandle,
    time::Duration,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Task {
    Backup,
    Verify { id: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum State {
    Queued,
    Running,
    Completed,
    Failed,
    Interrupted,
}
impl State {
    fn active(self) -> bool {
        matches!(self, Self::Queued | Self::Running)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Job {
    pub id: String,
    pub task: Task,
    pub state: State,
    pub at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct History {
    pub jobs: Vec<Job>,
    pub last_daily: Option<jiff::civil::Date>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct View {
    pub policy: Policy,
    pub jobs: Vec<Job>,
}

struct Shared {
    state: Mutex<WorkerState>,
    wake: Condvar,
    directory: PathBuf,
}
struct WorkerState {
    history: History,
    stopping: bool,
}
#[derive(Clone)]
pub(crate) struct Client(Arc<Shared>);

pub(crate) struct Worker {
    client: Client,
    thread: Option<JoinHandle<Result<(), String>>>,
}

fn read_history(directory: &Path) -> Result<History, String> {
    let path = directory.join("operations-history.json");
    if !path
        .try_exists()
        .map_err(|_| "operational history unavailable")?
    {
        return Ok(History::default());
    }
    if archive::regular(&path)? > 65_536 {
        return Err("operational history exceeds limit".to_owned());
    }
    let mut text = String::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(65_537).read_to_string(&mut text))
        .map_err(|_| "operational history unavailable")?;
    let history: History =
        serde_json::from_str(&text).map_err(|_| "invalid operational history")?;
    if history.jobs.len() > 64
        || history.jobs.iter().any(|job| {
            crate::backup_cli::artifact(directory, &job.id).is_err()
                || job.at.parse::<jiff::Timestamp>().is_err()
        })
    {
        return Err("invalid operational history".to_owned());
    }
    Ok(history)
}

fn save_history(directory: &Path, history: &History) -> Result<(), String> {
    let path = directory.join("operations-history.json");
    if std::fs::symlink_metadata(&path).is_ok() {
        archive::regular(&path)?;
    }
    let bytes = serde_json::to_vec(history).map_err(|_| "cannot encode operational history")?;
    if bytes.len() > 65_536 {
        return Err("operational history exceeds limit".to_owned());
    }
    let mut staged = tempfile::NamedTempFile::new_in(directory)
        .map_err(|_| "cannot prepare operational history")?;
    staged
        .write_all(&bytes)
        .and_then(|()| staged.as_file().sync_all())
        .map_err(|_| "cannot write operational history")?;
    staged
        .persist(path)
        .map_err(|_| "cannot publish operational history")?;
    Ok(())
}

impl Client {
    pub(crate) fn view(&self) -> Result<View, String> {
        let policy = crate::operations_config::load(&self.0.directory)?;
        let state = self
            .0
            .state
            .lock()
            .map_err(|_| "operational worker unavailable")?;
        Ok(View {
            policy,
            jobs: state.history.jobs.clone(),
        })
    }
    pub(crate) fn submit(&self, task: Task) -> Result<Job, String> {
        if let Task::Verify { id } = &task {
            archive::regular(&crate::backup_cli::artifact(&self.0.directory, id)?)?;
        }
        let mut state = self
            .0
            .state
            .lock()
            .map_err(|_| "operational worker unavailable")?;
        if state.stopping || state.history.jobs.iter().any(|job| job.state.active()) {
            return Err("operational worker is busy or stopping".to_owned());
        }
        let job = Job {
            id: archive::identifier()?,
            task,
            state: State::Queued,
            at: jiff::Timestamp::now().to_string(),
        };
        let mut history = state.history.clone();
        push(&mut history, job.clone());
        save_history(&self.0.directory, &history)?;
        state.history = history;
        self.0.wake.notify_one();
        Ok(job)
    }
    pub(crate) fn policy(&self, policy: &Policy) -> Result<View, String> {
        let state = self
            .0
            .state
            .lock()
            .map_err(|_| "operational worker unavailable")?;
        if state.stopping {
            return Err("operational worker stopping".to_owned());
        }
        crate::operations_config::save(&self.0.directory, policy)?;
        drop(state);
        self.0.wake.notify_one();
        self.view()
    }
}

fn push(history: &mut History, job: Job) {
    if history.jobs.len() >= 64 {
        history.jobs.remove(0);
    }
    history.jobs.push(job);
}

impl Worker {
    pub(crate) fn start(
        directory: PathBuf,
        config: String,
        store: Arc<elzevir_store::ElzevirStore>,
    ) -> Result<Self, String> {
        crate::operations_config::load(&directory)?;
        let mut history = read_history(&directory)?;
        for job in &mut history.jobs {
            if job.state.active() {
                job.state = State::Interrupted;
            }
        }
        save_history(&directory, &history)?;
        let client = Client(Arc::new(Shared {
            state: Mutex::new(WorkerState {
                history,
                stopping: false,
            }),
            wake: Condvar::new(),
            directory,
        }));
        let runner = client.clone();
        let thread = std::thread::Builder::new()
            .name("elzevir-operations".to_owned())
            .spawn(move || runner.work(&config, &store))
            .map_err(|_| "cannot start operational worker")?;
        Ok(Self {
            client,
            thread: Some(thread),
        })
    }
    pub(crate) fn client(&self) -> Client {
        self.client.clone()
    }
    pub(crate) fn finish(mut self) -> Result<(), String> {
        self.stop()
    }
    fn stop(&mut self) -> Result<(), String> {
        let mut state = self
            .client
            .0
            .state
            .lock()
            .map_err(|_| "operational worker unavailable")?;
        state.stopping = true;
        drop(state);
        self.client.0.wake.notify_one();
        self.thread.take().map_or(Ok(()), |thread| {
            thread.join().map_err(|_| "operational worker failed")?
        })
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        // Preserve the writer/lock lifetime even if another server component fails.
        if self.stop().is_err() {
            tracing::error!(event = "operations.shutdown_failed");
        }
    }
}

impl Client {
    fn work(&self, config: &str, store: &elzevir_store::ElzevirStore) -> Result<(), String> {
        let mut heartbeat_at = None;
        loop {
            if heartbeat_at
                .is_none_or(|at: std::time::Instant| at.elapsed() >= Duration::from_hours(1))
            {
                if crate::broker::heartbeat(&self.0.directory).is_err() {
                    tracing::warn!(event = "broker.heartbeat_failed");
                }
                heartbeat_at = Some(std::time::Instant::now());
            }
            let mut state = self
                .0
                .state
                .lock()
                .map_err(|_| "operational worker unavailable")?;
            let policy = crate::operations_config::load(&self.0.directory)?;
            if !state.stopping
                && !state.history.jobs.iter().any(|job| job.state.active())
                && let Some(date) = policy.due(jiff::Timestamp::now(), state.history.last_daily)
            {
                let job = Job {
                    id: archive::identifier()?,
                    task: Task::Backup,
                    state: State::Queued,
                    at: jiff::Timestamp::now().to_string(),
                };
                state.history.last_daily = Some(date);
                push(&mut state.history, job);
                save_history(&self.0.directory, &state.history)?;
            }
            let queued = state
                .history
                .jobs
                .iter_mut()
                .find(|job| job.state == State::Queued);
            if let Some(job) = queued {
                job.state = State::Running;
                let job = job.clone();
                save_history(&self.0.directory, &state.history)?;
                drop(state);
                let result = self.execute(&job, config, store);
                let mut state = self
                    .0
                    .state
                    .lock()
                    .map_err(|_| "operational worker unavailable")?;
                let entry = state
                    .history
                    .jobs
                    .iter_mut()
                    .find(|entry| entry.id == job.id)
                    .ok_or("operational job lost")?;
                entry.state = if result.is_ok() {
                    State::Completed
                } else {
                    State::Failed
                };
                save_history(&self.0.directory, &state.history)?;
                if result.is_ok()
                    && matches!(job.task, Task::Backup)
                    && retain(&self.0.directory, &state.history, policy.keep).is_err()
                {
                    // The completed, synced artifact remains valid when cleanup fails.
                    tracing::warn!(event = "backup.retention_failed");
                }
                continue;
            }
            if state.stopping {
                break;
            }
            // Waits are interruptible by submission and shutdown; they never block the UI.
            drop(
                self.0
                    .wake
                    .wait_timeout(state, Duration::from_secs(30))
                    .map_err(|_| "operational worker unavailable")?,
            );
        }
        Ok(())
    }
    fn execute(
        &self,
        job: &Job,
        config: &str,
        store: &elzevir_store::ElzevirStore,
    ) -> Result<(), String> {
        match &job.task {
            Task::Backup => {
                crate::backup_cli::make_directory(&self.0.directory.join("backups"))?;
                archive::create(
                    &self.0.directory,
                    config,
                    &crate::backup_cli::artifact(&self.0.directory, &job.id)?,
                    Some(store.readers()),
                )?;
            }
            Task::Verify { id } => {
                archive::extract(
                    &crate::backup_cli::artifact(&self.0.directory, id)?,
                    &self.0.directory.join("backup.key"),
                )?;
            }
        }
        Ok(())
    }
}

/// Only artifacts with successful jobs are owned; unknown files are never candidates.
fn retain(directory: &Path, history: &History, keep: u16) -> Result<(), String> {
    let completed = history
        .jobs
        .iter()
        .rev()
        .filter(|job| matches!(job.task, Task::Backup) && job.state == State::Completed);
    for job in completed.skip(usize::from(keep.max(1))) {
        let path = crate::backup_cli::artifact(directory, &job.id)?;
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_file() && !metadata.is_symlink() => {
                std::fs::remove_file(path).map_err(|_| "backup retention failed")?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            _ => return Err("backup retention target refused".to_owned()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepted_backup_survives_client_drop_and_stop_releases_the_store() {
        let directory = tempfile::tempdir().unwrap();
        let data = crate::open_store(directory.path()).unwrap();
        let store = Arc::new(data.store);
        let weak = Arc::downgrade(&store);
        let worker = Worker::start(
            directory.path().to_owned(),
            "[server]\ndata_dir='.'\n".to_owned(),
            store,
        )
        .unwrap();
        let client = worker.client();
        let job = client.submit(Task::Backup).unwrap();
        drop(client);
        worker.finish().unwrap();
        assert!(weak.upgrade().is_none());
        let history = read_history(directory.path()).unwrap();
        let finished = history
            .jobs
            .iter()
            .find(|entry| entry.id == job.id)
            .unwrap();
        assert_eq!(finished.state, State::Completed);
        assert!(
            crate::backup_cli::artifact(directory.path(), &job.id)
                .unwrap()
                .is_file()
        );
    }
    #[test]
    fn retention_preserves_unknown_files_and_the_latest_snapshot() {
        let directory = tempfile::tempdir().unwrap();
        let backups = directory.path().join("backups");
        std::fs::create_dir(&backups).unwrap();
        std::fs::write(backups.join("operator-notes.age"), "do not delete").unwrap();
        let mut history = History::default();
        for number in 0..3_u8 {
            let id = format!("{number:032x}");
            std::fs::write(
                crate::backup_cli::artifact(directory.path(), &id).unwrap(),
                "fixture",
            )
            .unwrap();
            history.jobs.push(Job {
                id,
                task: Task::Backup,
                state: State::Completed,
                at: "2026-11-17T00:00:00Z".to_owned(),
            });
        }
        retain(directory.path(), &history, 1).unwrap();
        assert!(
            crate::backup_cli::artifact(directory.path(), &format!("{:032x}", 2))
                .unwrap()
                .exists()
        );
        assert_eq!(std::fs::read_dir(backups).unwrap().count(), 2);
    }
}
