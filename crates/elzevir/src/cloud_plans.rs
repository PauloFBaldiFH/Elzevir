// SPDX-License-Identifier: GPL-3.0-or-later
//! Plan-owned remote artifacts, daily claims and verified-upload retention.
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read as _,
    path::Path,
};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Plan {
    pub id: String,
    pub name: String,
    pub connection: String,
    pub folder: String,
    pub keep: u16,
    pub daily: Option<crate::operations_config::Daily>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Definition {
    pub id: Option<String>,
    pub name: String,
    pub connection: String,
    pub folder: String,
    pub keep: u16,
    pub daily: Option<crate::operations_config::Daily>,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    plans: Vec<Plan>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Artifact {
    #[serde(default)]
    state: TransferState,
    #[serde(default)]
    digest: Option<String>,
    id: String,
    plan: String,
    connection: String,
    folder: String,
    at: String,
}
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TransferState {
    Queued,
    Unprobed,
    Pending,
    Failed,
    #[default]
    Verified,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ledger {
    artifacts: Vec<Artifact>,
}
fn read<T: serde::de::DeserializeOwned>(
    directory: &Path,
    name: &str,
    limit: u64,
) -> Result<Option<T>, String> {
    let path = directory.join(name);
    if !path.exists() {
        return Ok(None);
    }
    if crate::backup_archive::regular(&path)? > limit {
        return Err("cloud plan data exceeds limit".to_owned());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(limit + 1).read_to_end(&mut bytes))
        .map_err(|_| "cloud plan data unavailable")?;
    if u64::try_from(bytes.len()).map_err(|_| "cloud plan data exceeds limit")? > limit {
        return Err("cloud plan data exceeds limit".to_owned());
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| "invalid cloud plan data".to_owned())
}
pub(crate) fn load(directory: &Path) -> Result<Vec<Plan>, String> {
    let catalog: Catalog = read(directory, "cloud-plans.json", 65_536)?.unwrap_or_default();
    let mut ids = BTreeSet::new();
    let mut names = BTreeSet::new();
    if catalog.plans.len() > 16 {
        return Err("cloud plan limit exceeded".to_owned());
    }
    for plan in &catalog.plans {
        if !crate::cloud::valid_id(&plan.id)
            || !crate::cloud::valid_id(&plan.connection)
            || !ids.insert(&plan.id)
            || !names.insert(&plan.name)
            || plan.name.trim().is_empty()
            || plan.name.len() > 64
            || plan.name.chars().any(char::is_control)
        {
            return Err("invalid cloud plan".to_owned());
        }
        crate::cloud::folder(&plan.folder)?;
        crate::operations_config::Policy {
            keep: plan.keep,
            daily: plan.daily.clone(),
        }
        .validate()?;
    }
    Ok(catalog.plans)
}
fn save(directory: &Path, plans: Vec<Plan>) -> Result<(), String> {
    crate::broker::publish(
        directory,
        &directory.join("cloud-plans.json"),
        &serde_json::to_vec(&Catalog { plans }).map_err(|_| "cannot encode cloud plans")?,
    )
}
pub(crate) fn get(directory: &Path, id: &str) -> Result<Plan, String> {
    if !crate::cloud::valid_id(id) {
        return Err("invalid cloud plan identity".to_owned());
    }
    load(directory)?
        .into_iter()
        .find(|plan| plan.id == id)
        .ok_or("cloud plan not found".to_owned())
}
pub(crate) fn upsert(directory: &Path, definition: Definition) -> Result<Plan, String> {
    let connection = crate::cloud::connection(directory, &definition.connection)?;
    if !connection.ready {
        return Err("cloud plan requires a ready connection".to_owned());
    }
    let mut plans = load(directory)?;
    let id = definition
        .id
        .clone()
        .map_or_else(crate::backup_archive::identifier, Ok)?;
    if !crate::cloud::valid_id(&id)
        || definition.id.is_some() && !plans.iter().any(|plan| plan.id == id)
    {
        return Err("existing cloud plan not found".to_owned());
    }
    let plan = Plan {
        id: id.clone(),
        name: definition.name,
        connection: definition.connection,
        folder: definition.folder,
        keep: definition.keep,
        daily: definition.daily,
    };
    if plan.name.trim().is_empty()
        || plan.name.len() > 64
        || plan.name.chars().any(char::is_control)
        || plans
            .iter()
            .any(|current| current.name == plan.name && current.id != id)
    {
        return Err("cloud plan requires a unique bounded name".to_owned());
    }
    crate::cloud::folder(&plan.folder)?;
    crate::operations_config::Policy {
        keep: plan.keep,
        daily: plan.daily.clone(),
    }
    .validate()?;
    if let Some(existing) = plans.iter_mut().find(|existing| existing.id == id) {
        *existing = plan.clone();
    } else {
        if plans.len() >= 16 {
            return Err("cloud plan limit exceeded".to_owned());
        }
        plans.push(plan.clone());
    }
    save(directory, plans)?;
    Ok(plan)
}
pub(crate) fn disable(directory: &Path, id: &str) -> Result<(), String> {
    let mut plans = load(directory)?;
    plans
        .iter_mut()
        .find(|plan| plan.id == id)
        .ok_or("cloud plan not found")?
        .daily = None;
    save(directory, plans)
}
pub(crate) fn remove(directory: &Path, id: &str) -> Result<(), String> {
    let mut plans = load(directory)?;
    let index = plans
        .iter()
        .position(|plan| plan.id == id)
        .ok_or("cloud plan not found")?;
    plans.remove(index);
    save(directory, plans)
}
pub(crate) fn due(
    directory: &Path,
    at: jiff::Timestamp,
    claims: &BTreeMap<String, jiff::civil::Date>,
) -> Result<Option<(String, jiff::civil::Date)>, String> {
    for plan in load(directory)? {
        if let Some(date) = (crate::operations_config::Policy {
            keep: plan.keep,
            daily: plan.daily,
        })
        .due(at, claims.get(&plan.id).copied())
        {
            return Ok(Some((plan.id, date)));
        }
    }
    Ok(None)
}
fn ledger(directory: &Path) -> Result<Ledger, String> {
    let ledger: Ledger =
        read(directory, "cloud-artifacts.json", 4 * 1024 * 1024)?.unwrap_or_default();
    let mut ids = BTreeSet::new();
    if ledger.artifacts.len() > 8192 {
        return Err("remote artifact ownership limit exceeded".to_owned());
    }
    for artifact in &ledger.artifacts {
        if !crate::cloud::valid_id(&artifact.id)
            || !crate::cloud::valid_id(&artifact.plan)
            || !crate::cloud::valid_id(&artifact.connection)
            || !ids.insert((&artifact.connection, &artifact.folder, &artifact.id))
            || artifact
                .digest
                .as_ref()
                .is_some_and(|h| h.len() != 64 || !h.bytes().all(|b| b.is_ascii_hexdigit()))
            || artifact.at.parse::<jiff::Timestamp>().is_err()
        {
            return Err("invalid remote artifact ownership".to_owned());
        }
        crate::cloud::folder(&artifact.folder)?;
    }
    Ok(ledger)
}
fn save_ledger(directory: &Path, ledger: &Ledger) -> Result<(), String> {
    let bytes =
        serde_json::to_vec(ledger).map_err(|_| "cannot encode remote artifact ownership")?;
    if bytes.len() > 4 * 1024 * 1024 || ledger.artifacts.len() > 8192 {
        return Err("remote artifact ownership limit exceeded".to_owned());
    }
    crate::broker::publish(directory, &directory.join("cloud-artifacts.json"), &bytes)?;
    #[cfg(unix)]
    std::fs::File::open(directory)
        .and_then(|f| f.sync_all())
        .map_err(|_| "cannot sync transfer ledger")?;
    Ok(())
}
trait Backend {
    fn preflight(&self, directory: &Path, plan: &Plan, id: &str) -> Result<(), String>;
    fn confirmed(
        &self,
        directory: &Path,
        plan: &Plan,
        id: &str,
        digest: &str,
    ) -> Result<(), String>;
    fn upload(
        &self,
        directory: &Path,
        plan: &Plan,
        source: &Path,
        digest: &str,
    ) -> Result<(), String>;
    fn remove(&self, directory: &Path, artifact: &Artifact) -> Result<(), String>;
}
struct Native {
    control: crate::cancellation::Control,
}
impl Native {
    fn progress(&self) -> crate::process_runner::Progress {
        let control = std::sync::Arc::clone(&self.control);
        std::sync::Arc::new(move |_| crate::cancellation::check(&control))
    }
}
impl Backend for Native {
    fn preflight(&self, directory: &Path, plan: &Plan, id: &str) -> Result<(), String> {
        crate::cloud::inspect_object(directory, plan, id, None, Some(self.progress()))
    }
    fn confirmed(
        &self,
        directory: &Path,
        plan: &Plan,
        id: &str,
        digest: &str,
    ) -> Result<(), String> {
        crate::cloud::inspect_object(directory, plan, id, Some(digest), Some(self.progress()))
    }
    fn upload(
        &self,
        directory: &Path,
        plan: &Plan,
        source: &Path,
        digest: &str,
    ) -> Result<(), String> {
        crate::cloud::upload(
            directory,
            &plan.connection,
            &plan.folder,
            source,
            digest,
            Some(self.progress()),
        )
    }
    fn remove(&self, directory: &Path, artifact: &Artifact) -> Result<(), String> {
        crate::cloud::remove_owned(
            directory,
            &artifact.connection,
            &artifact.folder,
            &artifact.id,
            artifact
                .digest
                .as_deref()
                .ok_or("remote digest unavailable")?,
            Some(self.progress()),
        )
    }
}
pub(crate) fn execute(
    directory: &Path,
    plan: &str,
    job: &str,
    config: &str,
    readers: &elzevir_store::ReadPool,
    control: crate::cancellation::Control,
) -> Result<(), String> {
    let native = Native { control };
    execute_with_control(
        directory,
        plan,
        job,
        config,
        readers,
        &native,
        &native.control,
    )
}

#[cfg(test)]
fn execute_with(
    directory: &Path,
    plan: &str,
    job: &str,
    config: &str,
    readers: &elzevir_store::ReadPool,
    backend: &impl Backend,
) -> Result<(), String> {
    let control: crate::cancellation::Control = std::sync::Arc::new(|| false);
    execute_with_control(directory, plan, job, config, readers, backend, &control)
}

fn execute_with_control(
    directory: &Path,
    plan_id: &str,
    job_id: &str,
    config: &str,
    readers: &elzevir_store::ReadPool,
    backend: &impl Backend,
    control: &crate::cancellation::Control,
) -> Result<(), String> {
    let plan = get(directory, plan_id)?;
    crate::backup_cli::make_directory(&directory.join("backups"))?;
    let output = crate::backup_cli::artifact(directory, job_id)?;
    crate::backup_archive::create_controlled(directory, config, &output, Some(readers), control)?;
    crate::local_artifacts::record(directory, job_id)?;
    send_group(
        directory,
        std::slice::from_ref(&plan),
        job_id,
        backend,
        control,
    )
}
pub(crate) fn reviewed(
    directory: &Path,
    target: &elzevir_panel::CloudTarget,
) -> Result<Plan, String> {
    let plan = get(directory, &target.plan)?;
    if plan.connection != target.connection || plan.folder != target.folder {
        return Err("review the changed destination".into());
    }
    Ok(plan)
}
pub(crate) fn resend(
    directory: &Path,
    targets: &[elzevir_panel::CloudTarget],
    id: &str,
    control: &crate::cancellation::Control,
) -> Result<(), String> {
    if !crate::local_artifacts::ids(directory)?
        .iter()
        .any(|v| v == id)
    {
        return Err("select a managed local copy".into());
    }
    let path = crate::backup_cli::artifact(directory, id)?;
    let verified = crate::backup_archive::extract_controlled(
        &path,
        &directory.join("backup.key"),
        std::sync::Arc::clone(control),
        Some(directory),
    )?;
    if verified.manifest.id != id {
        return Err("local archive identity changed".into());
    }
    drop(verified);
    if targets.is_empty() || targets.len() > 16 {
        return Err("select up to sixteen destinations".into());
    }
    let mut unique = BTreeSet::new();
    let plans = targets
        .iter()
        .map(|t| {
            if !unique.insert((&t.connection, &t.folder)) {
                return Err("duplicated destination".into());
            }
            reviewed(directory, t)
        })
        .collect::<Result<Vec<_>, String>>()?;
    send_group(
        directory,
        &plans,
        id,
        &Native {
            control: std::sync::Arc::clone(control),
        },
        control,
    )
}
fn send_group(
    directory: &Path,
    plans: &[Plan],
    id: &str,
    backend: &impl Backend,
    control: &crate::cancellation::Control,
) -> Result<(), String> {
    let path = crate::backup_cli::artifact(directory, id)?;
    let digest = crate::backup_archive::hash(&path, control)?;
    let mut ledger = ledger(directory)?;
    for plan in plans {
        if let Some(entry) = ledger
            .artifacts
            .iter()
            .find(|a| a.id == id && a.connection == plan.connection && a.folder == plan.folder)
        {
            if entry.plan != plan.id || entry.digest.as_ref() != Some(&digest) {
                return Err("artifact ownership or content changed".into());
            }
        } else {
            ledger.artifacts.push(Artifact {
                id: id.into(),
                plan: plan.id.clone(),
                connection: plan.connection.clone(),
                folder: plan.folder.clone(),
                at: jiff::Timestamp::now().to_string(),
                state: TransferState::Queued,
                digest: Some(digest.clone()),
            });
        }
    }
    save_ledger(directory, &ledger)?;
    let mut result = Ok(());
    for plan in plans {
        crate::cancellation::check(control)?;
        if let Err(error) = send(directory, plan, id, backend, &digest) {
            result = Err(error);
        }
    }
    result
}
fn send(
    directory: &Path,
    plan: &Plan,
    id: &str,
    backend: &impl Backend,
    digest: &str,
) -> Result<(), String> {
    let output = crate::backup_cli::artifact(directory, id)?;
    let mut ledger = ledger(directory)?;
    let index = ledger
        .artifacts
        .iter()
        .position(|a| a.id == id && a.connection == plan.connection && a.folder == plan.folder)
        .ok_or("missing upload intent")?;
    let entry = ledger
        .artifacts
        .get_mut(index)
        .ok_or("missing upload intent")?;
    let preflight = matches!(entry.state, TransferState::Queued | TransferState::Unprobed);
    let reconciled = !preflight && backend.confirmed(directory, plan, id, digest).is_ok();
    if preflight && let Err(error) = backend.preflight(directory, plan, id) {
        entry.state = TransferState::Unprobed;
        save_ledger(directory, &ledger)?;
        return Err(error);
    }
    entry.state = TransferState::Pending;
    save_ledger(directory, &ledger)?;
    let result = if reconciled {
        Ok(())
    } else {
        backend.upload(directory, plan, &output, digest)
    };
    ledger
        .artifacts
        .get_mut(index)
        .ok_or("remote entry missing")?
        .state = if result.is_ok() {
        TransferState::Verified
    } else {
        TransferState::Failed
    };
    save_ledger(directory, &ledger)?;
    result?;
    if retain(directory, plan, &mut ledger, backend).is_err() {
        tracing::warn!(event = "cloud.retention_failed");
    }
    Ok(())
}
fn retain(
    directory: &Path,
    plan: &Plan,
    ledger: &mut Ledger,
    backend: &impl Backend,
) -> Result<(), String> {
    let expired = ledger
        .artifacts
        .iter()
        .rev()
        .filter(|a| a.plan == plan.id && a.state == TransferState::Verified && a.digest.is_some())
        .skip(usize::from(plan.keep.max(1)))
        .cloned()
        .collect::<Vec<_>>();
    for artifact in expired {
        backend.remove(directory, &artifact)?;
        ledger.artifacts.retain(|a| {
            !(a.id == artifact.id
                && a.connection == artifact.connection
                && a.folder == artifact.folder)
        });
        save_ledger(directory, ledger)?;
    }
    Ok(())
}
pub(crate) fn pinned(directory: &Path) -> Result<BTreeSet<String>, String> {
    Ok(ledger(directory)?
        .artifacts
        .into_iter()
        .filter(|a| a.state != TransferState::Verified)
        .map(|a| a.id)
        .collect())
}
pub(crate) fn expected_digest(
    directory: &Path,
    target: &elzevir_panel::CloudTarget,
    id: &str,
) -> Result<Option<String>, String> {
    Ok(ledger(directory)?
        .artifacts
        .into_iter()
        .find(|a| a.connection == target.connection && a.folder == target.folder && a.id == id)
        .and_then(|a| a.digest))
}
pub(crate) fn history(
    directory: &Path,
) -> Result<Vec<(elzevir_panel::CloudTarget, String, String)>, String> {
    Ok(ledger(directory)?
        .artifacts
        .into_iter()
        .rev()
        .take(128)
        .map(|a| {
            (
                elzevir_panel::CloudTarget {
                    plan: a.plan,
                    connection: a.connection,
                    folder: a.folder,
                },
                a.id,
                match a.state {
                    TransferState::Queued => "op-queued",
                    TransferState::Pending => "cloud-unconfirmed",
                    TransferState::Unprobed | TransferState::Failed => "cloud-upload-failed",
                    TransferState::Verified => "cloud-verified",
                }
                .into(),
            )
        })
        .collect())
}
pub(crate) fn restored(directory: &Path) -> Result<(), String> {
    let mut plans = load(directory)?;
    for plan in &mut plans {
        plan.daily = None;
    }
    save(directory, plans)
}

pub(crate) fn summaries(directory: &Path) -> Result<Vec<elzevir_panel::CloudPlan>, String> {
    let ledger = ledger(directory)?;
    Ok(load(directory)?
        .into_iter()
        .map(|plan| {
            let count = ledger
                .artifacts
                .iter()
                .filter(|artifact| {
                    artifact.plan == plan.id
                        && artifact.state == TransferState::Verified
                        && artifact.digest.is_some()
                })
                .count();
            elzevir_panel::CloudPlan {
                id: plan.id,
                name: plan.name,
                connection: plan.connection,
                folder: plan.folder,
                keep: plan.keep,
                daily: plan.daily.map(|daily| {
                    format!("{:02}:{:02} {}", daily.hour, daily.minute, daily.time_zone)
                }),
                retention_pending: count > usize::from(plan.keep),
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    struct Fake {
        fail: bool,
        events: RefCell<Vec<String>>,
    }
    impl Backend for Fake {
        fn preflight(&self, _: &Path, _: &Plan, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn confirmed(&self, _: &Path, _: &Plan, _: &str, _: &str) -> Result<(), String> {
            if self.fail {
                Err("offline".into())
            } else {
                Ok(())
            }
        }
        fn upload(&self, _: &Path, plan: &Plan, source: &Path, _: &str) -> Result<(), String> {
            assert_eq!(plan.folder, "Elzevir/library");
            assert!(source.is_file());
            self.events.borrow_mut().push("upload".to_owned());
            if self.fail {
                Err("fixture upload failure".to_owned())
            } else {
                Ok(())
            }
        }
        fn remove(&self, _: &Path, artifact: &Artifact) -> Result<(), String> {
            self.events.borrow_mut().push(artifact.id.clone());
            Ok(())
        }
    }
    fn fixture(directory: &Path) -> Plan {
        let connection = "a".repeat(32);
        std::fs::write(directory.join("cloud.json"),serde_json::to_vec(&serde_json::json!({"connections":[{"id":connection,"name":"Fictional","provider":"local","config_id":connection,"ready":true,"parent":null}]})).unwrap()).unwrap();
        upsert(
            directory,
            Definition {
                id: None,
                name: "Fictional plan".to_owned(),
                connection,
                folder: "Elzevir/library".to_owned(),
                keep: 2,
                daily: None,
            },
        )
        .unwrap()
    }
    #[test]
    fn cloud_schedule_is_claimed_once_per_civil_date_across_dst_and_restarts() {
        let directory = tempfile::tempdir().unwrap();
        let plan = fixture(directory.path());
        let plan = upsert(
            directory.path(),
            Definition {
                id: Some(plan.id),
                name: plan.name,
                connection: plan.connection,
                folder: plan.folder,
                keep: plan.keep,
                daily: Some(crate::operations_config::Daily {
                    hour: 1,
                    minute: 30,
                    time_zone: "America/New_York".to_owned(),
                }),
            },
        )
        .unwrap();
        let first = "2026-11-01T05:30:00Z".parse().unwrap();
        let (id, date) = due(directory.path(), first, &BTreeMap::new())
            .unwrap()
            .unwrap();
        assert_eq!(id, plan.id);
        let claims = BTreeMap::from([(id, date)]);
        assert!(
            due(
                directory.path(),
                "2026-11-01T06:30:00Z".parse().unwrap(),
                &claims
            )
            .unwrap()
            .is_none()
        );
        assert!(
            due(
                directory.path(),
                "2026-10-31T05:30:00Z".parse().unwrap(),
                &claims
            )
            .unwrap()
            .is_none()
        );
        assert!(
            due(
                directory.path(),
                "2026-11-02T06:30:00Z".parse().unwrap(),
                &claims
            )
            .unwrap()
            .is_some()
        );
        restored(directory.path()).unwrap();
        assert!(
            load(directory.path())
                .unwrap()
                .iter()
                .all(|plan| plan.daily.is_none())
        );
    }
    #[test]
    fn failed_upload_keeps_the_local_artifact_and_does_not_claim_or_delete_remote_objects() {
        let directory = tempfile::tempdir().unwrap();
        let data = crate::open_store(directory.path()).unwrap();
        let plan = fixture(directory.path());
        let backend = Fake {
            fail: true,
            events: RefCell::new(Vec::new()),
        };
        let id = "b".repeat(32);
        assert!(
            execute_with(
                directory.path(),
                &plan.id,
                &id,
                "config",
                data.store.readers(),
                &backend
            )
            .is_err()
        );
        assert!(
            crate::backup_cli::artifact(directory.path(), &id)
                .unwrap()
                .exists()
        );
        assert_eq!(
            ledger(directory.path())
                .unwrap()
                .artifacts
                .first()
                .unwrap()
                .state,
            TransferState::Failed
        );
        assert_eq!(*backend.events.borrow(), ["upload"]);
    }
    #[test]
    fn interrupted_upload_reconciles_by_destination_and_digest_and_never_reuploads() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let plan = fixture(root);
        let id = "b".repeat(32);
        let output = crate::backup_cli::artifact(root, &id).unwrap();
        crate::backup_cli::make_directory(output.parent().unwrap()).unwrap();
        std::fs::write(&output, b"fixture").unwrap();
        let control: crate::cancellation::Control = std::sync::Arc::new(|| false);
        let digest = crate::backup_archive::hash(&output, &control).unwrap();
        save_ledger(
            root,
            &Ledger {
                artifacts: vec![Artifact {
                    id: id.clone(),
                    plan: plan.id.clone(),
                    connection: plan.connection.clone(),
                    folder: plan.folder.clone(),
                    at: jiff::Timestamp::now().to_string(),
                    state: TransferState::Pending,
                    digest: Some(digest),
                }],
            },
        )
        .unwrap();
        assert_eq!(
            history(root).unwrap().first().unwrap().2,
            "cloud-unconfirmed"
        );
        assert!(pinned(root).unwrap().contains(&id));
        let backend = Fake {
            fail: false,
            events: RefCell::default(),
        };
        let mut second = plan.clone();
        second.id = "f".repeat(32);
        second.folder = "Elzevir/library".into();
        second.connection = "9".repeat(32);
        send_group(root, &[plan.clone(), second], &id, &backend, &control).unwrap();
        assert_eq!(*backend.events.borrow(), ["upload"]);
        assert_eq!(history(root).unwrap().len(), 2);
        assert!(
            history(root)
                .unwrap()
                .iter()
                .all(|row| row.2 == "cloud-verified")
        );
        assert!(!pinned(root).unwrap().contains(&id));
        std::fs::write(output, b"replaced").unwrap();
        assert!(send_group(root, &[plan], &id, &backend, &control).is_err());
    }
    #[test]
    fn retention_runs_only_after_verification_and_targets_only_the_plans_owned_artifacts() {
        let directory = tempfile::tempdir().unwrap();
        let data = crate::open_store(directory.path()).unwrap();
        let plan = fixture(directory.path());
        let other = "e".repeat(32);
        let mut before = Ledger::default();
        for (id, owner) in [
            ("b".repeat(32), plan.id.clone()),
            ("c".repeat(32), plan.id.clone()),
            (other.clone(), "f".repeat(32)),
        ] {
            before.artifacts.push(Artifact {
                id,
                plan: owner,
                digest: Some("a".repeat(64)),
                state: TransferState::Verified,
                connection: plan.connection.clone(),
                folder: plan.folder.clone(),
                at: "2026-11-17T01:00:00Z".to_owned(),
            });
        }
        save_ledger(directory.path(), &before).unwrap();
        let backend = Fake {
            fail: false,
            events: RefCell::new(Vec::new()),
        };
        execute_with(
            directory.path(),
            &plan.id,
            &"d".repeat(32),
            "config",
            data.store.readers(),
            &backend,
        )
        .unwrap();
        assert_eq!(
            *backend.events.borrow(),
            ["upload".to_owned(), "b".repeat(32)]
        );
        let after = ledger(directory.path()).unwrap();
        assert!(after.artifacts.iter().any(|artifact| artifact.id == other));
        assert_eq!(
            after
                .artifacts
                .iter()
                .filter(|artifact| artifact.plan == plan.id
                    && artifact.state == TransferState::Verified
                    && artifact.digest.is_some())
                .count(),
            2
        );
    }
}
