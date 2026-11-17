// SPDX-License-Identifier: GPL-3.0-or-later
//! Bounded operational policy, atomically changed by the institution owner.

use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    path::Path,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Policy {
    pub keep: u16,
    pub daily: Option<Daily>,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            keep: 7,
            daily: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Daily {
    pub hour: u8,
    pub minute: u8,
    pub time_zone: String,
}

impl Policy {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if !(1..=365).contains(&self.keep) {
            return Err("retention must be between 1 and 365".to_owned());
        }
        if let Some(daily) = &self.daily
            && (daily.hour > 23
                || daily.minute > 59
                || daily.time_zone.len() > 100
                || jiff::tz::TimeZone::get(&daily.time_zone).is_err())
        {
            return Err("invalid backup time or IANA time zone".to_owned());
        }
        Ok(())
    }

    /// Returns the civil date to claim, never a duplicate or a backwards date.
    pub(crate) fn due(
        &self,
        now: jiff::Timestamp,
        last: Option<jiff::civil::Date>,
    ) -> Option<jiff::civil::Date> {
        let daily = self.daily.as_ref()?;
        // Validation is repeated at the load boundary; unavailable zones suppress work.
        let zone = jiff::tz::TimeZone::get(&daily.time_zone).ok()?;
        let local = now.to_zoned(zone);
        let date = local.date();
        let time = (local.hour(), local.minute());
        (last.is_none_or(|last| date > last)
            && time
                >= (
                    i8::try_from(daily.hour).ok()?,
                    i8::try_from(daily.minute).ok()?,
                ))
            .then_some(date)
    }
}

pub(crate) fn load(directory: &Path) -> Result<Policy, String> {
    let path = directory.join("operations.toml");
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Policy::default()),
        Err(_) => return Err("operational policy unavailable".to_owned()),
    };
    if !metadata.is_file() || metadata.is_symlink() || metadata.len() > 65_536 {
        return Err("invalid operational policy file".to_owned());
    }
    let mut source = String::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(65_537).read_to_string(&mut source))
        .map_err(|_| "operational policy unavailable")?;
    if source.len() > 65_536 {
        return Err("operational policy exceeds limit".to_owned());
    }
    let policy: Policy = toml::from_str(&source).map_err(|_| "invalid operational policy")?;
    policy.validate()?;
    Ok(policy)
}

pub(crate) fn save(directory: &Path, policy: &Policy) -> Result<(), String> {
    policy.validate()?;
    // Reject links and preserve ownership when an elevated operator edits a policy.
    let path = directory.join("operations.toml");
    if std::fs::symlink_metadata(&path).is_ok() {
        crate::backup_archive::regular(&path)?;
    }
    let source = toml::to_string_pretty(policy).map_err(|_| "cannot encode operational policy")?;
    let mut file = tempfile::NamedTempFile::new_in(directory)
        .map_err(|_| "cannot prepare operational policy")?;
    file.write_all(source.as_bytes())
        .and_then(|()| file.as_file().sync_all())
        .map_err(|_| "cannot write operational policy")?;
    crate::file_owner::configuration(file.path(), if path.exists() { &path } else { directory })?;
    file.persist(&path)
        .map_err(|_| "cannot publish operational policy")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scheduling_survives_late_start_dst_fallback_and_clock_reversal() {
        let policy = Policy {
            keep: 7,
            daily: Some(Daily {
                hour: 1,
                minute: 30,
                time_zone: "America/New_York".to_owned(),
            }),
        };
        policy.validate().unwrap();
        let first: jiff::Timestamp = "2026-11-01T05:30:00Z".parse().unwrap();
        let date = policy.due(first, None).unwrap();
        assert!(
            policy
                .due("2026-11-01T06:30:00Z".parse().unwrap(), Some(date))
                .is_none()
        );
        assert!(
            policy
                .due("2026-10-31T18:00:00Z".parse().unwrap(), Some(date))
                .is_none()
        );
        assert!(
            policy
                .due("2026-11-02T18:00:00Z".parse().unwrap(), Some(date))
                .is_some()
        );
        assert!(Policy::default().due(first, None).is_none());
    }
    #[test]
    fn invalid_policy_does_not_replace_the_previous_file() {
        let directory = tempfile::tempdir().unwrap();
        save(directory.path(), &Policy::default()).unwrap();
        assert!(
            save(
                directory.path(),
                &Policy {
                    keep: 0,
                    daily: None
                }
            )
            .is_err()
        );
        assert_eq!(load(directory.path()).unwrap().keep, 7);
    }
}
