// SPDX-License-Identifier: GPL-3.0-or-later
//! Ownership survives truncation of the presentation job history.
use serde::{Deserialize, Serialize};
use std::{io::Read as _, path::Path};
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    ids: Vec<String>,
}
fn load(directory: &Path) -> Result<Catalog, String> {
    let path = directory.join("backup-artifacts.json");
    if !path.exists() {
        return Ok(Catalog::default());
    }
    if crate::backup_archive::regular(&path)? > 524_288 {
        return Err("backup artifact catalog exceeds limit".to_owned());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(524_289).read_to_end(&mut bytes))
        .map_err(|_| "backup artifact catalog unavailable")?;
    let catalog: Catalog =
        serde_json::from_slice(&bytes).map_err(|_| "invalid backup artifact catalog")?;
    let unique = catalog
        .ids
        .iter()
        .collect::<std::collections::BTreeSet<_>>();
    if catalog.ids.len() > 8192
        || unique.len() != catalog.ids.len()
        || catalog.ids.iter().any(|id| !crate::cloud::valid_id(id))
    {
        return Err("invalid backup artifact catalog".to_owned());
    }
    Ok(catalog)
}
pub(crate) fn ids(directory: &Path) -> Result<Vec<String>, String> {
    Ok(load(directory)?.ids)
}
fn save(directory: &Path, catalog: &Catalog) -> Result<(), String> {
    crate::broker::publish(
        directory,
        &directory.join("backup-artifacts.json"),
        &serde_json::to_vec(catalog).map_err(|_| "cannot encode backup artifact catalog")?,
    )
}
pub(crate) fn record(directory: &Path, id: &str) -> Result<(), String> {
    crate::backup_archive::regular(&crate::backup_cli::artifact(directory, id)?)?;
    let mut catalog = load(directory)?;
    if !catalog.ids.iter().any(|current| current == id) {
        catalog.ids.push(id.to_owned());
    }
    if catalog.ids.len() > 8192 {
        return Err("backup artifact catalog limit exceeded; review retention".to_owned());
    }
    save(directory, &catalog)
}
pub(crate) fn retain(directory: &Path, keep: u16) -> Result<(), String> {
    let mut catalog = load(directory)?;
    let remove = catalog.ids.len().saturating_sub(usize::from(keep.max(1)));
    for id in catalog.ids.iter().take(remove) {
        let path = crate::backup_cli::artifact(directory, id)?;
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_file() && !metadata.is_symlink() => {
                std::fs::remove_file(path).map_err(|_| "local backup retention failed")?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            _ => return Err("local backup retention target refused".to_owned()),
        }
    }
    catalog.ids.drain(..remove);
    save(directory, &catalog)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retention_beyond_job_history_keeps_the_requested_count_and_preserves_unknown_files() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join("backups")).unwrap();
        for number in 0_u32..90 {
            let id = format!("{number:032x}");
            crate::backup_archive::write_new(
                &crate::backup_cli::artifact(directory.path(), &id).unwrap(),
                b"fictional encrypted artifact",
            )
            .unwrap();
            record(directory.path(), &id).unwrap();
        }
        let unknown = directory.path().join("backups/other.age");
        std::fs::write(&unknown, b"unowned").unwrap();
        retain(directory.path(), 70).unwrap();
        assert_eq!(
            std::fs::read_dir(directory.path().join("backups"))
                .unwrap()
                .count(),
            71
        );
        assert!(unknown.exists());
        assert!(
            !crate::backup_cli::artifact(directory.path(), &format!("{:032x}", 19))
                .unwrap()
                .exists()
        );
        assert!(
            crate::backup_cli::artifact(directory.path(), &format!("{:032x}", 20))
                .unwrap()
                .exists()
        );
    }
}
