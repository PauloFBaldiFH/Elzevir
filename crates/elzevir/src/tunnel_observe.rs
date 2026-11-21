// SPDX-License-Identifier: GPL-3.0-or-later
//! Public HTTPS checks and allowlisted connector events, without raw secret-bearing logs.
use elzevir_panel::{PublicCheck, Tunnel, TunnelLog};
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
    time::Duration,
};
trait Http {
    fn head(&self, url: &str) -> Result<u16, String>;
}
struct Native;
impl Http for Native {
    fn head(&self, url: &str) -> Result<u16, String> {
        let agent = crate::http_client::secure_agent(0, Duration::from_secs(3))?;
        let response = agent
            .head(url)
            .call()
            .map_err(|_| "public tunnel request failed")?;
        Ok(response.status().as_u16())
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Report {
    schema: u8,
    catalog: String,
    staff: String,
    at: String,
    catalog_code: Option<u16>,
    staff_code: Option<u16>,
}
fn classify(code: Option<u16>, staff: bool) -> PublicCheck {
    match code {
        Some(200..=399) => PublicCheck::Ok,
        Some(401 | 403) if staff => PublicCheck::Protected,
        Some(_) => PublicCheck::Failed,
        None => PublicCheck::Unknown,
    }
}
pub(crate) fn test(directory: &Path) -> Result<(), String> {
    test_with(directory, &Native, jiff::Timestamp::now(), false)
}
pub(crate) fn test_staff(directory: &Path) -> Result<(), String> {
    test_with(directory, &Native, jiff::Timestamp::now(), true)
}
fn test_with(
    directory: &Path,
    http: &impl Http,
    at: jiff::Timestamp,
    staff_only: bool,
) -> Result<(), String> {
    let (catalog, staff) =
        crate::cloudflare::public_hosts(directory)?.ok_or("no managed public tunnel configured")?;
    let catalog = format!("https://{catalog}/");
    let staff = format!("https://{staff}/equipe");
    // Failed requests remain absent; do not expose TLS/proxy/body error messages.
    let report = Report {
        schema: 1,
        catalog: catalog.clone(),
        staff: staff.clone(),
        at: at.to_string(),
        catalog_code: if staff_only {
            None
        } else {
            http.head(&catalog).ok()
        },
        staff_code: http.head(&staff).ok(),
    };
    let mut file = tempfile::NamedTempFile::new_in(directory)
        .map_err(|_| "cannot prepare public test report")?;
    file.write_all(&serde_json::to_vec(&report).map_err(|_| "cannot encode public test report")?)
        .and_then(|()| file.as_file().sync_all())
        .map_err(|_| "cannot write public test report")?;
    let path = directory.join(if staff_only {
        "cf-staff-observation.json"
    } else {
        "cf-observation.json"
    });
    if std::fs::symlink_metadata(&path).is_ok() {
        crate::backup_archive::regular(&path)?;
    }
    crate::file_owner::private_tree(file.path(), directory)?;
    file.persist(path)
        .map_err(|_| "cannot publish public test report")?;
    Ok(())
}
pub(crate) fn observe(directory: &Path) -> Result<Option<Tunnel>, String> {
    let Some((catalog, staff)) = crate::cloudflare::public_hosts(directory)? else {
        return Ok(None);
    };
    let mut view = Tunnel {
        catalog: format!("https://{catalog}/"),
        staff: format!("https://{staff}/equipe"),
        tested_at: None,
        staff_tested_at: None,
        catalog_check: PublicCheck::NotRun,
        staff_check: PublicCheck::NotRun,
        logs: logs(directory),
    };
    for (file, staff_only) in [
        ("cf-observation.json", false),
        ("cf-staff-observation.json", true),
    ] {
        let path = directory.join(file);
        if path
            .try_exists()
            .map_err(|_| "public test report unavailable")?
        {
            if crate::backup_archive::regular(&path)? > 4096 {
                return Err("public test report exceeds limit".to_owned());
            }
            let mut bytes = Vec::new();
            std::fs::File::open(path)
                .and_then(|file| file.take(4097).read_to_end(&mut bytes))
                .map_err(|_| "public test report unavailable")?;
            let report: Report =
                serde_json::from_slice(&bytes).map_err(|_| "invalid public test report")?;
            if report.schema != 1
                || report.at.parse::<jiff::Timestamp>().is_err()
                || report
                    .catalog_code
                    .is_some_and(|code| !(100..=599).contains(&code))
                || report
                    .staff_code
                    .is_some_and(|code| !(100..=599).contains(&code))
            {
                return Err("invalid public test report".to_owned());
            }
            if report.catalog == view.catalog && report.staff == view.staff {
                if !staff_only {
                    view.tested_at = Some(report.at.clone());
                    view.catalog_check = classify(report.catalog_code, false);
                }
                if view
                    .staff_tested_at
                    .as_deref()
                    .and_then(|s| s.parse::<jiff::Timestamp>().ok())
                    <= report.at.parse().ok()
                {
                    view.staff_tested_at = Some(report.at);
                    view.staff_check = classify(report.staff_code, true);
                }
            }
        }
    }
    Ok(Some(view))
}
fn log_event(message: &str) -> Option<&'static str> {
    if message.starts_with("Registered tunnel connection") {
        Some("tunnel-event-connected")
    } else if message.starts_with("Retrying connection") {
        Some("tunnel-event-retrying")
    } else if message.starts_with("Tunnel connection error")
        || message.starts_with("Failed to serve tunnel connection")
    {
        Some("tunnel-event-failed")
    } else if message.starts_with("Initiating graceful shutdown") {
        Some("tunnel-event-stopping")
    } else {
        None
    }
}
fn logs(directory: &Path) -> TunnelLog {
    // A missing or unreadable source is unavailable, never an empty successful log.
    read_logs(&directory.join("logs/cloudflared/events.jsonl")).unwrap_or(TunnelLog::Unavailable)
}
fn read_logs(path: &Path) -> Result<TunnelLog, String> {
    let length = crate::backup_archive::regular(path)?;
    let mut source = std::fs::File::open(path).map_err(|_| "tunnel log unavailable")?;
    let offset = length.saturating_sub(65_536);
    source
        .seek(SeekFrom::Start(offset))
        .map_err(|_| "tunnel log unavailable")?;
    let mut bytes = Vec::new();
    source
        .take(65_537)
        .read_to_end(&mut bytes)
        .map_err(|_| "tunnel log unavailable")?;
    let mut events = Vec::new();
    let mut omitted = 0_u64;
    let mut lines = bytes.split_inclusive(|byte| *byte == b'\n');
    if offset > 0 {
        lines.next();
    }
    for line in lines {
        if line.last() != Some(&b'\n') {
            continue;
        }
        let event = serde_json::from_slice::<serde_json::Value>(line)
            .ok()
            .and_then(|value| {
                value
                    .get("message")
                    .and_then(serde_json::Value::as_str)
                    .and_then(log_event)
            });
        if let Some(event) = event {
            events.push(event);
        } else {
            omitted = omitted.saturating_add(1);
        }
    }
    events.reverse();
    events.truncate(40);
    Ok(TunnelLog::Ready { events, omitted })
}

pub(crate) fn status(directory: &Path) -> Result<serde_json::Value, String> {
    let Some(view) = observe(directory)? else {
        return Ok(serde_json::json!({"schema":1,"state":"not_configured"}));
    };
    let logs = match view.logs {
        TunnelLog::Unavailable => serde_json::json!({"state":"unavailable"}),
        TunnelLog::Ready { events, omitted } => {
            serde_json::json!({"state":"ready","events":events.iter().filter_map(|event|event.strip_prefix("tunnel-event-")).collect::<Vec<_>>(),"omitted":omitted})
        }
    };
    Ok(
        serde_json::json!({"schema":1,"state":"configured","catalog":view.catalog,"staff":view.staff,"last_test":{"at":view.tested_at,"staff_at":view.staff_tested_at,"catalog":view.catalog_check.code(),"staff":view.staff_check.code()},"logs":logs}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fake;
    impl Http for Fake {
        fn head(&self, url: &str) -> Result<u16, String> {
            Ok(if url.contains("staff.example") {
                403
            } else {
                200
            })
        }
    }
    struct StaffOnly;
    impl Http for StaffOnly {
        fn head(&self, url: &str) -> Result<u16, String> {
            assert_eq!(url, "https://staff.example.org/equipe");
            Ok(403)
        }
    }
    #[test]
    fn public_tests_distinguish_protected_staff_and_log_projection_omits_payloads() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join("secrets")).unwrap();
        std::fs::write(directory.path().join("secrets/cf-intent.json"),serde_json::to_vec(&serde_json::json!({"schema":1,"catalog":"catalog.example.org","staff":"staff.example.org","name":"fictional","id":"00000000-0000-4000-8000-000000000001","catalog_created":true,"staff_created":true,"enforce_hosts":true})).unwrap()).unwrap();
        std::fs::create_dir_all(directory.path().join("logs/cloudflared")).unwrap();
        std::fs::write(directory.path().join("logs/cloudflared/events.jsonl"),b"{\"message\":\"Registered tunnel connection\",\"token\":\"fictional-secret\"}\n{\"message\":\"unknown fictional-secret\"}\n").unwrap();
        test_with(
            directory.path(),
            &Fake,
            "2026-11-17T00:00:00Z".parse().unwrap(),
            false,
        )
        .unwrap();
        let view = observe(directory.path()).unwrap().unwrap();
        test_with(
            directory.path(),
            &StaffOnly,
            "2026-11-18T00:00:00Z".parse().unwrap(),
            true,
        )
        .unwrap();
        let latest = observe(directory.path()).unwrap().unwrap();
        assert_eq!(latest.tested_at, view.tested_at);
        assert_eq!(
            latest.staff_tested_at.as_deref(),
            Some("2026-11-18T00:00:00Z")
        );
        assert_eq!(view.catalog_check, PublicCheck::Ok);
        assert_eq!(view.staff_check, PublicCheck::Protected);
        assert!(
            matches!(&view.logs,TunnelLog::Ready {events,..} if events==&vec!["tunnel-event-connected"])
        );
        assert!(!format!("{view:?}").contains("fictional-secret"));
    }
}
