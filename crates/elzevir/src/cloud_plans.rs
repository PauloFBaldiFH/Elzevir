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
    id: String,
    plan: String,
    connection: String,
    folder: String,
    at: String,
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
            || !ids.insert(&artifact.id)
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
    crate::broker::publish(directory, &directory.join("cloud-artifacts.json"), &bytes)
}
trait Backend {
    fn upload(&self, directory: &Path, plan: &Plan, source: &Path) -> Result<(), String>;
    fn remove(&self, directory: &Path, artifact: &Artifact) -> Result<(), String>;
}
struct Native;
impl Backend for Native {
    fn upload(&self, directory: &Path, plan: &Plan, source: &Path) -> Result<(), String> {
        crate::cloud::upload(directory, &plan.connection, &plan.folder, source)
    }
    fn remove(&self, directory: &Path, artifact: &Artifact) -> Result<(), String> {
        crate::cloud::remove_owned(
            directory,
            &artifact.connection,
            &artifact.folder,
            &artifact.id,
        )
    }
}
pub(crate) fn execute(
    directory: &Path,
    plan: &str,
    job: &str,
    config: &str,
    readers: &elzevir_store::ReadPool,
) -> Result<(), String> {
    execute_with(directory, plan, job, config, readers, &Native)
}
fn execute_with(
    directory: &Path,
    plan_id: &str,
    job_id: &str,
    config: &str,
    readers: &elzevir_store::ReadPool,
    backend: &impl Backend,
) -> Result<(), String> {
    let plan = get(directory, plan_id)?;
    let mut ledger = ledger(directory)?;
    if ledger.artifacts.len() >= 8192 {
        return Err("remote artifact ownership limit exceeded; review retention".to_owned());
    }
    crate::backup_cli::make_directory(&directory.join("backups"))?;
    let output = crate::backup_cli::artifact(directory, job_id)?;
    crate::backup_archive::create(directory, config, &output, Some(readers))?;
    crate::local_artifacts::record(directory, job_id)?;
    backend.upload(directory, &plan, &output)?;
    ledger.artifacts.push(Artifact {
        id: job_id.to_owned(),
        plan: plan.id.clone(),
        connection: plan.connection.clone(),
        folder: plan.folder.clone(),
        at: jiff::Timestamp::now().to_string(),
    });
    save_ledger(directory, &ledger)?;
    if retain(directory, &plan, &mut ledger, backend).is_err() {
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
    let ids = ledger
        .artifacts
        .iter()
        .rev()
        .filter(|artifact| artifact.plan == plan.id)
        .skip(usize::from(plan.keep.max(1)))
        .map(|artifact| artifact.id.clone())
        .collect::<Vec<_>>();
    for id in ids {
        let artifact = ledger
            .artifacts
            .iter()
            .find(|artifact| artifact.id == id)
            .ok_or("remote artifact ownership lost")?;
        backend.remove(directory, artifact)?;
        ledger.artifacts.retain(|artifact| artifact.id != id);
        save_ledger(directory, ledger)?;
    }
    Ok(())
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
                .filter(|artifact| artifact.plan == plan.id)
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
        fn upload(&self, _: &Path, plan: &Plan, source: &Path) -> Result<(), String> {
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
        assert!(ledger(directory.path()).unwrap().artifacts.is_empty());
        assert_eq!(*backend.events.borrow(), ["upload"]);
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
                .filter(|artifact| artifact.plan == plan.id)
                .count(),
            2
        );
    }
}
